use crate::cluster_sessions::{NodeSource, RemoteSessions, NODE_TAIL};
use crate::op_group::OpGroup;
use notchling_engine::bridge::net::crypto::{base64_decode, sha256_hex};
use notchling_engine::cluster::{NodeId, Qualified};
use notchling_local_api::{ops, ApiError, Caller, ErrorCode, OpSpec};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::Duration;

pub const MIRROR_DIR: &str = "mirror";
pub const FOLLOW_MS: u64 = 600_000;
pub const MAX_FOLLOWS: usize = 16;
const MAX_ROUNDS: usize = 8;
pub const MIRROR_CAP: u64 = 256 * 1024 * 1024;
const MAX_SEEN: usize = 1024;
const PULL_EVERY: Duration = Duration::from_secs(2);

pub type Clock = Arc<dyn Fn() -> u64 + Send + Sync>;
pub type OnCap = Arc<dyn Fn(&NodeId) + Send + Sync>;

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

pub fn mirror_path(
    dir: &Path,
    node: &NodeId,
    qualified: &str,
    remote_path: &str,
) -> Option<PathBuf> {
    let name = Path::new(remote_path).file_name()?.to_str()?;
    if name.is_empty() || name == ".." {
        return None;
    }
    let mut file = sha256_hex(qualified.as_bytes())[..16].to_string();
    if let Some(ext) = Path::new(name).extension().and_then(|e| e.to_str()) {
        if !ext.is_empty() && ext.len() <= 8 && ext.bytes().all(|b| b.is_ascii_alphanumeric()) {
            file.push('.');
            file.push_str(ext);
        }
    }
    Some(dir.join(node.as_str()).join(file))
}

fn is_real_dir(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok_and(|meta| meta.is_dir())
}

pub fn is_real_file(path: &Path) -> bool {
    let plain = std::fs::symlink_metadata(path).is_ok_and(|meta| meta.is_file());
    plain && folders_are_real(path)
}

fn folders_are_real(file: &Path) -> bool {
    let Some(folder) = file.parent() else {
        return false;
    };
    folder
        .ancestors()
        .take(3)
        .all(|dir| match std::fs::symlink_metadata(dir) {
            Ok(meta) => meta.is_dir(),
            Err(e) => e.kind() == std::io::ErrorKind::NotFound,
        })
}

fn ensure_real_dirs(folder: &Path) -> std::io::Result<()> {
    let refuse = || std::io::Error::other("a mirror folder is a link or not a folder");
    let mut chain: Vec<&Path> = folder.ancestors().take(3).collect();
    chain.reverse();
    for dir in chain {
        match std::fs::symlink_metadata(dir) {
            Ok(meta) if meta.is_dir() => {}
            Ok(_) => return Err(refuse()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                notchling_platform::files::ensure_private_dir(dir)
                    .map_err(std::io::Error::other)?;
                if !is_real_dir(dir) {
                    return Err(refuse());
                }
            }
            Err(e) => return Err(e),
        }
    }
    Ok(())
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
}

pub struct Mirror {
    dir: PathBuf,
    remote: Arc<RemoteSessions>,
    source: Arc<dyn TailSource>,
    clock: Clock,
    follows: Mutex<BTreeMap<String, Follow>>,
    seen: Mutex<BTreeMap<String, String>>,
    capped: Mutex<BTreeSet<String>>,
    cap: u64,
    on_cap: OnCap,
    epoch: AtomicU64,
    writing: Mutex<()>,
    nudged: Mutex<bool>,
    wake: Condvar,
    stop: AtomicBool,
}

fn write_chunk(path: &Path, data: &[u8], reset: bool) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        ensure_real_dirs(parent)?;
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
    if !folders_are_real(path) {
        return None;
    }
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
        Mirror::with_cap(dir, remote, source, clock, (MIRROR_CAP, Arc::new(|_| {})))
    }

    pub fn with_cap(
        dir: PathBuf,
        remote: Arc<RemoteSessions>,
        source: Arc<dyn TailSource>,
        clock: Clock,
        (cap, on_cap): (u64, OnCap),
    ) -> Arc<Mirror> {
        Arc::new(Mirror {
            dir,
            remote,
            source,
            clock,
            follows: Mutex::new(BTreeMap::new()),
            seen: Mutex::new(BTreeMap::new()),
            capped: Mutex::new(BTreeSet::new()),
            cap,
            on_cap,
            epoch: AtomicU64::new(0),
            writing: Mutex::new(()),
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
            .and_then(|path| mirror_path(&self.dir, &q.node, qualified, path))
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
            follows.insert(
                qualified.to_string(),
                Follow {
                    node: q.node,
                    id: q.id,
                    file,
                    until,
                },
            );
        }
        self.nudge();
        Ok(json!({"following": true, "id": qualified, "until": until}))
    }

    pub fn clear(&self) {
        self.epoch.fetch_add(1, Ordering::SeqCst);
        let _quiet = lock(&self.writing);
        lock(&self.follows).clear();
        lock(&self.seen).clear();
        lock(&self.capped).clear();
    }

    pub fn pull(&self) {
        let now = (self.clock)();
        let epoch = self.epoch.load(Ordering::SeqCst);
        let live: Vec<(String, Follow)> = {
            let mut follows = lock(&self.follows);
            follows.retain(|_, f| f.until > now);
            follows
                .iter()
                .map(|(k, f)| (k.clone(), f.clone()))
                .collect()
        };
        for (key, follow) in live {
            self.pull_one(&key, &follow, epoch);
        }
    }

    fn remember(&self, key: &str, seen: Option<String>) {
        let mut book = lock(&self.seen);
        match seen {
            Some(gen) => {
                if book.len() >= MAX_SEEN && !book.contains_key(key) {
                    let oldest = book.keys().next().cloned();
                    if let Some(oldest) = oldest {
                        book.remove(&oldest);
                    }
                }
                book.insert(key.to_string(), gen);
            }
            None => {
                book.remove(key);
            }
        }
    }

    fn store(
        &self,
        key: &str,
        follow: &Follow,
        chunk: (&[u8], bool, Option<String>),
        epoch: u64,
    ) -> Result<bool, ()> {
        let (data, reset, gen) = chunk;
        let _write = lock(&self.writing);
        if self.epoch.load(Ordering::SeqCst) != epoch {
            return Err(());
        }
        if reset || !data.is_empty() {
            let had = if reset {
                0
            } else {
                file_len(&follow.file).ok_or(())?
            };
            if had + data.len() as u64 > self.cap {
                lock(&self.follows).remove(key);
                if lock(&self.capped).insert(key.to_string()) {
                    (self.on_cap)(&follow.node);
                }
                return Ok(false);
            }
            write_chunk(&follow.file, data, reset).map_err(|_| ())?;
        }
        self.remember(key, gen);
        Ok(true)
    }

    fn pull_one(&self, key: &str, follow: &Follow, epoch: u64) {
        for _ in 0..MAX_ROUNDS {
            let Some(from) = file_len(&follow.file) else {
                return;
            };
            let seen = lock(&self.seen).get(key).cloned();
            let Ok(out) = self
                .source
                .tail(&follow.node, &follow.id, from, seen.as_deref())
            else {
                return;
            };
            let Some(data) = out
                .get("data")
                .and_then(Value::as_str)
                .and_then(base64_decode)
            else {
                return;
            };
            let reset = out.get("reset").and_then(Value::as_bool).unwrap_or(false);
            let gen = out.get("gen").and_then(Value::as_str).map(str::to_string);
            match self.store(key, follow, (&data, reset, gen), epoch) {
                Ok(true) => {}
                Ok(false) | Err(()) => return,
            }
            let next = out.get("next").and_then(Value::as_u64).unwrap_or(0);
            let size = out.get("size").and_then(Value::as_u64).unwrap_or(0);
            if data.is_empty() || next >= size {
                break;
            }
        }
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
