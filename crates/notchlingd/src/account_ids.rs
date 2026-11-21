use hmac::{Hmac, Mac};
use notchling_engine::bridge::net::crypto::{hex, random_bytes};
use notchling_platform::files::write_private_no_follow;
use sha2::Sha256;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

pub const KEY_FILE: &str = "account-id.key";
const KEY_LEN: usize = 32;

pub struct AccountIds {
    path: PathBuf,
    key: OnceLock<Option<[u8; KEY_LEN]>>,
}

impl AccountIds {
    pub fn in_dir(dir: &Path) -> AccountIds {
        AccountIds {
            path: dir.join(KEY_FILE),
            key: OnceLock::new(),
        }
    }

    pub fn id(&self, provider: &str, raw: &str) -> Option<String> {
        let key = (*self.key.get_or_init(|| load_or_create(&self.path)))?;
        let mut mac = <Hmac<Sha256> as Mac>::new_from_slice(&key).ok()?;
        mac.update(provider.as_bytes());
        mac.update(&[0u8]);
        mac.update(raw.as_bytes());
        let tag = mac.finalize().into_bytes();
        Some(format!("acct-{}", hex(&tag[..8])))
    }
}

fn read_key(path: &Path) -> Option<[u8; KEY_LEN]> {
    if !fs::symlink_metadata(path).ok()?.file_type().is_file() {
        return None;
    }
    <[u8; KEY_LEN]>::try_from(fs::read(path).ok()?).ok()
}

fn load_or_create(path: &Path) -> Option<[u8; KEY_LEN]> {
    if let Some(key) = read_key(path) {
        return Some(key);
    }
    let key = <[u8; KEY_LEN]>::try_from(random_bytes(KEY_LEN).ok()?).ok()?;
    if let Some(dir) = path.parent() {
        let _ = fs::create_dir_all(dir);
    }
    let _ = write_private_no_follow(path, &key);
    Some(key)
}

#[cfg(test)]
#[path = "account_ids_tests.rs"]
mod tests;
