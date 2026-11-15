use crate::cluster_blobs::{NODE_PUT, NODE_PUT_END};
use crate::cluster_node::ClusterNode;
use crate::cluster_workspace_send::WORKSPACE_WAIT;
use notchling_engine::bridge::net::crypto::base64;
use notchling_engine::cluster::ack_book::AckBook;
use notchling_engine::cluster::blob::{chunks, hex_of, safe_name, Digest, CHUNK, MAX_BLOB};
use notchling_engine::cluster::core::LocalCmd;
use notchling_engine::cluster::snapshot::{staged_path, Manifest, Part, PartKind};
use notchling_engine::cluster::standby_store::{SNAP_COMMIT, SNAP_OFFER, SNAP_PART};
use notchling_engine::cluster::wire::{Origin, OriginTier};
use notchling_engine::cluster::NodeId;
use notchling_local_api::{ApiError, ErrorCode};
use serde_json::{json, Value};
use sha2::{Digest as _, Sha256};
use std::collections::BTreeMap;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, OnceLock};
use std::time::{Duration, Instant};

pub const RETRY: Duration = Duration::from_secs(10);
pub const MAX_ROUNDS: usize = 2;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Candidate {
    pub id: NodeId,
    pub can_be_master: bool,
    pub online: bool,
    pub active: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Facts {
    pub me: NodeId,
    pub home: Option<NodeId>,
    pub members: Vec<Candidate>,
}

pub trait SnapPeers: Send + Sync {
    fn call(&self, node: &NodeId, op: &str, args: Value) -> Result<Value, ApiError>;
}

pub struct NodeSnapPeers {
    node: Arc<OnceLock<Arc<ClusterNode>>>,
}

impl NodeSnapPeers {
    pub fn new(node: Arc<OnceLock<Arc<ClusterNode>>>) -> NodeSnapPeers {
        NodeSnapPeers { node }
    }
}

impl SnapPeers for NodeSnapPeers {
    fn call(&self, node: &NodeId, op: &str, args: Value) -> Result<Value, ApiError> {
        let me = self
            .node
            .get()
            .ok_or_else(|| failed("this machine can't be a cluster node"))?;
        let origin = Origin {
            node: me.me().clone(),
            tier: OriginTier::Node,
        };
        me.local_within(
            LocalCmd::Forward {
                node: node.clone(),
                op: op.to_string(),
                args,
                origin,
            },
            WORKSPACE_WAIT,
        )
    }
}

pub fn facts_from_status(status: &Value) -> Option<Facts> {
    let me = NodeId::parse(status.get("me")?.as_str()?)?;
    let home = status
        .get("cluster")
        .and_then(|c| c.get("home"))
        .and_then(Value::as_str)
        .and_then(NodeId::parse);
    let members = status
        .get("members")?
        .as_array()?
        .iter()
        .filter_map(|m| {
            Some(Candidate {
                id: NodeId::parse(m.get("id")?.as_str()?)?,
                can_be_master: m.get("canBeMaster")?.as_bool()?,
                online: m.get("online")?.as_bool().unwrap_or(false),
                active: m.get("state")?.as_str() == Some("active"),
            })
        })
        .collect();
    Some(Facts { me, home, members })
}

pub(crate) fn failed(why: impl Into<String>) -> ApiError {
    ApiError::new(ErrorCode::Failed, why)
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

#[derive(Clone, Debug)]
pub struct Offered {
    pub manifest: Manifest,
    pub dir: PathBuf,
}

#[derive(Default)]
struct Slot {
    busy: bool,
    queued: Option<Offered>,
    sent: Option<String>,
    failed: Option<Instant>,
    error: Option<String>,
}

#[derive(Default)]
struct State {
    latest: Option<Offered>,
    slots: BTreeMap<NodeId, Slot>,
}

pub struct SenderParts {
    pub peers: Arc<dyn SnapPeers>,
    pub facts: Arc<dyn Fn() -> Option<Facts> + Send + Sync>,
    pub ready: Arc<dyn Fn(&NodeId) -> bool + Send + Sync>,
    pub book: Arc<AckBook>,
    pub retry: Duration,
}

struct Inner {
    parts: SenderParts,
    state: Mutex<State>,
    idle: Condvar,
}

pub struct SnapSender {
    inner: Arc<Inner>,
}

impl SnapSender {
    pub fn new(parts: SenderParts) -> SnapSender {
        SnapSender {
            inner: Arc::new(Inner {
                parts,
                state: Mutex::new(State::default()),
                idle: Condvar::new(),
            }),
        }
    }

    pub fn book(&self) -> &Arc<AckBook> {
        &self.inner.parts.book
    }

    pub fn offer(&self, manifest: Manifest, dir: PathBuf) -> Vec<NodeId> {
        let offered = Offered { manifest, dir };
        lock(&self.inner.state).latest = Some(offered.clone());
        let targets = self.inner.targets();
        for id in &targets {
            Inner::enqueue(&self.inner, id, offered.clone());
        }
        targets
    }

    pub fn tick(&self) -> Vec<NodeId> {
        let mut started = Vec::new();
        for id in self.inner.targets() {
            let next = {
                let state = lock(&self.inner.state);
                let Some(latest) = state.latest.clone() else {
                    return started;
                };
                let slot = state.slots.get(&id);
                let behind = slot.is_none_or(|s| s.sent.as_deref() != Some(&latest.manifest.set));
                let waiting = slot.is_some_and(|s| {
                    s.busy
                        || s.failed
                            .is_some_and(|at| at.elapsed() < self.inner.parts.retry)
                });
                (behind && !waiting).then_some(latest)
            };
            if let Some(offered) = next {
                Inner::enqueue(&self.inner, &id, offered);
                started.push(id);
            }
        }
        started
    }

    pub fn sent(&self, node: &NodeId) -> Option<String> {
        lock(&self.inner.state)
            .slots
            .get(node)
            .and_then(|s| s.sent.clone())
    }

    pub fn last_error(&self, node: &NodeId) -> Option<String> {
        lock(&self.inner.state)
            .slots
            .get(node)
            .and_then(|s| s.error.clone())
    }

    pub fn wait_idle(&self, within: Duration) -> bool {
        let deadline = Instant::now() + within;
        let mut state = lock(&self.inner.state);
        while state.slots.values().any(|s| s.busy) {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return false;
            }
            state = self
                .inner
                .idle
                .wait_timeout(state, left)
                .unwrap_or_else(|e| e.into_inner())
                .0;
        }
        true
    }
}

impl Inner {
    fn targets(&self) -> Vec<NodeId> {
        let Some(facts) = (self.parts.facts)() else {
            return Vec::new();
        };
        if facts.home.as_ref() != Some(&facts.me) {
            return Vec::new();
        }
        facts
            .members
            .iter()
            .filter(|m| m.id != facts.me && m.active && m.can_be_master && m.online)
            .filter(|m| (self.parts.ready)(&m.id))
            .map(|m| m.id.clone())
            .collect()
    }

    fn enqueue(this: &Arc<Inner>, id: &NodeId, offered: Offered) {
        {
            let mut state = lock(&this.state);
            let slot = state.slots.entry(id.clone()).or_default();
            if slot.busy {
                slot.queued = Some(offered);
                return;
            }
            slot.busy = true;
        }
        let inner = this.clone();
        let target = id.clone();
        let spawned = std::thread::Builder::new()
            .name("snap-send".into())
            .spawn(move || inner.drain(&target, offered));
        if spawned.is_err() {
            let mut state = lock(&this.state);
            if let Some(slot) = state.slots.get_mut(id) {
                slot.busy = false;
                slot.failed = Some(Instant::now());
                slot.error = Some("no thread to send with".to_string());
            }
            this.idle.notify_all();
        }
    }

    fn drain(&self, id: &NodeId, first: Offered) {
        let mut offered = first;
        loop {
            let result = if self.targets().contains(id) {
                self.send(id, &offered)
            } else {
                Err("that machine is not ready or not online".to_string())
            };
            let mut state = lock(&self.state);
            let slot = state.slots.entry(id.clone()).or_default();
            match result {
                Ok(()) => {
                    slot.sent = Some(offered.manifest.set.clone());
                    slot.failed = None;
                    slot.error = None;
                }
                Err(why) => {
                    slot.failed = Some(Instant::now());
                    slot.error = Some(why);
                }
            }
            match slot.queued.take() {
                Some(next) => offered = next,
                None => {
                    slot.busy = false;
                    drop(state);
                    self.idle.notify_all();
                    return;
                }
            }
        }
    }

    fn call(&self, id: &NodeId, op: &str, args: Value) -> Result<Value, String> {
        self.parts.peers.call(id, op, args).map_err(|e| e.message)
    }

    fn send(&self, id: &NodeId, offered: &Offered) -> Result<(), String> {
        let mut lengths = self
            .parts
            .book
            .get(id)
            .map(|acked| acked.lengths)
            .unwrap_or_default();
        let mut round = 0;
        let (manifest, missing) = loop {
            round += 1;
            let manifest = for_target(offered, &lengths)?;
            let reply = self.call(id, SNAP_OFFER, json!({"manifest": manifest}))?;
            let actual = lengths_of(&reply);
            let missing: Vec<Part> =
                serde_json::from_value(reply.get("missing").cloned().unwrap_or(Value::Null))
                    .map_err(|_| "the other machine's answer to an offer is damaged".to_string())?;
            if tails_agree(&manifest, &actual) {
                break (manifest, missing);
            }
            if round >= MAX_ROUNDS {
                return Err("the other machine's transcripts keep changing".to_string());
            }
            lengths = actual;
        };
        for (index, part) in manifest.parts.iter().enumerate() {
            if missing.contains(part) {
                self.send_part(id, offered, &manifest.set, index, part)?;
            }
        }
        let reply = self.call(id, SNAP_COMMIT, json!({"set": manifest.set}))?;
        let _ = self
            .parts
            .book
            .record(id, &manifest.set, lengths_of(&reply));
        Ok(())
    }

    fn send_part(
        &self,
        id: &NodeId,
        offered: &Offered,
        set: &str,
        index: usize,
        part: &Part,
    ) -> Result<(), String> {
        let src = staged_path(&offered.dir, &part.path)
            .ok_or_else(|| format!("{} can't be sent", part.path.display()))?;
        let name = format!("p-{set}-{index}");
        if !safe_name(&name) {
            return Err(format!("{set} is not a set name"));
        }
        if part.kind == PartKind::Dir {
            let mut files = Vec::new();
            walk(&src, "", &mut files)?;
            let mut listed = Vec::with_capacity(files.len());
            for (n, (rel, path)) in files.iter().enumerate() {
                let transfer = format!("{name}-{n}");
                if !safe_name(&transfer) {
                    return Err("a folder has too many files".to_string());
                }
                let len = std::fs::symlink_metadata(path)
                    .map_err(|e| format!("{}: {e}", path.display()))?
                    .len();
                self.push(id, &transfer, path, (0, len), None)?;
                listed.push(json!({"rel": rel, "transfer": transfer}));
            }
            let args = json!({"set": set, "part": part, "files": listed});
            self.call(id, SNAP_PART, args)?;
            return Ok(());
        }
        let skip = match part.kind {
            PartKind::Tail { from } => from.saturating_sub(staged_from(offered, &part.path)),
            _ => 0,
        };
        self.push(id, &name, &src, (skip, part.len), Some(&part.sha256))?;
        let args = json!({"set": set, "part": part, "transfer": name});
        self.call(id, SNAP_PART, args)?;
        Ok(())
    }

    fn push(
        &self,
        id: &NodeId,
        transfer: &str,
        path: &Path,
        (skip, len): (u64, u64),
        want: Option<&str>,
    ) -> Result<(), String> {
        if len > MAX_BLOB {
            return Err(format!("{} is over 256 MB", path.display()));
        }
        let mut file = open_plain(path)?;
        file.seek(SeekFrom::Start(skip))
            .map_err(|e| format!("{}: {e}", path.display()))?;
        let mut digest = Digest::default();
        let mut pieces = chunks(len);
        if pieces.is_empty() {
            pieces.push((0, 0));
        }
        for (offset, size) in pieces {
            let mut data = vec![0; size];
            file.read_exact(&mut data)
                .map_err(|e| format!("{} changed while it was sent: {e}", path.display()))?;
            digest.update(&data);
            let args = json!({"transfer": transfer, "offset": offset, "data": base64(&data)});
            let reply = self.call(id, NODE_PUT, args)?;
            if reply.get("next").and_then(Value::as_u64) != Some(offset + size as u64) {
                return Err("the other machine lost count of what it received".to_string());
            }
        }
        let sha256 = digest.finish();
        if want.is_some_and(|w| w != sha256) {
            return Err(format!("{} changed after it was staged", path.display()));
        }
        let args = json!({"transfer": transfer, "sha256": sha256});
        self.call(id, NODE_PUT_END, args)?;
        Ok(())
    }
}

fn open_plain(path: &Path) -> Result<File, String> {
    let meta = std::fs::symlink_metadata(path).map_err(|e| format!("{}: {e}", path.display()))?;
    if !meta.is_file() {
        return Err(format!("{} is not a plain file", path.display()));
    }
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW);
    }
    options
        .open(path)
        .map_err(|e| format!("{}: {e}", path.display()))
}

fn walk(dir: &Path, rel: &str, out: &mut Vec<(String, PathBuf)>) -> Result<(), String> {
    let mut entries: Vec<_> = std::fs::read_dir(dir)
        .map_err(|e| format!("{}: {e}", dir.display()))?
        .filter_map(Result::ok)
        .collect();
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        let path = entry.path();
        let Some(name) = entry.file_name().to_str().map(str::to_string) else {
            return Err(format!("{} has a name that is not text", path.display()));
        };
        let rel_name = if rel.is_empty() {
            name
        } else {
            format!("{rel}/{name}")
        };
        let meta =
            std::fs::symlink_metadata(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        if meta.is_dir() {
            walk(&path, &rel_name, out)?;
        } else if meta.is_file() {
            out.push((rel_name, path));
        } else {
            return Err(format!("{} is a link or special file", path.display()));
        }
    }
    Ok(())
}

pub(crate) fn lengths_of(reply: &Value) -> BTreeMap<PathBuf, u64> {
    reply
        .get("lengths")
        .and_then(Value::as_object)
        .map(|map| {
            map.iter()
                .filter_map(|(path, len)| Some((PathBuf::from(path), len.as_u64()?)))
                .collect()
        })
        .unwrap_or_default()
}

fn tails_agree(manifest: &Manifest, actual: &BTreeMap<PathBuf, u64>) -> bool {
    manifest.parts.iter().all(|part| match part.kind {
        PartKind::Tail { from } if from > 0 => {
            let have = actual.get(&part.path).copied().unwrap_or(0);
            have == from || have == from + part.len
        }
        _ => true,
    })
}

fn staged_from(offered: &Offered, path: &Path) -> u64 {
    offered
        .manifest
        .parts
        .iter()
        .find_map(|p| match p.kind {
            PartKind::Tail { from } if p.path == path => Some(from),
            _ => None,
        })
        .unwrap_or(0)
}

fn hash_range(path: &Path, skip: u64, len: u64) -> Result<String, String> {
    let mut file = open_plain(path)?;
    file.seek(SeekFrom::Start(skip))
        .map_err(|e| format!("{}: {e}", path.display()))?;
    let mut hasher = Sha256::new();
    let mut block = vec![0; CHUNK];
    let mut left = len;
    while left > 0 {
        let want = left.min(CHUNK as u64) as usize;
        file.read_exact(&mut block[..want])
            .map_err(|e| format!("{} changed: {e}", path.display()))?;
        hasher.update(&block[..want]);
        left -= want as u64;
    }
    Ok(hex_of(&hasher.finalize()))
}

pub fn for_target(offered: &Offered, lengths: &BTreeMap<PathBuf, u64>) -> Result<Manifest, String> {
    let mut manifest = offered.manifest.clone();
    for part in &mut manifest.parts {
        let PartKind::Tail { from } = part.kind else {
            continue;
        };
        if from == 0 {
            continue;
        }
        let have = lengths.get(&part.path).copied().unwrap_or(0);
        if have == from {
            continue;
        }
        let end = from + part.len;
        if have < from || have > end {
            return Err(format!(
                "{} is at {have} there, outside what this set carries ({from} to {end})",
                part.path.display()
            ));
        }
        let skip = have - from;
        let src = staged_path(&offered.dir, &part.path)
            .ok_or_else(|| format!("{} can't be sent", part.path.display()))?;
        part.sha256 = hash_range(&src, skip, part.len - skip)?;
        part.len -= skip;
        part.kind = PartKind::Tail { from: have };
    }
    Ok(manifest)
}

#[cfg(all(test, unix))]
#[path = "snap_send_tests.rs"]
mod tests;
