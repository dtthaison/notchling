use super::{
    account_for, Checkpoint, FileKeyStore, KeyRead, KeyStore, MemoryKeyStore, UserDeleted,
    HEAD_FILE, KEY_FILE,
};
use crate::sessions::testutil::TempDir;
use std::time::{Duration, Instant};

fn checkpoint(n: u64) -> Checkpoint {
    Checkpoint {
        n,
        seal: "s".repeat(44),
        month: 202611,
        settings_hash: Some("abcd1234".into()),
        prune_anchor: None,
        user_deleted: UserDeleted::default(),
        at: 1_794_560_523_117,
    }
}

#[test]
fn memory_store_round_trip() {
    let store = MemoryKeyStore::new();
    assert_eq!(store.read_key(), KeyRead::Missing);
    assert!(!store.head_exists());
    let key = store.create_key().expect("creates");
    assert_eq!(store.read_key(), KeyRead::Found(key));
    store.write_head(&checkpoint(7)).expect("writes");
    assert!(store.head_exists());
    assert_eq!(store.read_head(), Some(checkpoint(7)));
    assert_eq!(
        MemoryKeyStore::with_key([5; 32]).read_key(),
        KeyRead::Found([5; 32])
    );
}

#[test]
fn memory_store_denies_when_told() {
    let store = MemoryKeyStore::with_key([5; 32]);
    store.deny_reads(true);
    assert_eq!(store.read_key(), KeyRead::Denied);
    store.deny_reads(false);
    assert_eq!(store.read_key(), KeyRead::Found([5; 32]));
}

#[test]
fn memory_store_can_stand_in_for_a_prompt() {
    let store = MemoryKeyStore::with_key([5; 32]);
    store.block_reads(Duration::from_millis(60));
    let start = Instant::now();
    assert_eq!(store.read_key(), KeyRead::Found([5; 32]));
    assert!(start.elapsed() >= Duration::from_millis(60));
}

#[test]
fn file_store_round_trip_0600() {
    let dir = TempDir::new("keys-file");
    let store = FileKeyStore::new(dir.path("daemon"));
    let key = store.create_key().expect("creates");
    assert_eq!(store.read_key(), KeyRead::Found(key));
    store.write_head(&checkpoint(3)).expect("writes");
    store.write_head(&checkpoint(4)).expect("overwrites");
    assert_eq!(store.read_head(), Some(checkpoint(4)));
    assert!(store.head_exists());
    assert!(store.create_key().is_err());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        for name in [KEY_FILE, HEAD_FILE] {
            let mode = std::fs::metadata(dir.path(&format!("daemon/{name}")))
                .expect("meta")
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o600, "{name}");
        }
    }
}

#[test]
fn file_store_missing_is_missing() {
    let dir = TempDir::new("keys-missing");
    let store = FileKeyStore::new(dir.path("daemon"));
    assert_eq!(store.read_key(), KeyRead::Missing);
    assert_eq!(store.read_head(), None);
    assert!(!store.head_exists());
}

#[test]
fn file_store_with_a_short_key_is_denied_not_missing() {
    let dir = TempDir::new("keys-short");
    dir.write("daemon/audit-key", "short");
    let store = FileKeyStore::new(dir.path("daemon"));
    assert_eq!(store.read_key(), KeyRead::Denied);
}

#[cfg(unix)]
#[test]
fn file_store_refuses_symlink() {
    let dir = TempDir::new("keys-link");
    let target = dir.write("elsewhere", &"k".repeat(32));
    std::fs::create_dir_all(dir.path("daemon")).expect("mkdir");
    std::os::unix::fs::symlink(&target, dir.path("daemon/audit-key")).expect("link");
    let store = FileKeyStore::new(dir.path("daemon"));
    assert_eq!(store.read_key(), KeyRead::Denied);
    assert!(store.create_key().is_err());
    assert_eq!(
        std::fs::read_to_string(&target).expect("read"),
        "k".repeat(32)
    );
}

#[test]
fn account_is_16_hex_and_differs_per_folder() {
    let a = TempDir::new("keys-account-a");
    let b = TempDir::new("keys-account-b");
    let first = account_for(a.root());
    assert_eq!(first.len(), 16);
    assert!(first
        .bytes()
        .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c)));
    assert_ne!(first, account_for(b.root()));
    assert_eq!(first, account_for(a.root()));
}

#[test]
fn user_deleted_keeps_newest_16_and_counts_all() {
    let mut deleted = UserDeleted::default();
    for month in 0..20u32 {
        deleted.push(202501 + month, i64::from(month));
    }
    assert_eq!(deleted.months.len(), 16);
    assert_eq!(deleted.at.len(), 16);
    assert_eq!(deleted.count, 20);
    assert_eq!(deleted.months[0], 202505);
    assert_eq!(deleted.months[15], 202520);
}
