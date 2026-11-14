use super::IdentityCache;
use crate::sessions::testutil::TempDir;
use sha2::{Digest, Sha256};
use std::time::{Duration, SystemTime};

fn hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn set_mtime(path: &std::path::Path, secs: u64) {
    let file = std::fs::OpenOptions::new().write(true).open(path).unwrap();
    file.set_modified(SystemTime::UNIX_EPOCH + Duration::from_secs(secs))
        .unwrap();
}

#[test]
fn same_file_is_hashed_once() {
    let dir = TempDir::new("ident-once");
    let path = dir.write("bin/tool", "one");
    let mut cache = IdentityCache::new();
    let a = cache.identity(&path).unwrap();
    let b = cache.identity(&path).unwrap();
    assert_eq!(a, b);
    assert_eq!(a.sha256, hex(b"one"));
    assert_eq!(a.sha256.len(), 64);
    assert_eq!(cache.hashed(), 1);
}

#[test]
fn changed_file_is_hashed_again() {
    let dir = TempDir::new("ident-changed");
    let path = dir.write("bin/tool", "one");
    set_mtime(&path, 1_000_000);
    let mut cache = IdentityCache::new();
    let first = cache.identity(&path).unwrap();
    std::fs::write(&path, "two!").unwrap();
    set_mtime(&path, 2_000_000);
    let second = cache.identity(&path).unwrap();
    assert_ne!(first, second);
    assert_eq!(second.sha256, hex(b"two!"));
    assert_eq!(cache.hashed(), 2);
}

#[cfg(unix)]
#[test]
fn symlink_is_followed() {
    let dir = TempDir::new("ident-link");
    let target = dir.write("share/versions/2.1.3", "binary bytes");
    let link = dir.path("bin/tool");
    std::fs::create_dir_all(link.parent().unwrap()).unwrap();
    std::os::unix::fs::symlink(&target, &link).unwrap();
    let mut cache = IdentityCache::new();
    let id = cache.identity(&link).unwrap();
    assert_eq!(id.sha256, hex(b"binary bytes"));
    let direct = cache.identity(&target).unwrap();
    assert_eq!(id, direct);
    assert_eq!(cache.hashed(), 1);
}

#[test]
fn missing_file_is_none() {
    let dir = TempDir::new("ident-missing");
    let mut cache = IdentityCache::new();
    assert!(cache.identity(&dir.path("nothing")).is_none());
    assert!(cache.identity(dir.root()).is_none());
    assert_eq!(cache.hashed(), 0);
}

#[test]
fn large_file_streams() {
    let dir = TempDir::new("ident-large");
    let bytes = vec![7u8; 300_000];
    let path = dir.path("big");
    std::fs::write(&path, &bytes).unwrap();
    let mut cache = IdentityCache::new();
    assert_eq!(cache.identity(&path).unwrap().sha256, hex(&bytes));
}
