use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};

const CHUNK: usize = 64 * 1024;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Identity {
    pub sha256: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct Key {
    path: PathBuf,
    size: u64,
    mtime: Option<std::time::SystemTime>,
    inode: u64,
}

#[derive(Default)]
pub struct IdentityCache {
    seen: HashMap<Key, Identity>,
    hashed: u64,
}

impl IdentityCache {
    pub fn new() -> IdentityCache {
        IdentityCache::default()
    }

    pub fn hashed(&self) -> u64 {
        self.hashed
    }

    pub fn identity(&mut self, path: &Path) -> Option<Identity> {
        let resolved = std::fs::canonicalize(path).ok()?;
        let meta = std::fs::metadata(&resolved).ok()?;
        if !meta.is_file() {
            return None;
        }
        let key = Key {
            path: resolved.clone(),
            size: meta.len(),
            mtime: meta.modified().ok(),
            inode: inode(&meta),
        };
        if let Some(found) = self.seen.get(&key) {
            return Some(found.clone());
        }
        let sha256 = hash_file(&resolved)?;
        self.hashed += 1;
        let identity = Identity { sha256 };
        self.seen.retain(|k, _| k.path != key.path);
        self.seen.insert(key, identity.clone());
        Some(identity)
    }
}

#[cfg(unix)]
fn inode(meta: &std::fs::Metadata) -> u64 {
    use std::os::unix::fs::MetadataExt;
    meta.ino()
}

#[cfg(not(unix))]
fn inode(_meta: &std::fs::Metadata) -> u64 {
    0
}

fn hash_file(path: &Path) -> Option<String> {
    let mut file = std::fs::File::open(path).ok()?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; CHUNK];
    loop {
        let n = file.read(&mut buf).ok()?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Some(
        hasher
            .finalize()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect(),
    )
}
