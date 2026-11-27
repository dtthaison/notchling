use super::manifest::{host_allowed, ModelFile};
use sha2::{Digest, Sha256};
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

pub const CANCELLED: &str = "cancelled";
pub const MAX_REDIRECTS: usize = 5;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Head {
    pub status: u16,
    pub location: Option<String>,
    pub length: Option<u64>,
    pub range_start: Option<u64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Progress {
    pub bytes: u64,
    pub total: u64,
}

pub trait Receiver {
    fn head(&mut self, head: &Head) -> Result<(), String>;
    fn data(&mut self, bytes: &[u8]) -> Result<(), String>;
}

pub trait Fetch: Send + Sync {
    fn get(&self, url: &str, from: u64, rx: &mut dyn Receiver) -> Result<Head, String>;
}

struct Sink<'a> {
    part: &'a Path,
    file: Option<File>,
    hasher: Sha256,
    written: u64,
    from: u64,
    size: u64,
    cancel: &'a AtomicBool,
    progress: &'a dyn Fn(Progress),
}

impl Sink<'_> {
    fn reset(&mut self) {
        self.hasher = Sha256::new();
        self.written = 0;
    }
}

impl Receiver for Sink<'_> {
    fn head(&mut self, head: &Head) -> Result<(), String> {
        let resume = match head.status {
            206 => {
                if head.range_start != Some(self.from) {
                    return Err("the server resumed from the wrong place".into());
                }
                true
            }
            200 => false,
            other => return Err(format!("the server answered {other}")),
        };
        if !resume && self.from > 0 {
            self.reset();
        }
        let file = notchling_platform::files::open_part(self.part, !resume).map_err(write_text)?;
        self.file = Some(file);
        Ok(())
    }

    fn data(&mut self, bytes: &[u8]) -> Result<(), String> {
        if self.cancel.load(Ordering::SeqCst) {
            return Err(CANCELLED.into());
        }
        if self.written + bytes.len() as u64 > self.size {
            return Err(TOO_MANY.into());
        }
        let file = self.file.as_mut().ok_or("no body expected")?;
        file.write_all(bytes).map_err(write_text)?;
        self.hasher.update(bytes);
        self.written += bytes.len() as u64;
        (self.progress)(Progress {
            bytes: self.written,
            total: self.size,
        });
        Ok(())
    }
}

const TOO_MANY: &str = "the file is larger than expected";
const EARLY: &str = "the connection closed early";
const MISMATCH: &str = "the file did not match its checksum";
const NO_SPACE: &str = "the file could not be written, check the free space";

fn write_text(e: impl std::fmt::Display) -> String {
    format!("{NO_SPACE} ({e})")
}

pub fn split_url(url: &str) -> Result<(&str, &str), String> {
    let rest = url.strip_prefix("https://").ok_or("https only")?;
    let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let host = &rest[..end];
    if host.is_empty() || host.contains(['@', ':', ' ', '\\']) {
        return Err("bad host".into());
    }
    Ok((host, &rest[end..]))
}

fn resolve(base: &str, location: &str) -> Result<String, String> {
    if location.starts_with("https://") {
        return Ok(location.to_string());
    }
    let (host, _) = split_url(base)?;
    if let Some(rest) = location.strip_prefix("//") {
        return Ok(format!("https://{rest}"));
    }
    if location.starts_with('/') {
        return Ok(format!("https://{host}{location}"));
    }
    Err("the server redirected to a place that is not allowed".into())
}

fn check_host(url: &str, hosts: &[String]) -> Result<(), String> {
    let (host, _) = split_url(url).map_err(|_| "a redirect left https".to_string())?;
    let host = host.to_ascii_lowercase();
    if hosts.iter().any(|h| host_allowed(h, &host)) {
        Ok(())
    } else {
        Err("the server redirected to a host that is not allowed".into())
    }
}

fn hash_existing(part: &Path, hasher: &mut Sha256) -> Result<u64, String> {
    let mut file = File::open(part).map_err(write_text)?;
    let mut buf = vec![0u8; 1 << 20];
    let mut total = 0u64;
    loop {
        let n = file.read(&mut buf).map_err(write_text)?;
        if n == 0 {
            return Ok(total);
        }
        hasher.update(&buf[..n]);
        total += n as u64;
    }
}

pub fn file_matches(path: &Path, file: &ModelFile) -> bool {
    let Ok(meta) = fs::symlink_metadata(path) else {
        return false;
    };
    if !meta.file_type().is_file() || meta.len() != file.size {
        return false;
    }
    let mut hasher = Sha256::new();
    matches!(hash_existing(path, &mut hasher), Ok(n) if n == file.size)
        && hex(&hasher.finalize()) == file.sha256
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn fetch_file(
    net: &dyn Fetch,
    hosts: &[String],
    file: &ModelFile,
    dir: &Path,
    cancel: &AtomicBool,
    progress: &dyn Fn(Progress),
) -> Result<PathBuf, String> {
    if !crate::delegate::media::safe_component(&file.save_as) {
        return Err("the file name is not allowed".into());
    }
    if fs::symlink_metadata(dir).is_ok_and(|m| m.file_type().is_symlink()) {
        return Err("refusing to write through a link".into());
    }
    notchling_platform::files::ensure_private_dir(dir).map_err(write_text)?;
    let part = dir.join(format!("{}.part", file.save_as));
    let target = dir.join(&file.save_as);
    let mut sink = Sink {
        part: &part,
        file: None,
        hasher: Sha256::new(),
        written: 0,
        from: 0,
        size: file.size,
        cancel,
        progress,
    };
    match fs::symlink_metadata(&part) {
        Ok(meta) if meta.file_type().is_symlink() => {
            return Err("refusing to write through a link".into())
        }
        Ok(meta) if meta.is_file() && meta.len() <= file.size => {
            sink.written = hash_existing(&part, &mut sink.hasher)?;
        }
        Ok(_) => {
            let _ = fs::remove_file(&part);
        }
        Err(_) => {}
    }
    let outcome = download(net, hosts, file, &mut sink, 0);
    match outcome {
        Ok(()) => {}
        Err(e) if e == CANCELLED => return Err(e),
        Err(e) if e.starts_with(NO_SPACE) || e == TOO_MANY => {
            drop(sink.file.take());
            let _ = fs::remove_file(&part);
            return Err(e);
        }
        Err(e) => return Err(e),
    }
    drop(sink.file.take());
    if sink.written < file.size {
        return Err(EARLY.into());
    }
    let digest = hex(&sink.hasher.finalize_reset());
    if sink.written != file.size || digest != file.sha256 {
        let _ = fs::remove_file(&part);
        return Err(MISMATCH.into());
    }
    fs::rename(&part, &target).map_err(write_text)?;
    Ok(target)
}

fn download(
    net: &dyn Fetch,
    hosts: &[String],
    file: &ModelFile,
    sink: &mut Sink,
    restarts: usize,
) -> Result<(), String> {
    if sink.written == file.size {
        return Ok(());
    }
    let mut url = file.url();
    for _ in 0..=MAX_REDIRECTS {
        check_host(&url, hosts)?;
        sink.from = sink.written;
        let head = net.get(&url, sink.written, sink)?;
        match head.status {
            200 | 206 => return Ok(()),
            301 | 302 | 303 | 307 | 308 => {
                let location = head.location.ok_or("a redirect without a place")?;
                url = resolve(&url, &location)?;
            }
            416 if restarts == 0 => {
                let _ = fs::remove_file(sink.part);
                sink.reset();
                return download(net, hosts, file, sink, 1);
            }
            other => return Err(format!("the server answered {other}")),
        }
    }
    Err("too many redirects".into())
}
