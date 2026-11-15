use crate::cluster_blobs::{private_dir, remove_plain, Blobs};
use crate::cluster_node::ClusterNode;
use crate::cluster_node::LocalCalls;
use crate::snap_send::{
    facts_from_status, failed, Facts, NodeSnapPeers, SenderParts, SnapPeers, SnapSender, RETRY,
};
use notchling_engine::cluster::ack_book::AckBook;
use notchling_engine::cluster::blob::safe_name;
use notchling_engine::cluster::snapshot::Part;
use notchling_engine::cluster::standby_store::{
    StandbyStore, SNAP_ACK, SNAP_COMMIT, SNAP_OFFER, SNAP_PART,
};
use notchling_engine::cluster::wire::Origin;
use notchling_engine::cluster::NodeId;
use notchling_local_api::ApiError;
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, OnceLock};

pub const KEEP_SETS: usize = 2;
pub const SCRATCH_DIR: &str = "snap-in";
pub const SNAP_OPS: [&str; 4] = [SNAP_OFFER, SNAP_PART, SNAP_COMMIT, SNAP_ACK];

fn bad(why: impl Into<String>) -> ApiError {
    ApiError::bad_request(why)
}

pub struct SnapNode {
    cluster: PathBuf,
    blobs: Arc<Blobs>,
    peers: Arc<dyn SnapPeers>,
    facts: Arc<dyn Fn() -> Option<Facts> + Send + Sync>,
    book: Arc<AckBook>,
}

fn plain_rel(rel: &str) -> bool {
    !rel.is_empty()
        && !rel.contains('\n')
        && Path::new(rel)
            .components()
            .all(|c| matches!(c, Component::Normal(_)))
        && !rel.starts_with('/')
}

fn lengths_json(store: &StandbyStore) -> Value {
    let map: Map<String, Value> = store
        .lengths()
        .into_iter()
        .filter_map(|(path, len)| Some((path.to_str()?.to_string(), json!(len))))
        .collect();
    Value::Object(map)
}

fn field<'a>(args: &'a Value, name: &str) -> Result<&'a str, ApiError> {
    args.get(name)
        .and_then(Value::as_str)
        .ok_or_else(|| bad(format!("{name} is text")))
}

impl SnapNode {
    pub fn new(
        cluster: PathBuf,
        blobs: Arc<Blobs>,
        peers: Arc<dyn SnapPeers>,
        facts: Arc<dyn Fn() -> Option<Facts> + Send + Sync>,
        book: Arc<AckBook>,
    ) -> Arc<SnapNode> {
        Arc::new(SnapNode {
            cluster,
            blobs,
            peers,
            facts,
            book,
        })
    }

    fn standing(&self, origin: &Origin) -> Result<(Facts, NodeId), ApiError> {
        let facts = (self.facts)().ok_or_else(|| failed("this machine is in no cluster"))?;
        let home = facts
            .home
            .clone()
            .ok_or_else(|| failed("this machine is in no cluster"))?;
        if origin.node != home {
            return Err(bad("only the primary sends a snapshot set"));
        }
        Ok((facts, home))
    }

    fn store(&self, origin: &Origin) -> Result<(StandbyStore, NodeId), ApiError> {
        let (facts, home) = self.standing(origin)?;
        if home == facts.me {
            return Err(bad("the primary takes no snapshot set"));
        }
        let can = facts
            .members
            .iter()
            .any(|m| m.id == facts.me && m.active && m.can_be_master);
        if !can {
            return Err(bad("this machine isn't a Can Be Master machine"));
        }
        Ok((StandbyStore::new(&self.cluster, &home), home))
    }

    fn offer(&self, args: &Value, origin: &Origin) -> Result<Value, ApiError> {
        let (store, _) = self.store(origin)?;
        let manifest = serde_json::from_value(args.get("manifest").cloned().unwrap_or(Value::Null))
            .map_err(|_| bad("manifest is a snapshot manifest"))?;
        store.begin(&manifest).map_err(failed)?;
        Ok(json!({
            "missing": store.missing(&manifest),
            "lengths": lengths_json(&store),
        }))
    }

    fn part(&self, args: &Value, origin: &Origin) -> Result<Value, ApiError> {
        let (store, _) = self.store(origin)?;
        let set = field(args, "set")?;
        let part: Part = serde_json::from_value(args.get("part").cloned().unwrap_or(Value::Null))
            .map_err(|_| bad("part is a snapshot part"))?;
        if let Some(files) = args.get("files") {
            return self.dir_part(&store, set, &part, files);
        }
        let transfer = field(args, "transfer")?;
        let path = self.blobs.finished(transfer)?;
        let result = store.part(set, &part, &path).map_err(failed);
        self.blobs.discard(transfer);
        result.map(|()| json!({"ok": true}))
    }

    fn dir_part(
        &self,
        store: &StandbyStore,
        set: &str,
        part: &Part,
        files: &Value,
    ) -> Result<Value, ApiError> {
        let files = files.as_array().ok_or_else(|| bad("files is a list"))?;
        if !safe_name(set) {
            return Err(bad("set is a short name"));
        }
        let scratch = private_dir(&self.cluster, SCRATCH_DIR)
            .map_err(|e| failed(format!("the scratch folder: {e}")))?;
        let work = scratch.join(format!("{set}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&work);
        let result = self.assemble(&work, files).and_then(|()| {
            store
                .part(set, part, &work)
                .map_err(failed)
                .map(|()| json!({"ok": true}))
        });
        let _ = std::fs::remove_dir_all(&work);
        result
    }

    fn assemble(&self, work: &Path, files: &[Value]) -> Result<(), ApiError> {
        notchling_platform::files::ensure_private_dir(work).map_err(|e| failed(e.to_string()))?;
        let mut names: BTreeMap<String, String> = BTreeMap::new();
        for file in files {
            let rel = field(file, "rel")?;
            let transfer = field(file, "transfer")?;
            if !plain_rel(rel)
                || names
                    .insert(rel.to_string(), transfer.to_string())
                    .is_some()
            {
                return Err(bad(format!(
                    "{rel:?} is not a plain path inside the folder"
                )));
            }
        }
        for (rel, transfer) in &names {
            let from = self.blobs.finished(transfer)?;
            let to = work.join(rel);
            if let Some(parent) = to.parent() {
                notchling_platform::files::ensure_private_dir(parent)
                    .map_err(|e| failed(e.to_string()))?;
            }
            if std::fs::symlink_metadata(&to).is_ok() {
                remove_plain(&from);
                return Err(bad(format!("{rel:?} is in the folder twice")));
            }
            std::fs::rename(&from, &to).map_err(|e| failed(e.to_string()))?;
        }
        Ok(())
    }

    fn commit(&self, args: &Value, origin: &Origin) -> Result<Value, ApiError> {
        let (store, home) = self.store(origin)?;
        let set = field(args, "set")?;
        store.commit(set).map_err(failed)?;
        store.prune(KEEP_SETS);
        let lengths = lengths_json(&store);
        let _ = self
            .peers
            .call(&home, SNAP_ACK, json!({"set": set, "lengths": lengths}));
        Ok(json!({"set": set, "lengths": lengths}))
    }

    fn ack(&self, args: &Value, origin: &Origin) -> Result<Value, ApiError> {
        let facts = (self.facts)().ok_or_else(|| failed("this machine is in no cluster"))?;
        if facts.home.as_ref() != Some(&facts.me) {
            return Err(bad("only the primary records acknowledgements"));
        }
        let can = facts
            .members
            .iter()
            .any(|m| m.id == origin.node && m.id != facts.me && m.active && m.can_be_master);
        if !can {
            return Err(bad("that machine isn't a Can Be Master standby"));
        }
        let set = field(args, "set")?;
        if !safe_name(set) {
            return Err(bad("set is a short name"));
        }
        let lengths: BTreeMap<PathBuf, u64> = args
            .get("lengths")
            .and_then(Value::as_object)
            .ok_or_else(|| bad("lengths is a map of paths to numbers"))?
            .iter()
            .map(|(path, len)| {
                len.as_u64()
                    .map(|n| (PathBuf::from(path), n))
                    .ok_or_else(|| bad("lengths is a map of paths to numbers"))
            })
            .collect::<Result<_, _>>()?;
        self.book.record(&origin.node, set, lengths).map_err(bad)?;
        Ok(json!({"ok": true}))
    }

    pub fn call(&self, op: &str, args: &Value, origin: &Origin) -> Option<Result<Value, ApiError>> {
        match op {
            SNAP_OFFER => Some(self.offer(args, origin)),
            SNAP_PART => Some(self.part(args, origin)),
            SNAP_COMMIT => Some(self.commit(args, origin)),
            SNAP_ACK => Some(self.ack(args, origin)),
            _ => None,
        }
    }
}

pub struct Snap {
    pub sender: Arc<SnapSender>,
    pub node: Arc<SnapNode>,
}

pub fn build(
    cluster: PathBuf,
    blobs: Arc<Blobs>,
    holder: Arc<OnceLock<Arc<ClusterNode>>>,
    ready: Arc<dyn Fn(&NodeId) -> bool + Send + Sync>,
) -> Snap {
    let book = Arc::new(AckBook::new(&cluster));
    let peers: Arc<dyn SnapPeers> = Arc::new(NodeSnapPeers::new(holder.clone()));
    let facts_from = holder;
    let facts: Arc<dyn Fn() -> Option<Facts> + Send + Sync> = Arc::new(move || {
        let node = facts_from.get()?;
        facts_from_status(&node.status())
    });
    let sender = Arc::new(SnapSender::new(SenderParts {
        peers: peers.clone(),
        facts: facts.clone(),
        ready,
        book: book.clone(),
        retry: RETRY,
    }));
    let node = SnapNode::new(cluster, blobs, peers, facts, book);
    Snap { sender, node }
}

pub struct SnapCalls {
    inner: Arc<dyn LocalCalls>,
    node: Arc<SnapNode>,
}

impl SnapCalls {
    pub fn new(inner: Arc<dyn LocalCalls>, node: Arc<SnapNode>) -> SnapCalls {
        SnapCalls { inner, node }
    }
}

impl LocalCalls for SnapCalls {
    fn call(&self, op: &str, args: &Value, origin: &Origin) -> Result<Value, ApiError> {
        match self.node.call(op, args, origin) {
            Some(result) => result,
            None => self.inner.call(op, args, origin),
        }
    }
}

#[cfg(all(test, unix))]
#[path = "snap_recv_tests.rs"]
mod tests;
