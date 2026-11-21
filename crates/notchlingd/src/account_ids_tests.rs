use super::*;
use crate::limits_ops_tests::TempDir;
use sha2::{Digest, Sha256};

const EMAILS: [&str; 4] = [
    "email:erin@work-corp.com",
    "email:alice@example.com",
    "acc-work",
    "erin@work-corp.com",
];

fn old_public_salt_id(provider: &str, raw: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"notchling.accounts.summary.v1");
    hasher.update([0u8]);
    hasher.update(provider.as_bytes());
    hasher.update([0u8]);
    hasher.update(raw.as_bytes());
    let digest = hasher.finalize();
    format!("acct-{}", hex(&digest[..8]))
}

#[test]
fn the_same_key_gives_a_stable_id_in_the_old_format() {
    let tmp = TempDir::new("ids-stable");
    let first = AccountIds::in_dir(tmp.root());
    let id = first.id("claude-code", "acc-work").unwrap();
    assert_eq!(id.len(), "acct-".len() + 16);
    assert!(id.starts_with("acct-"));
    assert!(id["acct-".len()..].chars().all(|c| c.is_ascii_hexdigit()));
    assert_eq!(first.id("claude-code", "acc-work").unwrap(), id);
    let reopened = AccountIds::in_dir(tmp.root());
    assert_eq!(reopened.id("claude-code", "acc-work").unwrap(), id);
    assert_ne!(first.id("claude-code", "acc-other").unwrap(), id);
    assert_ne!(first.id("codex", "acc-work").unwrap(), id);
}

#[test]
fn two_keys_give_different_ids() {
    let (one, two) = (TempDir::new("ids-one"), TempDir::new("ids-two"));
    let (a, b) = (
        AccountIds::in_dir(one.root()),
        AccountIds::in_dir(two.root()),
    );
    for raw in EMAILS {
        assert_ne!(a.id("claude-code", raw), b.id("claude-code", raw), "{raw}");
    }
}

#[cfg(unix)]
#[test]
fn the_key_file_is_private_and_32_bytes() {
    use std::os::unix::fs::PermissionsExt;
    let tmp = TempDir::new("ids-mode");
    let ids = AccountIds::in_dir(&tmp.path("daemon"));
    assert!(ids.id("claude-code", "x").is_some());
    let file = tmp.path("daemon").join(KEY_FILE);
    let mode = fs::metadata(&file).unwrap().permissions().mode();
    assert_eq!(mode & 0o777, 0o600);
    assert_eq!(fs::read(&file).unwrap().len(), KEY_LEN);
    let leftovers: Vec<String> = fs::read_dir(tmp.path("daemon"))
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(leftovers, [KEY_FILE]);
}

#[test]
fn a_missing_or_wrong_length_key_is_made_again() {
    let tmp = TempDir::new("ids-regen");
    let file = tmp.path(KEY_FILE);
    for bad in [&b""[..], &b"short"[..], &[7u8; 33][..]] {
        fs::write(&file, bad).unwrap();
        let ids = AccountIds::in_dir(tmp.root());
        let id = ids.id("claude-code", "acc-work").unwrap();
        assert_eq!(fs::read(&file).unwrap().len(), KEY_LEN);
        assert_eq!(
            AccountIds::in_dir(tmp.root())
                .id("claude-code", "acc-work")
                .unwrap(),
            id
        );
    }
    fs::remove_file(&file).unwrap();
    assert!(AccountIds::in_dir(tmp.root())
        .id("claude-code", "acc-work")
        .is_some());
    assert_eq!(fs::read(&file).unwrap().len(), KEY_LEN);
}

#[test]
fn an_existing_key_is_kept_and_not_rewritten() {
    let tmp = TempDir::new("ids-keep");
    fs::write(tmp.path(KEY_FILE), [9u8; KEY_LEN]).unwrap();
    let id = AccountIds::in_dir(tmp.root())
        .id("claude-code", "acc-work")
        .unwrap();
    assert_eq!(fs::read(tmp.path(KEY_FILE)).unwrap(), vec![9u8; KEY_LEN]);
    let mut mac = <Hmac<Sha256> as Mac>::new_from_slice(&[9u8; KEY_LEN]).unwrap();
    mac.update(b"claude-code");
    mac.update(&[0u8]);
    mac.update(b"acc-work");
    let tag = mac.finalize().into_bytes();
    assert_eq!(id, format!("acct-{}", hex(&tag[..8])));
}

#[cfg(unix)]
#[test]
fn a_planted_link_is_never_followed() {
    let tmp = TempDir::new("ids-link");
    let target = tmp.path("target");
    fs::write(&target, [5u8; KEY_LEN]).unwrap();
    std::os::unix::fs::symlink(&target, tmp.path(KEY_FILE)).unwrap();
    let id = AccountIds::in_dir(tmp.root())
        .id("claude-code", "acc-work")
        .unwrap();
    assert_eq!(fs::read(&target).unwrap(), vec![5u8; KEY_LEN]);
    let mut mac = <Hmac<Sha256> as Mac>::new_from_slice(&[5u8; KEY_LEN]).unwrap();
    mac.update(b"claude-code");
    mac.update(&[0u8]);
    mac.update(b"acc-work");
    let tag = mac.finalize().into_bytes();
    assert_ne!(id, format!("acct-{}", hex(&tag[..8])));
}

#[test]
fn no_guess_of_the_email_reproduces_an_id_without_the_key() {
    let tmp = TempDir::new("ids-reverse");
    let ids = AccountIds::in_dir(tmp.root());
    for provider in ["claude-code", "codex", "antigravity"] {
        for raw in EMAILS {
            let id = ids.id(provider, raw).unwrap();
            assert_ne!(id, old_public_salt_id(provider, raw), "{provider} {raw}");
            let unkeyed = Sha256::digest(format!("{provider}\0{raw}"));
            assert_ne!(id, format!("acct-{}", hex(&unkeyed[..8])));
            let zero_key = <Hmac<Sha256> as Mac>::new_from_slice(&[0u8; KEY_LEN])
                .map(|mut mac| {
                    mac.update(provider.as_bytes());
                    mac.update(&[0u8]);
                    mac.update(raw.as_bytes());
                    mac.finalize().into_bytes()
                })
                .unwrap();
            assert_ne!(id, format!("acct-{}", hex(&zero_key[..8])));
        }
    }
}
