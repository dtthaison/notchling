use notchling_engine::cluster::fence::{
    parse, FenceFile, Gate, FENCE_FILE, FENCE_VERSION, MAX_FENCE_BYTES,
};
use notchling_engine::cluster::members::Term;
use notchling_engine::cluster::persist;
use notchling_engine::cluster::NodeId;
use serde_json::Value;
use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

static WRITES: Mutex<()> = Mutex::new(());

fn serial() -> MutexGuard<'static, ()> {
    WRITES.lock().unwrap_or_else(|e| e.into_inner())
}

pub fn fence_path(cluster: &Path) -> PathBuf {
    let name = Path::new(FENCE_FILE)
        .file_name()
        .map_or_else(|| FENCE_FILE.into(), |n| n.to_os_string());
    cluster.join(name)
}

struct Held {
    fence: FenceFile,
    cluster: Option<String>,
}

fn load(cluster: &Path) -> Result<Option<Held>, String> {
    let path = fence_path(cluster);
    let shown = |why: &str| format!("{}: {why}", path.display());
    let meta = match std::fs::symlink_metadata(&path) {
        Ok(meta) => meta,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(shown(&e.to_string())),
    };
    if !meta.is_file() {
        return Err(shown("isn't a plain file"));
    }
    if meta.len() > MAX_FENCE_BYTES as u64 {
        return Err(shown("is too large"));
    }
    let bytes = std::fs::read(&path).map_err(|e| shown(&e.to_string()))?;
    let Some(fence) = parse(&bytes) else {
        return Err(shown(
            "is damaged or written by a newer Notchling, so it was left as it is",
        ));
    };
    let cluster = serde_json::from_slice::<Value>(&bytes)
        .ok()
        .and_then(|v| v.get("cluster").and_then(Value::as_str).map(str::to_string));
    Ok(Some(Held { fence, cluster }))
}

pub fn read_fence(cluster: &Path) -> Option<FenceFile> {
    load(cluster).ok().flatten().map(|held| held.fence)
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

fn put(cluster: &Path, fence: &FenceFile, stamp: Option<&str>) -> Result<(), String> {
    let mut value = serde_json::to_value(fence).map_err(|e| e.to_string())?;
    if let (Some(stamp), Some(object)) = (stamp, value.as_object_mut()) {
        object.insert("cluster".to_string(), Value::String(stamp.to_string()));
    }
    let bytes = serde_json::to_vec(&value).map_err(|e| e.to_string())?;
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
    let temp = path.with_extension(format!("json.{}.tmp", std::process::id()));
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

pub fn write_fence(cluster: &Path, fence: &FenceFile) -> Result<(), String> {
    let _serial = serial();
    put(cluster, fence, None)
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
    mark_moved_in(cluster, me, term, keys, to_name, None)
}

pub fn mark_moved_in(
    cluster: &Path,
    me: &NodeId,
    term: Term,
    keys: &[String],
    to_name: &str,
    cluster_id: Option<&str>,
) -> Result<(), String> {
    let _serial = serial();
    let held = load(cluster)?;
    let other = |held: &Held| {
        matches!(
            (cluster_id, held.cluster.as_deref()),
            (Some(now), Some(then)) if now != then
        )
    };
    let (mut fence, stamp) = match held {
        Some(held) if other(&held) => (blank(term, me), None),
        Some(Held { fence, cluster }) if fence.term <= term => {
            (FenceFile { term, ..fence }, cluster)
        }
        Some(Held { fence, cluster }) => (fence, cluster),
        None => (blank(term, me), None),
    };
    for key in keys {
        fence.sessions.insert(
            key.clone(),
            Gate::Moved {
                to: to_name.to_string(),
            },
        );
    }
    put(cluster, &fence, cluster_id.or(stamp.as_deref()))
}

pub fn clear_moved(cluster: &Path, keys: &[String]) -> Result<(), String> {
    let _serial = serial();
    let Some(Held {
        mut fence,
        cluster: stamp,
    }) = load(cluster)?
    else {
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
    put(cluster, &fence, stamp.as_deref())
}

fn remove(cluster: &Path) -> Result<(), String> {
    let path = fence_path(cluster);
    match std::fs::symlink_metadata(&path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(format!("{}: {e}", path.display())),
        Ok(meta) if meta.is_dir() => Err(format!("{} isn't a plain file", path.display())),
        Ok(_) => std::fs::remove_file(&path).map_err(|e| format!("{}: {e}", path.display())),
    }
}

pub fn wipe(cluster: &Path) -> Result<(), String> {
    let _serial = serial();
    remove(cluster)
}

pub fn drop_stale(cluster: &Path, current: Option<&str>) -> Result<(), String> {
    let _serial = serial();
    let Some(now) = current else {
        return remove(cluster);
    };
    let Some(held) = load(cluster)? else {
        return Ok(());
    };
    match held.cluster.as_deref() {
        Some(then) if then != now => remove(cluster),
        _ => Ok(()),
    }
}

pub fn drop_stale_at_start(cluster: &Path) -> Result<(), String> {
    match persist::load(cluster) {
        Ok(Some(state)) => drop_stale(cluster, Some(&state.id)),
        Ok(None) => wipe(cluster),
        Err(_) => Ok(()),
    }
}
