use notchling_engine::cluster::fence::{
    parse, FenceFile, Gate, FENCE_FILE, FENCE_VERSION, MAX_FENCE_BYTES,
};
use notchling_engine::cluster::members::Term;
use notchling_engine::cluster::NodeId;
use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};

pub fn fence_path(cluster: &Path) -> PathBuf {
    let name = Path::new(FENCE_FILE)
        .file_name()
        .map_or_else(|| FENCE_FILE.into(), |n| n.to_os_string());
    cluster.join(name)
}

pub fn read_fence(cluster: &Path) -> Option<FenceFile> {
    let path = fence_path(cluster);
    let meta = std::fs::symlink_metadata(&path).ok()?;
    if !meta.is_file() || meta.len() > MAX_FENCE_BYTES as u64 {
        return None;
    }
    parse(&std::fs::read(&path).ok()?)
}

#[cfg(unix)]
fn create_private(path: &Path) -> std::io::Result<std::fs::File> {
    use std::os::unix::fs::OpenOptionsExt;
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)
}

#[cfg(not(unix))]
fn create_private(path: &Path) -> std::io::Result<std::fs::File> {
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
}

pub fn write_fence(cluster: &Path, fence: &FenceFile) -> Result<(), String> {
    let bytes = serde_json::to_vec(fence).map_err(|e| e.to_string())?;
    if bytes.len() > MAX_FENCE_BYTES {
        return Err("the fence file would be too large".to_string());
    }
    let path = fence_path(cluster);
    let shown = |e: std::io::Error| format!("{}: {e}", path.display());
    std::fs::create_dir_all(cluster).map_err(shown)?;
    if let Ok(meta) = std::fs::symlink_metadata(&path) {
        if !meta.is_file() {
            return Err(format!("{} isn't a plain file", path.display()));
        }
    }
    let temp = path.with_extension("json.tmp");
    let _ = std::fs::remove_file(&temp);
    let mut file = create_private(&temp).map_err(shown)?;
    let written = file.write_all(&bytes).and_then(|_| file.sync_all());
    drop(file);
    if let Err(e) = written {
        let _ = std::fs::remove_file(&temp);
        return Err(shown(e));
    }
    std::fs::rename(&temp, &path).map_err(|e| {
        let _ = std::fs::remove_file(&temp);
        shown(e)
    })?;
    if let Ok(folder) = std::fs::File::open(cluster) {
        let _ = folder.sync_all();
    }
    Ok(())
}

fn blank(term: Term, node: &NodeId) -> FenceFile {
    FenceFile {
        v: FENCE_VERSION,
        term,
        node: node.clone(),
        boot_id: notchling_platform::clock::boot_id().unwrap_or_default(),
        valid_until: 0,
        sessions: BTreeMap::new(),
    }
}

pub fn mark_moved(
    cluster: &Path,
    me: &NodeId,
    term: Term,
    keys: &[String],
    to_name: &str,
) -> Result<(), String> {
    let mut fence = match read_fence(cluster) {
        Some(held) if held.term <= term => FenceFile { term, ..held },
        Some(held) => held,
        None => blank(term, me),
    };
    for key in keys {
        fence.sessions.insert(
            key.clone(),
            Gate::Moved {
                to: to_name.to_string(),
            },
        );
    }
    write_fence(cluster, &fence)
}

pub fn clear_moved(cluster: &Path, keys: &[String]) -> Result<(), String> {
    let Some(mut fence) = read_fence(cluster) else {
        return Ok(());
    };
    let before = fence.sessions.len();
    for key in keys {
        if matches!(fence.sessions.get(key), Some(Gate::Moved { .. })) {
            fence.sessions.remove(key);
        }
    }
    if fence.sessions.len() == before {
        return Ok(());
    }
    write_fence(cluster, &fence)
}
