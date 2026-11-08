//! The store against a fake Keychain item and a temp `vault.json` (Swift `VaultStoreRenameTests`,
//! plus saves, removes, Always Allow and the errors, which never hold a value).

use super::entry::{Kind, VaultEntry, VaultValue};
use super::store::{KeychainError, VaultError, VaultKeychain, VaultStore};
use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

/// The item in memory; `fail_writes` makes the next writes fail.
#[derive(Clone, Default)]
pub(crate) struct FakeItem {
    pub data: Arc<Mutex<Option<Vec<u8>>>>,
    pub reads: Arc<Mutex<u32>>,
    pub fail_reads: Arc<Mutex<Option<KeychainError>>>,
}

impl VaultKeychain for FakeItem {
    fn read(&self) -> Result<Option<Vec<u8>>, KeychainError> {
        *self.reads.lock().unwrap() += 1;
        if let Some(e) = self.fail_reads.lock().unwrap().clone() {
            return Err(e);
        }
        Ok(self.data.lock().unwrap().clone())
    }
    fn write(&self, data: &[u8]) -> Result<(), KeychainError> {
        *self.data.lock().unwrap() = Some(data.to_vec());
        Ok(())
    }
}

impl FakeItem {
    fn values(&self) -> serde_json::Value {
        let data = self.data.lock().unwrap().clone().unwrap_or(b"{}".to_vec());
        serde_json::from_slice(&data).unwrap()
    }
}

struct Temp(PathBuf);

impl Temp {
    fn new(tag: &str) -> Temp {
        let dir = std::env::temp_dir().join(format!("nl-vault-store-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Temp(dir)
    }
    fn index(&self) -> PathBuf {
        self.0.join("daemon/vault.json")
    }
}

impl Drop for Temp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn value(secret: &str) -> VaultValue {
    VaultValue {
        username: String::new(),
        secret: secret.into(),
    }
}

fn store(tag: &str) -> (Temp, FakeItem, VaultStore) {
    let temp = Temp::new(tag);
    let item = FakeItem::default();
    let store = VaultStore::new(temp.index(), Box::new(item.clone()));
    (temp, item, store)
}

#[test]
fn a_save_writes_the_list_0600_and_the_value_to_the_item_only() {
    let (temp, item, store) = store("save");
    let entry = VaultEntry::new("github", Kind::Token);
    let saved = store
        .save(
            &entry,
            Some(value("fake-gh-token-0001")),
            None,
            "2026-11-08T00:00:00Z",
        )
        .unwrap();
    assert!(saved.value_changed);
    assert_eq!(saved.entry.updated, "2026-11-08T00:00:00Z");
    let text = std::fs::read_to_string(temp.index()).unwrap();
    assert!(
        !text.contains("fake-gh-token-0001"),
        "no value in vault.json"
    );
    assert!(text.contains("\"secretVar\": \"GITHUB_TOKEN\""), "{text}");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(temp.index())
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
        let dir = std::fs::metadata(temp.index().parent().unwrap()).unwrap();
        assert_eq!(dir.permissions().mode() & 0o777, 0o700);
    }
    assert_eq!(item.values()["github"]["secret"], "fake-gh-token-0001");
    assert_eq!(
        store.value(&entry).unwrap(),
        Some(value("fake-gh-token-0001"))
    );
    // The same value again is no change; another is.
    let again = store
        .save(&entry, Some(value("fake-gh-token-0001")), Some(&entry), "t")
        .unwrap();
    assert!(!again.value_changed);
    let other = store
        .save(&entry, Some(value("fake-gh-token-0002")), Some(&entry), "t")
        .unwrap();
    assert!(other.value_changed);
}

#[test]
fn values_are_read_once_and_forgotten_on_request() {
    let (_temp, item, store) = store("cache");
    let entry = VaultEntry::new("npm", Kind::Token);
    store
        .save(&entry, Some(value("fake-npm-0001")), None, "t")
        .unwrap();
    let reads = *item.reads.lock().unwrap();
    store.value(&entry).unwrap();
    store.value(&entry).unwrap();
    assert_eq!(*item.reads.lock().unwrap(), reads, "kept in memory");
    store.forget();
    store.value(&entry).unwrap();
    assert_eq!(*item.reads.lock().unwrap(), reads + 1);
}

#[test]
fn an_empty_value_is_refused_and_nothing_is_written() {
    let (temp, item, store) = store("empty");
    let login = VaultEntry::new("reg", Kind::Login);
    let e = store.save(&login, None, None, "t").unwrap_err();
    assert_eq!(e.to_string(), "Enter the password.");
    let e = store
        .save(
            &VaultEntry::new("t", Kind::Token),
            Some(value("")),
            None,
            "t",
        )
        .unwrap_err();
    assert_eq!(e.to_string(), "Enter the token.");
    assert!(!temp.index().exists());
    assert!(item.data.lock().unwrap().is_none());
}

#[test]
fn a_list_that_cant_be_read_is_left_alone() {
    let (temp, _item, store) = store("broken");
    std::fs::create_dir_all(temp.index().parent().unwrap()).unwrap();
    std::fs::write(temp.index(), b"{not a list").unwrap();
    let e = store
        .save(
            &VaultEntry::new("a", Kind::Token),
            Some(value("fake-0001")),
            None,
            "t",
        )
        .unwrap_err();
    assert!(e.to_string().contains("can't be read"), "{e}");
    assert_eq!(std::fs::read(temp.index()).unwrap(), b"{not a list");
    assert!(store.entries().is_err());
}

#[test]
fn errors_never_quote_the_item() {
    let (_temp, item, store) = store("quote");
    *item.data.lock().unwrap() = Some(br#"{"a": "fake-leaky-value-0001"}"#.to_vec());
    let e = store.value(&VaultEntry::new("a", Kind::Token)).unwrap_err();
    assert_eq!(e, VaultError::ItemUnreadable);
    assert!(!format!("{e} {e:?}").contains("fake-leaky-value-0001"));
    *item.fail_reads.lock().unwrap() = Some(KeychainError::Denied);
    store.forget();
    let e = store.value(&VaultEntry::new("a", Kind::Token)).unwrap_err();
    assert!(e.to_string().starts_with("Keychain access was denied"));
}

#[test]
fn a_rename_that_failed_at_the_list_finishes_on_the_second_try() {
    let (temp, item, store) = store("rename");
    let old = VaultEntry::new("sd-claude", Kind::Token);
    store
        .save(&old, Some(value("the-claude-token-value")), None, "t")
        .unwrap();
    // The list's folder can't be written: the item is, the list isn't. (Root writes anyway.)
    #[cfg(unix)]
    if unsafe { libc::geteuid() } != 0 {
        use std::os::unix::fs::PermissionsExt;
        let folder = temp.index().parent().unwrap().to_path_buf();
        let mode = |m| std::fs::set_permissions(&folder, std::fs::Permissions::from_mode(m));
        mode(0o500).unwrap();
        let failed = store.rename(&old, "claude-worker", "t");
        mode(0o700).unwrap();
        assert!(failed.is_err());
        let keys: Vec<String> = item.values().as_object().unwrap().keys().cloned().collect();
        assert_eq!(keys, ["claude-worker"]);
        assert_eq!(store.entries().unwrap()[0].name, "sd-claude");
    }

    let renamed = store.rename(&old, "claude-worker", "t").unwrap();
    assert!(
        !renamed.value_changed,
        "the same value under a new name is not a change"
    );
    let list = store.entries().unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].name, "claude-worker");
    assert_eq!(list[0].secret_var, "CLAUDE_WORKER_TOKEN");
    assert_eq!(
        item.values(),
        serde_json::json!({"claude-worker": {"username": "", "secret": "the-claude-token-value"}}),
        "the value is kept, once"
    );
}

#[test]
fn a_rename_with_no_value_anywhere_refuses() {
    let (temp, _item, store) = store("norename");
    let old = VaultEntry::new("sd-claude", Kind::Token);
    super::list::write_index(&temp.index(), std::slice::from_ref(&old)).unwrap();
    assert!(store.rename(&old, "claude-worker", "t").is_err());
    assert_eq!(store.entries().unwrap()[0].name, "sd-claude");
}

#[test]
fn remove_stop_asking_and_expiry() {
    let (_temp, item, store) = store("remove");
    let gh = VaultEntry {
        ask: Some(true),
        ..VaultEntry::new("gh", Kind::Token)
    };
    store
        .save(&gh, Some(value("fake-gh-0001")), None, "t")
        .unwrap();
    store
        .stop_asking(&BTreeSet::from(["gh".to_string()]))
        .unwrap();
    assert_eq!(store.entry("GH").unwrap().unwrap().ask, Some(false));
    assert!(store.set_expires("gh", Some("2026-12-06")).unwrap());
    assert!(!store.set_expires("gh", Some("2026-12-06")).unwrap());
    assert!(store.set_expires("gh", Some("2026-02-30")).is_err());
    assert!(matches!(
        store.set_expires("nope", None),
        Err(VaultError::NoSuchSecret(_))
    ));
    assert_eq!(
        store.entry("gh").unwrap().unwrap().expires.as_deref(),
        Some("2026-12-06")
    );
    assert!(store.remove("GH").unwrap());
    assert!(!store.remove("gh").unwrap());
    assert!(store.entries().unwrap().is_empty());
    assert_eq!(item.values(), serde_json::json!({}));
}

#[test]
fn remove_never_touches_a_value_the_daemons_list_doesnt_name() {
    let (_temp, item, store) = store("remove-unlisted");
    // A value the Swift app saved in the same item, under a name only its own list has.
    *item.data.lock().unwrap() =
        Some(br#"{"swift-only":{"username":"","secret":"fake-swift-0001"}}"#.to_vec());
    let theirs = serde_json::json!({"swift-only": {"username": "", "secret": "fake-swift-0001"}});
    assert!(!store.remove("swift-only").unwrap(), "not the daemon's");
    assert_eq!(item.values(), theirs);
    let gh = VaultEntry::new("gh", Kind::Token);
    store
        .save(&gh, Some(value("fake-gh-0003")), None, "t")
        .unwrap();
    assert!(store.remove("gh").unwrap());
    assert_eq!(item.values(), theirs);
}
