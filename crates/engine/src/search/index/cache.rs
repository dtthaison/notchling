//! The index's cache file: `{"version": N, "sessions": [{"meta": {...}, "text": "..."}]}`, the
//! shape of the Swift app's `search-index.json`. A file with another version, or one that does not
//! parse, is ignored and the index is rebuilt.

use super::CACHE_VERSION;
use crate::search::{IndexedSession, SessionMeta};
use serde::{Deserialize, Serialize};
use std::io::Read;
use std::path::Path;

/// The most of a cache file read; a bigger one is not ours.
const MAX_CACHE_BYTES: u64 = 256 * 1024 * 1024;

#[derive(Serialize, Deserialize)]
struct CacheFile {
    version: u32,
    sessions: Vec<Record>,
}

#[derive(Serialize, Deserialize)]
struct Record {
    meta: SessionMeta,
    text: String,
}

/// The sessions in the cache at `path`; empty when it is missing, damaged or another version.
pub(super) fn load(path: &Path) -> Vec<IndexedSession> {
    let Ok(file) = std::fs::File::open(path) else {
        return Vec::new();
    };
    let mut data = Vec::new();
    if file.take(MAX_CACHE_BYTES).read_to_end(&mut data).is_err() {
        return Vec::new();
    }
    match serde_json::from_slice::<CacheFile>(&data) {
        Ok(f) if f.version == CACHE_VERSION => f
            .sessions
            .into_iter()
            .map(|r| IndexedSession::new(r.meta, r.text))
            .collect(),
        _ => Vec::new(),
    }
}

/// Writes the cache (0600, atomically).
pub(super) fn save(path: &Path, sessions: &[IndexedSession]) -> Result<(), String> {
    let file = CacheFile {
        version: CACHE_VERSION,
        sessions: sessions
            .iter()
            .map(|s| Record {
                meta: s.meta.clone(),
                text: s.text.clone(),
            })
            .collect(),
    };
    let data = serde_json::to_vec(&file).map_err(|e| e.to_string())?;
    if let Some(parent) = path.parent() {
        notchling_platform::files::ensure_private_dir(parent).map_err(|e| e.to_string())?;
    }
    notchling_platform::files::write_private_no_follow(path, &data).map_err(|e| e.to_string())
}
