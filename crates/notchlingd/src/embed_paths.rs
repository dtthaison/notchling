use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::SystemTime;

pub const GRANITE_ID: &str = "granite-97m-r2@835ad14/p1";
pub const STATIC_ID: &str = "static-mrl@b68f412/512i8/p1";
pub const TABLE: &str = "table-512i8.bin";
pub const TABLE_HASH: &str = "table-512i8.sha256";

#[derive(Clone, Copy, PartialEq, Eq)]
struct FileStamp {
    len: u64,
    modified: SystemTime,
    #[cfg(unix)]
    ino: u64,
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct Stamp {
    table: FileStamp,
    hash: FileStamp,
}

fn memo() -> &'static Mutex<HashMap<PathBuf, Stamp>> {
    static MEMO: OnceLock<Mutex<HashMap<PathBuf, Stamp>>> = OnceLock::new();
    MEMO.get_or_init(|| Mutex::new(HashMap::new()))
}

fn lock_memo() -> std::sync::MutexGuard<'static, HashMap<PathBuf, Stamp>> {
    memo().lock().unwrap_or_else(|e| e.into_inner())
}

fn file_stamp(path: &Path) -> Option<FileStamp> {
    let meta = std::fs::symlink_metadata(path).ok()?;
    if !meta.file_type().is_file() {
        return None;
    }
    Some(FileStamp {
        len: meta.len(),
        modified: meta.modified().ok()?,
        #[cfg(unix)]
        ino: std::os::unix::fs::MetadataExt::ino(&meta),
    })
}

fn stamp_of(dir: &Path) -> Option<Stamp> {
    Some(Stamp {
        table: file_stamp(&dir.join(TABLE))?,
        hash: file_stamp(&dir.join(TABLE_HASH))?,
    })
}

fn sha256_file(path: &Path) -> Option<String> {
    let mut file = std::fs::File::open(path).ok()?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = file.read(&mut buf).ok()?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    let digest = hasher.finalize();
    Some(digest.iter().map(|b| format!("{b:02x}")).collect())
}

pub fn table_ready(dir: &Path) -> bool {
    let table = dir.join(TABLE);
    let hash_path = dir.join(TABLE_HASH);
    let Ok(table_meta) = std::fs::symlink_metadata(&table) else {
        return false;
    };
    let Ok(hash_meta) = std::fs::symlink_metadata(&hash_path) else {
        return false;
    };
    if !table_meta.file_type().is_file()
        || !hash_meta.file_type().is_file()
        || hash_meta.len() > 128
    {
        return false;
    }
    let Ok(saved) = std::fs::File::open(&hash_path) else {
        return false;
    };
    let mut raw = Vec::with_capacity(129);
    if saved.take(129).read_to_end(&mut raw).is_err() || raw.len() > 128 {
        return false;
    }
    let Ok(text) = std::str::from_utf8(&raw) else {
        return false;
    };
    let want = text.trim();
    !want.is_empty() && sha256_file(&table).as_deref() == Some(want)
}

pub fn models_dir(data: &Path) -> PathBuf {
    data.join("models")
}

pub fn granite_dir(data: &Path) -> PathBuf {
    models_dir(data).join("granite-97m-r2")
}

pub fn static_dir(data: &Path) -> PathBuf {
    models_dir(data).join("static-mrl")
}

pub fn vectors_db(data: &Path) -> PathBuf {
    data.join("daemon").join("vectors.db")
}

fn regular_dir(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_dir())
}

fn regular_file(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_file())
}

pub fn granite_present(data: &Path) -> bool {
    let dir = granite_dir(data);
    regular_dir(&models_dir(data))
        && regular_dir(&dir)
        && ["config.json", "model.safetensors", "tokenizer.json"]
            .iter()
            .all(|f| regular_file(&dir.join(f)))
}

pub fn static_present(data: &Path) -> bool {
    let dir = static_dir(data);
    if !regular_dir(&models_dir(data))
        || !regular_dir(&dir)
        || !regular_file(&dir.join(TABLE))
        || !regular_file(&dir.join("tokenizer.json"))
    {
        lock_memo().remove(&dir);
        return false;
    }
    let Some(stamp) = stamp_of(&dir) else {
        lock_memo().remove(&dir);
        return table_ready(&dir);
    };
    let hit = lock_memo().get(&dir).is_some_and(|saved| *saved == stamp);
    if hit {
        return true;
    }
    if table_ready(&dir) {
        if let Some(after) = stamp_of(&dir).filter(|s| *s == stamp) {
            lock_memo().insert(dir, after);
        }
        true
    } else {
        lock_memo().remove(&dir);
        false
    }
}

pub fn helper_exe(daemon_exe: &Path) -> PathBuf {
    let beside = daemon_exe.with_file_name("notchling-embed");
    match daemon_exe.parent().and_then(Path::parent) {
        Some(contents) => {
            let helpers = contents.join("Helpers").join("notchling-embed");
            if helpers.is_file() {
                helpers
            } else {
                beside
            }
        }
        None => beside,
    }
}
