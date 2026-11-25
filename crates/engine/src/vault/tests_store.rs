use super::entry::{Kind, VaultEntry, VaultValue};
use super::list::VaultIndex;
use super::store::{KeychainError, VaultError, VaultKeychain, VaultStore};
use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

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
    fn index(&self) -> VaultIndex {
        VaultIndex::in_data(&self.0)
    }
    fn old_file(&self) -> PathBuf {
        self.0.join("daemon/vault.json")
    }
    fn text(&self) -> Option<String> {
        crate::store::doc(&self.0, super::list::INDEX_DOC).unwrap()
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
fn the_old_list_file_is_read_then_moved_in_by_the_first_save() {
    let (temp, _item, store) = store("import");
    std::fs::create_dir_all(temp.old_file().parent().unwrap()).unwrap();
    let old = serde_json::to_string(&[VaultEntry::new("npm", Kind::Token)]).unwrap();
    std::fs::write(temp.old_file(), old).unwrap();
    assert_eq!(store.entries().unwrap()[0].name, "npm");
    assert!(temp.old_file().exists());
    assert!(!crate::store::exists(&temp.0), "reading made no database");
    store
        .save(
            &VaultEntry::new("github", Kind::Token),
            Some(value("fake-gh-0001")),
            None,
            "t",
        )
        .unwrap();
    assert!(!temp.old_file().exists(), "moved in");
    let names: Vec<String> = store
        .entries()
        .unwrap()
        .into_iter()
        .map(|e| e.name)
        .collect();
    assert_eq!(names, ["github", "npm"]);
    assert!(temp.text().is_some());
}

#[test]
fn two_stores_saving_at_once_both_keep_their_entries() {
    let temp = Temp::new("two-writers");
    std::thread::scope(|scope| {
        for t in 0..4 {
            let index = temp.index();
            scope.spawn(move || {
                let store = VaultStore::new(index, Box::new(FakeItem::default()));
                for i in 0..5 {
                    let entry = VaultEntry::new(&format!("s{t}-{i}"), Kind::Token);
                    store
                        .save(&entry, Some(value("fake-0001")), None, "t")
                        .unwrap();
                }
            });
        }
    });
    assert_eq!(temp.index().read().unwrap().len(), 20);
}

#[test]
fn the_stamp_follows_the_list_and_nothing_else() {
    let temp = Temp::new("stamp");
    let index = temp.index();
    assert_eq!(index.stamp(), None);
    let mut a = VaultEntry::new("a", Kind::Token);
    a.updated = "2026-11-01T00:00:00Z".into();
    index.write(std::slice::from_ref(&a)).unwrap();
    let first = index.stamp().unwrap();
    crate::store::put_doc(&temp.0, "elsewhere", "1").unwrap();
    assert_eq!(
        index.stamp(),
        Some(first),
        "another document doesn't change it"
    );
    a.updated = "2026-11-09T00:00:00Z".into();
    index.write(&[a]).unwrap();
    assert_ne!(index.stamp(), Some(first), "a same-length change does");
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
    let text = temp.text().unwrap();
    assert!(!text.contains("fake-gh-token-0001"), "no value in the list");
    assert!(text.contains("\"secretVar\": \"GITHUB_TOKEN\""), "{text}");
    assert!(!temp.old_file().exists(), "no file is written any more");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(crate::store::db_path(&temp.0))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
    }
    assert_eq!(item.values()["github"]["secret"], "fake-gh-token-0001");
    assert_eq!(
        store.value(&entry).unwrap(),
        Some(value("fake-gh-token-0001"))
    );
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
    assert!(
        !crate::store::exists(&temp.0),
        "a refused save makes no database"
    );
    assert!(item.data.lock().unwrap().is_none());
}

#[test]
fn a_list_that_cant_be_read_is_left_alone() {
    let (temp, _item, store) = store("broken");
    std::fs::create_dir_all(temp.old_file().parent().unwrap()).unwrap();
    std::fs::write(temp.old_file(), b"{not a list").unwrap();
    let e = store
        .save(
            &VaultEntry::new("a", Kind::Token),
            Some(value("fake-0001")),
            None,
            "t",
        )
        .unwrap_err();
    assert!(e.to_string().contains("can't be read"), "{e}");
    assert_eq!(std::fs::read(temp.old_file()).unwrap(), b"{not a list");
    assert_eq!(temp.text(), None, "nothing is written beside it");
    assert!(store.entries().is_err());
    assert!(store.remove("a").is_err());
    assert!(store.set_expires("a", None).is_err());
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
    let db = crate::store::open(&temp.0).unwrap();
    db.execute_batch(
        "CREATE TRIGGER refuse_vault BEFORE UPDATE ON docs
         BEGIN SELECT RAISE(ABORT, 'refused'); END;",
    )
    .unwrap();
    let failed = store.rename(&old, "claude-worker", "t");
    assert!(failed.is_err());
    let keys: Vec<String> = item.values().as_object().unwrap().keys().cloned().collect();
    assert_eq!(keys, ["claude-worker"]);
    assert_eq!(store.entries().unwrap()[0].name, "sd-claude");
    db.execute_batch("DROP TRIGGER refuse_vault").unwrap();
    drop(db);

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
    temp.index().write(std::slice::from_ref(&old)).unwrap();
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

#[test]
fn a_save_or_rename_made_from_a_stale_entry_keeps_the_organization_set_since() {
    use crate::organizations::{Kind as OrgKind, Organizations};
    let (temp, _item, store) = store("organization");
    let entry = VaultEntry::new("npm", Kind::Token);
    store
        .save(&entry, Some(value("fake-npm-0001")), None, "t")
        .unwrap();
    let orgs = Organizations::new(&temp.0);
    orgs.create("Acme", None, None).unwrap();
    orgs.set_organization(OrgKind::Vault, "npm", "acme")
        .unwrap();
    let stale = store.entry("npm").unwrap().unwrap();
    assert_eq!(stale.organization.as_deref(), Some("acme"));
    orgs.create("Other", None, None).unwrap();
    orgs.set_organization(OrgKind::Vault, "npm", "other")
        .unwrap();
    let saved = store.save(&stale, None, Some(&stale), "t2").unwrap();
    assert_eq!(saved.entry.organization.as_deref(), Some("other"));
    assert_eq!(
        store.entry("npm").unwrap().unwrap().organization.as_deref(),
        Some("other")
    );
    let renamed = store.rename(&stale, "npm-two", "t3").unwrap();
    assert_eq!(renamed.entry.organization.as_deref(), Some("other"));
    let shadow = match orgs.state() {
        crate::organizations::State::Loaded(doc) => doc.shadow.vault,
        other => panic!("{other:?}"),
    };
    assert_eq!(shadow.get("npm-two").map(String::as_str), Some("other"));
    assert!(!shadow.contains_key("npm"));
}

#[test]
fn a_rename_moves_the_shadow_in_the_same_write_and_onto_an_existing_name_changes_nothing() {
    use crate::organizations::{Kind as OrgKind, Organizations, State};
    let (temp, _item, store) = store("rename-shadow");
    for name in ["alpha", "beta"] {
        store
            .save(
                &VaultEntry::new(name, Kind::Token),
                Some(value("fake-0001")),
                None,
                "t",
            )
            .unwrap();
    }
    let orgs = Organizations::new(&temp.0);
    orgs.create("Acme", None, None).unwrap();
    orgs.set_organization(OrgKind::Vault, "alpha", "acme")
        .unwrap();
    let before = temp.text();
    let org_before = crate::store::doc(&temp.0, crate::organizations::DOC).unwrap();
    let alpha = store.entry("alpha").unwrap().unwrap();
    assert!(store.rename(&alpha, "beta", "t2").is_err());
    assert_eq!(temp.text(), before);
    assert_eq!(
        crate::store::doc(&temp.0, crate::organizations::DOC).unwrap(),
        org_before
    );
    store.rename(&alpha, "gamma", "t3").unwrap();
    let State::Loaded(doc) = orgs.state() else {
        panic!("loaded");
    };
    assert_eq!(
        doc.shadow.vault.get("gamma").map(String::as_str),
        Some("acme")
    );
    assert!(!doc.shadow.vault.contains_key("alpha"));
}

#[test]
fn removing_a_secret_forgets_its_shadow() {
    use crate::organizations::{Kind as OrgKind, Organizations, State};
    let (temp, _item, store) = store("remove-shadow");
    store
        .save(
            &VaultEntry::new("alpha", Kind::Token),
            Some(value("fake-0001")),
            None,
            "t",
        )
        .unwrap();
    let orgs = Organizations::new(&temp.0);
    orgs.create("Acme", None, None).unwrap();
    orgs.set_organization(OrgKind::Vault, "alpha", "acme")
        .unwrap();
    assert!(store.remove("alpha").unwrap());
    let State::Loaded(doc) = orgs.state() else {
        panic!("loaded");
    };
    assert!(!doc.shadow.vault.contains_key("alpha"));
    store
        .save(
            &VaultEntry::new("alpha", Kind::Token),
            Some(value("fake-0002")),
            None,
            "t",
        )
        .unwrap();
    assert_eq!(orgs.resolve(OrgKind::Vault, "alpha").id, "personal");
}

#[test]
fn a_save_finds_the_secret_it_replaces_under_the_store_lock() {
    let (_temp, _item, store) = store("replacing");
    let entry = VaultEntry::new("gh", Kind::Token);
    store
        .save(&entry, Some(value("fake-gh-token-0001")), None, "t")
        .unwrap();
    let missing = store.save_replacing(&entry, None, Some("nope"), "t2", &|_, _| true);
    assert!(matches!(missing, Err(VaultError::NoSuchSecret(_))));
    let refused = store.save_replacing(&entry, None, Some("gh"), "t2", &|_, _| false);
    assert!(matches!(refused, Err(VaultError::Changed)));
    let (saved, old) = store
        .save_replacing(&entry, None, Some("GH"), "t3", &|_, _| true)
        .unwrap();
    assert_eq!(old.unwrap().name, "gh");
    assert_eq!(saved.entry.updated, "t3");
}

#[test]
fn an_edit_keeping_its_name_replaces_itself_and_a_new_secret_cannot_take_the_name() {
    let (_temp, item, store) = store("same-name");
    let entry = VaultEntry::new("gh", Kind::Token);
    store
        .save(&entry, Some(value("fake-gh-token-0001")), None, "t")
        .unwrap();
    let mut edited = entry.clone();
    edited.note = "edited".into();
    let taken = store
        .save_replacing(
            &edited,
            Some(value("fake-gh-token-0002")),
            None,
            "t2",
            &|_, _| true,
        )
        .unwrap_err();
    assert_eq!(taken.to_string(), "There's already a secret named gh.");
    assert_eq!(item.values()["gh"]["secret"], "fake-gh-token-0001");
    let (saved, old) = store
        .save_replacing(&edited, None, Some("gh"), "t3", &|_, _| true)
        .unwrap();
    assert_eq!(old.unwrap().name, "gh");
    assert!(!saved.value_changed);
    assert_eq!(saved.entry.note, "edited");
    assert_eq!(store.entries().unwrap().len(), 1);
    assert_eq!(store.entry("gh").unwrap().unwrap().note, "edited");
    assert_eq!(item.values()["gh"]["secret"], "fake-gh-token-0001");
    let (changed, _) = store
        .save_replacing(
            &edited,
            Some(value("fake-gh-token-0003")),
            Some("gh"),
            "t4",
            &|_, _| true,
        )
        .unwrap();
    assert!(changed.value_changed);
    assert_eq!(item.values()["gh"]["secret"], "fake-gh-token-0003");
}

#[test]
fn every_save_changes_the_revision_whatever_the_clock_says() {
    let (_temp, _item, store) = store("revision");
    let entry = VaultEntry::new("gh", Kind::Token);
    let first = store
        .save(&entry, Some(value("fake-gh-0001")), None, "t")
        .unwrap()
        .entry;
    let second = store
        .save(&first, Some(value("fake-gh-0002")), Some(&first), "t")
        .unwrap()
        .entry;
    assert_eq!(first.updated, second.updated);
    assert_eq!((first.rev, second.rev), (1, 2));
    assert_ne!(first.revision(), second.revision());
    let held = first.revision();
    let stale = store.save_replacing(&second, None, Some("gh"), "t", &|_, old| {
        old.is_some_and(|e| e.revision() == held)
    });
    assert!(matches!(stale, Err(VaultError::Changed)));
    let current = second.revision();
    let (third, _) = store
        .save_replacing(&second, None, Some("gh"), "t", &|_, old| {
            old.is_some_and(|e| e.revision() == current)
        })
        .unwrap();
    assert_eq!(third.entry.rev, 3);
}

#[test]
fn a_renamed_save_sees_the_entry_at_the_new_name_and_the_one_it_replaces() {
    let (_temp, _item, store) = store("replacing-rename");
    let gh = VaultEntry::new("gh", Kind::Token);
    store
        .save(&gh, Some(value("fake-gh-0001")), None, "t")
        .unwrap();
    let npm = VaultEntry::new("npm", Kind::Token);
    store
        .save(&npm, Some(value("fake-npm-0001")), None, "t")
        .unwrap();
    let renamed = gh.renamed("github");
    let seen = std::cell::RefCell::new(Vec::new());
    let (saved, old) = store
        .save_replacing(&renamed, None, Some("gh"), "t2", &|new, old| {
            seen.borrow_mut()
                .push((new.map(|e| e.name.clone()), old.map(|e| e.name.clone())));
            true
        })
        .unwrap();
    assert_eq!(seen.into_inner(), [(None, Some("gh".to_string()))]);
    assert_eq!(
        (saved.entry.name.as_str(), old.unwrap().name.as_str()),
        ("github", "gh")
    );
    let onto = npm.renamed("github");
    let clash = store.save_replacing(&onto, None, Some("npm"), "t3", &|_, _| true);
    assert!(matches!(clash, Err(VaultError::Problem(_))));
}

#[test]
fn the_counter_comes_from_the_stored_entry_and_survives_a_rename() {
    let (_temp, _item, store) = store("revision-rename");
    let mut entry = VaultEntry::new("gh", Kind::Token);
    entry.rev = 40;
    let first = store
        .save(&entry, Some(value("fake-gh-0001")), None, "t")
        .unwrap()
        .entry;
    assert_eq!(first.rev, 1, "a caller's own counter is ignored");
    let renamed = store.rename(&first, "gh2", "t").unwrap().entry;
    assert_eq!((renamed.name.as_str(), renamed.rev), ("gh2", 2));
    assert_eq!(store.entry("gh2").unwrap().unwrap().rev, 2);
}

#[test]
fn an_entry_written_before_the_counter_loads_and_saves() {
    let (temp, _item, store) = store("revision-legacy");
    std::fs::create_dir_all(temp.old_file().parent().unwrap()).unwrap();
    let old = br#"[{"name":"gh","kind":"token","note":"n","userVar":"","secretVar":"GH_TOKEN","updated":"2026-11-05T00:00:00Z"}]"#;
    std::fs::write(temp.old_file(), old).unwrap();
    let loaded = store.entry("gh").unwrap().unwrap();
    assert_eq!(loaded.rev, 0);
    assert_eq!(loaded.revision(), "2026-11-05T00:00:00Z");
    assert!(serde_json::to_value(&loaded).unwrap().get("rev").is_none());
    let before = loaded.revision();
    let saved = store
        .save(
            &loaded,
            Some(value("fake-gh-0001")),
            Some(&loaded),
            "2026-11-05T00:00:00Z",
        )
        .unwrap()
        .entry;
    assert_eq!(saved.updated, loaded.updated);
    assert_eq!(saved.rev, 1);
    assert_ne!(saved.revision(), before);
    let text = temp.text().unwrap();
    assert!(text.contains("\"rev\": 1"), "{text}");
}
