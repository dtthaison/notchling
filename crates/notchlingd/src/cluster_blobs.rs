use notchling_engine::bridge::net::crypto::base64_decode;
use notchling_engine::cluster::blob::{safe_name, Incoming, CHUNK, MAX_BLOB};
use notchling_local_api::{ApiError, ErrorCode};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, SystemTime};

pub const INCOMING_DIR: &str = "incoming";
pub const OUTGOING_DIR: &str = "outgoing";
pub const MAX_OPEN: usize = 8;
pub const STALE: Duration = Duration::from_secs(3600);
pub const NODE_PUT: &str = "node.put";
pub const NODE_PUT_END: &str = "node.put.end";

struct Open {
    incoming: Incoming,
    file: File,
}

pub struct Blobs {
    cluster: PathBuf,
    cap: u64,
    open: Mutex<BTreeMap<String, Open>>,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

fn bad(why: impl Into<String>) -> ApiError {
    ApiError::bad_request(why)
}

fn failed(why: impl Into<String>) -> ApiError {
    ApiError::new(ErrorCode::Failed, why)
}

pub fn real_dir(path: &Path) -> std::io::Result<()> {
    let refuse = || std::io::Error::other("a cluster folder is a link or not a folder");
    match std::fs::symlink_metadata(path) {
        Ok(meta) if meta.is_dir() => Ok(()),
        Ok(_) => Err(refuse()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            notchling_platform::files::ensure_private_dir(path).map_err(std::io::Error::other)?;
            match std::fs::symlink_metadata(path) {
                Ok(meta) if meta.is_dir() => Ok(()),
                _ => Err(refuse()),
            }
        }
        Err(e) => Err(e),
    }
}

pub fn private_dir(cluster: &Path, name: &str) -> std::io::Result<PathBuf> {
    real_dir(cluster)?;
    let dir = cluster.join(name);
    real_dir(&dir)?;
    Ok(dir)
}

pub fn is_real_file(path: &Path) -> bool {
    let plain = std::fs::symlink_metadata(path).is_ok_and(|meta| meta.is_file());
    let parents_real = path.parent().is_some_and(|dir| {
        dir.ancestors()
            .take(2)
            .all(|d| std::fs::symlink_metadata(d).is_ok_and(|m| m.is_dir()))
    });
    plain && parents_real
}

pub fn create_private(path: &Path) -> std::io::Result<File> {
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
    }
    options.open(path)
}

pub fn remove_plain(path: &Path) {
    if let Ok(meta) = std::fs::symlink_metadata(path) {
        if !meta.is_dir() {
            let _ = std::fs::remove_file(path);
        }
    }
}

pub fn wipe(cluster: &Path) -> Result<(), String> {
    for name in [INCOMING_DIR, OUTGOING_DIR] {
        let dir = cluster.join(name);
        match std::fs::symlink_metadata(&dir) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.to_string()),
            Ok(meta) if meta.file_type().is_symlink() => {
                std::fs::remove_file(&dir).map_err(|e| e.to_string())?
            }
            Ok(_) => std::fs::remove_dir_all(&dir).map_err(|e| e.to_string())?,
        }
    }
    Ok(())
}

fn part_path(dir: &Path, transfer: &str) -> PathBuf {
    dir.join(format!("{transfer}.part"))
}

fn transfer_of(args: &Value) -> Result<&str, ApiError> {
    args.get("transfer")
        .and_then(Value::as_str)
        .filter(|t| safe_name(t))
        .ok_or_else(|| bad("transfer is a short name of letters, numbers, - and _"))
}

impl Blobs {
    pub fn new(cluster: PathBuf) -> Arc<Blobs> {
        Blobs::with_cap(cluster, MAX_BLOB)
    }

    pub fn with_cap(cluster: PathBuf, cap: u64) -> Arc<Blobs> {
        Arc::new(Blobs {
            cluster,
            cap,
            open: Mutex::new(BTreeMap::new()),
        })
    }

    fn dir(&self) -> Result<PathBuf, ApiError> {
        private_dir(&self.cluster, INCOMING_DIR)
            .map_err(|e| failed(format!("the incoming folder: {e}")))
    }

    pub fn sweep(&self, now: SystemTime) {
        let Ok(dir) = self.dir() else {
            return;
        };
        if let Ok(entries) = std::fs::read_dir(&dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                let old = std::fs::symlink_metadata(&path)
                    .and_then(|m| m.modified())
                    .ok()
                    .and_then(|t| now.duration_since(t).ok())
                    .is_some_and(|age| age > STALE);
                if old {
                    remove_plain(&path);
                }
            }
        }
        lock(&self.open).retain(|name, _| std::fs::symlink_metadata(part_path(&dir, name)).is_ok());
    }

    fn abort(&self, dir: &Path, transfer: &str) {
        lock(&self.open).remove(transfer);
        remove_plain(&part_path(dir, transfer));
    }

    pub fn put(&self, args: &Value) -> Result<Value, ApiError> {
        let transfer = transfer_of(args)?;
        let offset = args
            .get("offset")
            .and_then(Value::as_u64)
            .ok_or_else(|| bad("offset is a number"))?;
        let data = args
            .get("data")
            .and_then(Value::as_str)
            .and_then(base64_decode)
            .ok_or_else(|| bad("data is base64 text"))?;
        if data.len() > CHUNK {
            return Err(bad("a chunk is over 256 KB"));
        }
        let dir = self.dir()?;
        if offset == 0 {
            self.sweep(SystemTime::now());
            let mut open = lock(&self.open);
            if open.remove(transfer).is_none() && open.len() >= MAX_OPEN {
                return Err(failed("too many files are arriving at once"));
            }
            let part = part_path(&dir, transfer);
            remove_plain(&part);
            let file =
                create_private(&part).map_err(|e| failed(format!("the incoming file: {e}")))?;
            open.insert(
                transfer.to_string(),
                Open {
                    incoming: Incoming::with_cap(self.cap),
                    file,
                },
            );
        }
        let result = {
            let mut open = lock(&self.open);
            let entry = open
                .get_mut(transfer)
                .ok_or_else(|| bad("that file isn't arriving: start at offset 0"))?;
            entry
                .incoming
                .put(offset, &data)
                .map_err(bad)
                .and_then(|()| {
                    entry
                        .file
                        .write_all(&data)
                        .map_err(|e| failed(format!("writing the incoming file: {e}")))
                })
                .map(|()| entry.incoming.next())
        };
        match result {
            Ok(next) => Ok(json!({"next": next})),
            Err(e) => {
                self.abort(&dir, transfer);
                Err(e)
            }
        }
    }

    pub fn end(&self, args: &Value) -> Result<Value, ApiError> {
        let transfer = transfer_of(args)?;
        let sha256 = args
            .get("sha256")
            .and_then(Value::as_str)
            .ok_or_else(|| bad("sha256 is text"))?;
        let dir = self.dir()?;
        let entry = lock(&self.open)
            .remove(transfer)
            .ok_or_else(|| bad("that file isn't arriving"))?;
        let size = entry.incoming.next();
        let part = part_path(&dir, transfer);
        if let Err(why) = entry.incoming.finish(sha256) {
            remove_plain(&part);
            return Err(bad(why));
        }
        drop(entry.file);
        if let Err(e) = std::fs::rename(&part, dir.join(transfer)) {
            remove_plain(&part);
            return Err(failed(format!("keeping the incoming file: {e}")));
        }
        Ok(json!({"size": size}))
    }

    pub fn finished(&self, transfer: &str) -> Result<PathBuf, ApiError> {
        if !safe_name(transfer) {
            return Err(bad("transfer is a short name of letters, numbers, - and _"));
        }
        let path = self.dir()?.join(transfer);
        if is_real_file(&path) {
            Ok(path)
        } else {
            Err(failed("that file hasn't arrived"))
        }
    }

    pub fn discard(&self, transfer: &str) {
        if safe_name(transfer) {
            if let Ok(dir) = self.dir() {
                self.abort(&dir, transfer);
                remove_plain(&dir.join(transfer));
            }
        }
    }
}

#[cfg(test)]
#[path = "cluster_blobs_tests.rs"]
mod tests;
