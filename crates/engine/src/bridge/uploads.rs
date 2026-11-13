use super::connection::lock;
use std::collections::HashMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

pub const MAX_BYTES: usize = 6 * 1024 * 1024;
pub const MAX_CHUNK_BYTES: usize = 160 * 1024;
pub const CHUNK_BYTES: usize = 96 * 1024;
pub const EXPIRES_AFTER_MS: f64 = 120_000.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UploadError {
    Unknown,
    OutOfOrder,
    TooBig,
    Incomplete,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Upload {
    pub connection_id: u64,
    pub session_id: String,
    pub expected_bytes: usize,
    pub caption: String,
    pub started_ms: f64,
    pub data: Vec<u8>,
    pub next_sequence: i64,
}

#[derive(Default)]
pub struct Uploads {
    uploads: Mutex<HashMap<String, Upload>>,
    counter: AtomicU64,
}

impl Uploads {
    pub fn begin(
        &self,
        connection_id: u64,
        session_id: &str,
        bytes: usize,
        caption: &str,
        now_ms: f64,
    ) -> String {
        let mut uploads = lock(&self.uploads);
        uploads.retain(|_, u| {
            u.connection_id != connection_id && now_ms - u.started_ms < EXPIRES_AFTER_MS
        });
        let n = self.counter.fetch_add(1, Ordering::Relaxed);
        let id = unique_id(&format!("upload|{connection_id}|{n}|{now_ms}"));
        uploads.insert(
            id.clone(),
            Upload {
                connection_id,
                session_id: session_id.to_string(),
                expected_bytes: bytes,
                caption: caption.to_string(),
                started_ms: now_ms,
                data: Vec::new(),
                next_sequence: 0,
            },
        );
        id
    }

    pub fn sweep(&self, now_ms: f64) {
        lock(&self.uploads).retain(|_, u| now_ms - u.started_ms < EXPIRES_AFTER_MS);
    }

    pub fn pending(&self) -> usize {
        lock(&self.uploads).len()
    }

    pub fn append(
        &self,
        id: &str,
        connection_id: u64,
        sequence: i64,
        chunk: &[u8],
        now_ms: f64,
    ) -> Result<(), UploadError> {
        let mut uploads = lock(&self.uploads);
        uploads.retain(|_, u| now_ms - u.started_ms < EXPIRES_AFTER_MS);
        let upload = uploads
            .get_mut(id)
            .filter(|u| u.connection_id == connection_id)
            .ok_or(UploadError::Unknown)?;
        if sequence != upload.next_sequence {
            return Err(UploadError::OutOfOrder);
        }
        if chunk.len() > MAX_CHUNK_BYTES || upload.data.len() + chunk.len() > upload.expected_bytes
        {
            return Err(UploadError::TooBig);
        }
        upload.data.extend_from_slice(chunk);
        upload.next_sequence += 1;
        Ok(())
    }

    pub fn finish(&self, id: &str, connection_id: u64, now_ms: f64) -> Result<Upload, UploadError> {
        let mut uploads = lock(&self.uploads);
        uploads.retain(|_, u| now_ms - u.started_ms < EXPIRES_AFTER_MS);
        if uploads.get(id).map(|u| u.connection_id) != Some(connection_id) {
            return Err(UploadError::Unknown);
        }
        let upload = uploads.remove(id).ok_or(UploadError::Unknown)?;
        if upload.data.len() != upload.expected_bytes {
            return Err(UploadError::Incomplete);
        }
        Ok(upload)
    }

    pub fn drop_connection(&self, connection_id: u64) {
        lock(&self.uploads).retain(|_, u| u.connection_id != connection_id);
    }
}

fn unique_id(seed: &str) -> String {
    let salt = format!(
        "{seed}|{}|{:?}",
        std::process::id(),
        std::time::SystemTime::now()
    );
    super::foreign::stable_uuid(&salt)
}

#[derive(Debug, Clone)]
pub struct ImageStore {
    pub directory: PathBuf,
    pub keep_for_ms: f64,
}

impl ImageStore {
    pub fn new(directory: PathBuf) -> ImageStore {
        ImageStore {
            directory,
            keep_for_ms: 24.0 * 3_600_000.0,
        }
    }

    pub fn detect(data: &[u8]) -> Option<&'static str> {
        let b = &data[..data.len().min(12)];
        if b.starts_with(&[0xFF, 0xD8, 0xFF]) {
            Some("jpg")
        } else if b.starts_with(&[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A]) {
            Some("png")
        } else if b.len() >= 6 && b.starts_with(b"GIF8") {
            Some("gif")
        } else if b.len() >= 12 && b.starts_with(b"RIFF") && &b[8..12] == b"WEBP" {
            Some("webp")
        } else {
            None
        }
    }

    pub fn save(&self, data: &[u8], ext: &str, now_ms: f64) -> std::io::Result<PathBuf> {
        let plain = (1..=8).contains(&ext.len()) && ext.bytes().all(|b| b.is_ascii_alphanumeric());
        if !plain {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "the file extension must be plain",
            ));
        }
        create_private_dir(&self.directory)?;
        self.prune(now_ms);
        for attempt in 0..4 {
            let seed = format!("image|{now_ms}|{}|{attempt}", data.len());
            let name = unique_id(&seed).replace('-', "")[..8].to_ascii_lowercase();
            let path = self.directory.join(format!("phone-{name}.{ext}"));
            let mut file = match new_private_file(&path) {
                Ok(file) => file,
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error),
            };
            if let Err(error) = file.write_all(data) {
                let _ = fs::remove_file(&path);
                return Err(error);
            }
            return Ok(path);
        }
        Err(std::io::Error::from(std::io::ErrorKind::AlreadyExists))
    }

    fn prune(&self, now_ms: f64) {
        let Ok(entries) = fs::read_dir(&self.directory) else {
            return;
        };
        for entry in entries.flatten() {
            let modified = entry
                .metadata()
                .and_then(|m| m.modified())
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_millis() as f64);
            if modified.is_some_and(|at| now_ms - at > self.keep_for_ms) {
                let _ = fs::remove_file(entry.path());
            }
        }
    }

    pub fn message(caption: &str, path: &str) -> String {
        let quoted = if path.contains([' ', '\'', '"']) {
            format!("\"{path}\"")
        } else {
            path.to_string()
        };
        let trimmed = caption.trim();
        if trimmed.is_empty() {
            quoted
        } else {
            format!("{trimmed} {quoted}")
        }
    }
}

#[cfg(unix)]
fn create_private_dir(dir: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(dir)?;
    fs::set_permissions(dir, fs::Permissions::from_mode(0o700))
}

#[cfg(not(unix))]
fn create_private_dir(dir: &Path) -> std::io::Result<()> {
    fs::create_dir_all(dir)
}

#[cfg(unix)]
fn new_private_file(path: &Path) -> std::io::Result<fs::File> {
    use std::os::unix::fs::OpenOptionsExt;
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
}

#[cfg(not(unix))]
fn new_private_file(path: &Path) -> std::io::Result<fs::File> {
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
}
