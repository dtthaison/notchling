use crate::cluster_sessions::{NodeSource, RemoteSessions, NODE_TAIL};
use crate::op_group::OpGroup;
use notchling_engine::bridge::net::crypto::base64_decode;
use notchling_engine::cluster::{NodeId, Qualified};
use notchling_local_api::{ops, ApiError, Caller, ErrorCode, OpSpec};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::Duration;

pub const MIRROR_DIR: &str = "mirror";
pub const FOLLOW_MS: u64 = 600_000;
pub const MAX_FOLLOWS: usize = 16;
const MAX_ROUNDS: usize = 8;
const PULL_EVERY: Duration = Duration::from_secs(2);

pub type Clock = Arc<dyn Fn() -> u64 + Send + Sync>;

pub trait TailSource: Send + Sync {
    fn tail(
        &self,
        node: &NodeId,
        id: &str,
        from: u64,
        seen: Option<&str>,
    ) -> Result<Value, ApiError>;
}

impl TailSource for NodeSource {
    fn tail(
        &self,
        node: &NodeId,
        id: &str,
        from: u64,
        seen: Option<&str>,
    ) -> Result<Value, ApiError> {
        let mut args = json!({"id": id, "from": from});
        if let Some(seen) = seen {
            args["gen"] = json!(seen);
        }
        self.ask(node, NODE_TAIL, args)
    }
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

fn bad(why: &str) -> ApiError {
    ApiError::new(ErrorCode::BadRequest, why)
}

pub fn mirror_path(dir: &Path, node: &NodeId, remote_path: &str) -> Option<PathBuf> {
    let name = Path::new(remote_path).file_name()?.to_str()?;
    if name.is_empty() || name == ".." {
        return None;
    }
    Some(dir.join(node.as_str()).join(name))
}

pub fn wipe(cluster_dir: &Path) -> Result<(), String> {
    let dir = cluster_dir.join(MIRROR_DIR);
    match std::fs::symlink_metadata(&dir) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.to_string()),
        Ok(meta) if meta.file_type().is_symlink() => {
            std::fs::remove_file(&dir).map_err(|e| e.to_string())
        }
        Ok(_) => std::fs::remove_dir_all(&dir).map_err(|e| e.to_string()),
    }
}

#[derive(Clone)]
struct Follow {
    node: NodeId,
    id: String,
    file: PathBuf,
    until: u64,
    seen: Option<String>,
}

pub struct Mirror {
    dir: PathBuf,
    remote: Arc<RemoteSessions>,
    source: Arc<dyn TailSource>,
    clock: Clock,
    follows: Mutex<BTreeMap<String, Follow>>,
    nudged: Mutex<bool>,
    wake: Condvar,
    stop: AtomicBool,
}

fn write_chunk(path: &Path, data: &[u8], reset: bool) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        notchling_platform::files::ensure_private_dir(parent).map_err(std::io::Error::other)?;
    }
    if let Ok(meta) = std::fs::symlink_metadata(path) {
        if !meta.is_file() {
            return Err(std::io::Error::other("the mirror file isn't a plain file"));
        }
    }
    let mut options = std::fs::OpenOptions::new();
    options.create(true);
    if reset {
        options.write(true).truncate(true);
    } else {
        options.append(true);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
    }
    options.open(path)?.write_all(data)
}

fn file_len(path: &Path) -> Option<u64> {
    match std::fs::symlink_metadata(path) {
        Ok(meta) if meta.is_file() => Some(meta.len()),
        Ok(_) => None,
        Err(_) => Some(0),
    }
}

impl Mirror {
    pub fn new(
        dir: PathBuf,
        remote: Arc<RemoteSessions>,
        source: Arc<dyn TailSource>,
        clock: Clock,
    ) -> Arc<Mirror> {
        Arc::new(Mirror {
            dir,
            remote,
            source,
            clock,
            follows: Mutex::new(BTreeMap::new()),
            nudged: Mutex::new(false),
            wake: Condvar::new(),
            stop: AtomicBool::new(false),
        })
    }

    pub fn following(&self) -> usize {
        let now = (self.clock)();
        let mut follows = lock(&self.follows);
        follows.retain(|_, f| f.until > now);
        follows.len()
    }

    pub fn follow_args(&self, args: &Value) -> Result<Value, ApiError> {
        let id = args
            .get("id")
            .and_then(Value::as_str)
            .ok_or_else(|| bad("cluster.follow takes {\"id\": \"<node>/<session>\"}"))?;
        self.follow(id)
    }

    pub fn follow(&self, qualified: &str) -> Result<Value, ApiError> {
        let q = Qualified::parse(qualified)
            .ok_or_else(|| bad("cluster.follow takes the id of a session on another node"))?;
        let row = self
            .remote
            .raw(&q.node, &q.id)
            .ok_or_else(|| ApiError::new(ErrorCode::Failed, "no such session on that node"))?;
        let file = row
            .get("transcriptPath")
            .and_then(Value::as_str)
            .and_then(|path| mirror_path(&self.dir, &q.node, path))
            .ok_or_else(|| ApiError::new(ErrorCode::Failed, "that session has no transcript"))?;
        let until = (self.clock)() + FOLLOW_MS;
        {
            let now = (self.clock)();
            let mut follows = lock(&self.follows);
            follows.retain(|_, f| f.until > now);
            if !follows.contains_key(qualified) && follows.len() >= MAX_FOLLOWS {
                return Err(ApiError::new(
                    ErrorCode::Failed,
                    "too many sessions are followed at once",
                ));
            }
            let seen = follows.get(qualified).and_then(|f| f.seen.clone());
            follows.insert(
                qualified.to_string(),
                Follow {
                    node: q.node,
                    id: q.id,
                    file,
                    until,
                    seen,
                },
            );
        }
        self.nudge();
        Ok(json!({"following": true, "id": qualified, "until": until}))
    }

    pub fn clear(&self) {
        lock(&self.follows).clear();
    }

    pub fn pull(&self) {
        let now = (self.clock)();
        let live: Vec<(String, Follow)> = {
            let mut follows = lock(&self.follows);
            follows.retain(|_, f| f.until > now);
            follows
                .iter()
                .map(|(k, f)| (k.clone(), f.clone()))
                .collect()
        };
        for (key, follow) in live {
            let seen = self.pull_one(&follow);
            if let Some(slot) = lock(&self.follows).get_mut(&key) {
                slot.seen = seen;
            }
        }
    }

    fn pull_one(&self, follow: &Follow) -> Option<String> {
        let mut seen = follow.seen.clone();
        for _ in 0..MAX_ROUNDS {
            let Some(from) = file_len(&follow.file) else {
                return seen;
            };
            let Ok(out) = self
                .source
                .tail(&follow.node, &follow.id, from, seen.as_deref())
            else {
                return seen;
            };
            let Some(data) = out
                .get("data")
                .and_then(Value::as_str)
                .and_then(base64_decode)
            else {
                return seen;
            };
            let reset = out.get("reset").and_then(Value::as_bool).unwrap_or(false);
            if (reset || !data.is_empty()) && write_chunk(&follow.file, &data, reset).is_err() {
                return seen;
            }
            seen = out.get("gen").and_then(Value::as_str).map(str::to_string);
            let next = out.get("next").and_then(Value::as_u64).unwrap_or(0);
            let size = out.get("size").and_then(Value::as_u64).unwrap_or(0);
            if data.is_empty() || next >= size {
                break;
            }
        }
        seen
    }

    pub fn nudge(&self) {
        *lock(&self.nudged) = true;
        self.wake.notify_all();
    }

    fn wait(&self) {
        let mut nudged = lock(&self.nudged);
        if !*nudged {
            nudged = self
                .wake
                .wait_timeout(nudged, PULL_EVERY)
                .unwrap_or_else(|e| e.into_inner())
                .0;
        }
        *nudged = false;
    }

    pub fn spawn(self: &Arc<Mirror>) {
        let mirror = self.clone();
        let spawned = std::thread::Builder::new()
            .name("cluster-mirror".into())
            .spawn(move || {
                while !mirror.stop.load(Ordering::SeqCst) {
                    mirror.pull();
                    mirror.wait();
                }
            });
        if spawned.is_err() {
            self.stop.store(true, Ordering::SeqCst);
        }
    }

    pub fn close(&self) {
        self.stop.store(true, Ordering::SeqCst);
        self.nudge();
    }
}

pub struct FollowOps {
    mirror: Arc<Mirror>,
}

impl FollowOps {
    pub fn new(mirror: Arc<Mirror>) -> FollowOps {
        FollowOps { mirror }
    }
}

impl OpGroup for FollowOps {
    fn spec(&self, op: &str) -> Option<OpSpec> {
        (op == ops::CLUSTER_FOLLOW).then_some(OpSpec::APP)
    }

    fn call(&self, _caller: &Caller, op: &str, args: &Value) -> Result<Value, ApiError> {
        match op {
            ops::CLUSTER_FOLLOW => self.mirror.follow_args(args),
            _ => Err(ApiError::internal(format!("{op} has no handler"))),
        }
    }
}

#[cfg(test)]
#[path = "cluster_mirror_tests.rs"]
mod tests;
