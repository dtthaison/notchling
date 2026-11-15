use super::id::NodeId;
use super::standby_store::Acked;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

pub const ACK_FILE: &str = "snap-acks.json";
const MAX_FILE: u64 = 32 * 1024 * 1024;
pub const MAX_LENGTHS: usize = 100_000;

pub struct AckBook {
    path: PathBuf,
    acks: Mutex<BTreeMap<NodeId, Acked>>,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

fn read(path: &Path) -> BTreeMap<NodeId, Acked> {
    let Ok(meta) = std::fs::symlink_metadata(path) else {
        return BTreeMap::new();
    };
    if !meta.is_file() || meta.len() > MAX_FILE {
        return BTreeMap::new();
    }
    std::fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

impl AckBook {
    pub fn new(cluster_dir: &Path) -> AckBook {
        let path = cluster_dir.join(ACK_FILE);
        AckBook {
            acks: Mutex::new(read(&path)),
            path,
        }
    }

    pub fn get(&self, node: &NodeId) -> Option<Acked> {
        lock(&self.acks).get(node).cloned()
    }

    pub fn all(&self) -> BTreeMap<NodeId, Acked> {
        lock(&self.acks).clone()
    }

    pub fn record(
        &self,
        node: &NodeId,
        set: &str,
        lengths: BTreeMap<PathBuf, u64>,
    ) -> Result<(), String> {
        if lengths.len() > MAX_LENGTHS || lengths.keys().any(|p| !p.is_absolute()) {
            return Err("those lengths aren't a list of absolute paths".to_string());
        }
        let mut acks = lock(&self.acks);
        let before = acks.insert(
            node.clone(),
            Acked {
                set: set.to_string(),
                lengths,
            },
        );
        let bytes = serde_json::to_vec(&*acks).map_err(|e| e.to_string())?;
        let saved = match self.path.parent() {
            Some(dir) => notchling_platform::files::ensure_private_dir(dir),
            None => Ok(()),
        }
        .and_then(|()| notchling_platform::files::write_private_no_follow(&self.path, &bytes));
        if let Err(e) = saved {
            match before {
                Some(old) => acks.insert(node.clone(), old),
                None => acks.remove(node),
            };
            return Err(e.to_string());
        }
        Ok(())
    }

    pub fn forget(&self, node: &NodeId) {
        let mut acks = lock(&self.acks);
        if acks.remove(node).is_some() {
            if let Ok(bytes) = serde_json::to_vec(&*acks) {
                let _ = notchling_platform::files::write_private_no_follow(&self.path, &bytes);
            }
        }
    }
}
