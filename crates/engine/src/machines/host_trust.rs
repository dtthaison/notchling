use super::addresses::Route;
use super::runner::{Call, Runner};
use super::ssh_test::{TEST_CONNECT_SECS, TEST_SECS};
use base64::engine::general_purpose::{STANDARD, STANDARD_NO_PAD};
use base64::Engine;
use sha2::{Digest, Sha256};
use std::io::Write;
use std::path::{Path, PathBuf};

pub const MAX_KEYS: usize = 8;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HostKey {
    pub field: String,
    pub kind: String,
    pub blob: String,
    pub fingerprint: String,
}

impl HostKey {
    pub fn line(&self) -> String {
        format!("{} {} {}", self.field, self.kind, self.blob)
    }

    pub fn label(&self) -> String {
        let kind = self
            .kind
            .strip_prefix("ssh-")
            .or_else(|| self.kind.strip_prefix("ecdsa-sha2-"))
            .unwrap_or(&self.kind);
        kind.to_ascii_uppercase()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ScanFailure {
    Jump,
    Nothing,
}

pub fn known_hosts_path(home: &Path) -> PathBuf {
    home.join(".ssh").join("known_hosts")
}

pub fn host_field(route: &Route) -> String {
    match route.port {
        Some(port) if port != 22 => format!("[{}]:{port}", route.host),
        _ => route.host.clone(),
    }
}

pub fn scan_argv(route: &Route) -> Vec<String> {
    let mut argv = vec![
        "/usr/bin/ssh-keyscan".to_owned(),
        "-T".to_owned(),
        TEST_CONNECT_SECS.to_string(),
    ];
    if let Some(port) = route.port {
        argv.push("-p".to_owned());
        argv.push(port.to_string());
    }
    argv.push("--".to_owned());
    argv.push(route.host.clone());
    argv
}

pub fn fingerprint(blob: &str) -> Option<String> {
    let bytes = STANDARD.decode(blob).ok()?;
    Some(format!(
        "SHA256:{}",
        STANDARD_NO_PAD.encode(Sha256::digest(&bytes))
    ))
}

fn blob_kind(blob: &str) -> Option<String> {
    let bytes = STANDARD.decode(blob).ok()?;
    let len = u32::from_be_bytes(bytes.get(..4)?.try_into().ok()?) as usize;
    let name = bytes.get(4..4usize.checked_add(len)?)?;
    String::from_utf8(name.to_vec()).ok()
}

pub fn parse_scan(out: &str, route: &Route) -> Vec<HostKey> {
    let field = host_field(route);
    let mut keys: Vec<HostKey> = Vec::new();
    for line in out.lines() {
        if line.starts_with('#') {
            continue;
        }
        let mut words = line.split_whitespace();
        let (Some(host), Some(kind), Some(blob), None) =
            (words.next(), words.next(), words.next(), words.next())
        else {
            continue;
        };
        if host != field && host != route.host {
            continue;
        }
        if blob_kind(blob).as_deref() != Some(kind) {
            continue;
        }
        let Some(fingerprint) = fingerprint(blob) else {
            continue;
        };
        if keys.iter().any(|k| k.blob == blob) || keys.len() >= MAX_KEYS {
            continue;
        }
        keys.push(HostKey {
            field: field.clone(),
            kind: kind.to_owned(),
            blob: blob.to_owned(),
            fingerprint,
        });
    }
    keys
}

pub fn scan(runner: &dyn Runner, route: &Route) -> Result<Vec<HostKey>, ScanFailure> {
    if route.jump.is_some() {
        return Err(ScanFailure::Jump);
    }
    let result = runner.run(&Call::secs(scan_argv(route), TEST_SECS));
    let keys = parse_scan(&result.out, route);
    if keys.is_empty() {
        Err(ScanFailure::Nothing)
    } else {
        Ok(keys)
    }
}

fn already(existing: &str, key: &HostKey) -> bool {
    existing.lines().any(|line| {
        let mut words = line.split_whitespace();
        words.next() == Some(key.field.as_str())
            && words.next() == Some(key.kind.as_str())
            && words.next() == Some(key.blob.as_str())
    })
}

pub fn trust(home: &Path, keys: &[HostKey]) -> Result<usize, String> {
    let path = known_hosts_path(home);
    let existing = std::fs::read_to_string(&path).unwrap_or_default();
    let fresh: Vec<&HostKey> = keys.iter().filter(|k| !already(&existing, k)).collect();
    if fresh.is_empty() {
        return Ok(0);
    }
    if let Some(dir) = path.parent() {
        create_private_dir(dir).map_err(|e| e.to_string())?;
    }
    let mut text = String::new();
    if !existing.is_empty() && !existing.ends_with('\n') {
        text.push('\n');
    }
    for key in &fresh {
        text.push_str(&key.line());
        text.push('\n');
    }
    let mut file = open_private(&path).map_err(|e| e.to_string())?;
    file.write_all(text.as_bytes()).map_err(|e| e.to_string())?;
    Ok(fresh.len())
}

#[cfg(unix)]
fn create_private_dir(dir: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::DirBuilderExt;
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(dir)
}

#[cfg(not(unix))]
fn create_private_dir(dir: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)
}

#[cfg(unix)]
fn open_private(path: &Path) -> std::io::Result<std::fs::File> {
    use std::os::unix::fs::OpenOptionsExt;
    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .mode(0o600)
        .open(path)
}

#[cfg(not(unix))]
fn open_private(path: &Path) -> std::io::Result<std::fs::File> {
    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
}
