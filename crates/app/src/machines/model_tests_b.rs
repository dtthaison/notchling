use std::sync::mpsc::channel;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use notchling_engine::machines::registry::{write_lock, AdminAccess, RegistryFiles};
use notchling_engine::vault::entry::{Kind, VaultEntry};
use notchling_engine::vault::list::VaultIndex;

use super::data::Setup;
use super::model::MachinesModel;
use super::model_tests::{lock, mac, off_world, scripted_world, with_job};

struct Scratch(std::path::PathBuf);

impl Scratch {
    fn new(tag: &str) -> Scratch {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let dir = std::env::temp_dir().join(format!(
            "notchling-machines-{tag}-{}-{nanos}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        Scratch(dir)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn entry(name: &str, updated: &str) -> VaultEntry {
    let mut e = VaultEntry::new(name, Kind::Token);
    e.updated = updated.into();
    e
}

fn save_list(index: &VaultIndex, entries: &[VaultEntry]) {
    index.write(entries).unwrap();
}

#[test]
fn a_value_saved_in_the_vault_offers_the_update_without_reading_any_value() {
    let root = Scratch::new("vault-watch");
    let setup = Setup::for_run(Some(&root.0), false, false).unwrap();
    assert!(setup.fixture && !setup.vault, "no Keychain, no process");
    assert!(RegistryFiles::single(&setup.data_dir).save(&[with_job(mac("m1"))]));
    let list = setup.vault_index();
    let first = [
        entry("claude-worker", "2026-11-01T00:00:00Z"),
        entry("unrelated", "2026-11-01T00:00:00Z"),
    ];
    save_list(&list, &first);
    let mut model = MachinesModel::new(setup.parts());
    assert!(
        !model.watch_vault(),
        "the list as the window opened is the baseline"
    );
    assert!(model.update_offers().is_empty());

    let renewed = [
        entry("claude-worker", "2026-11-09T08:00:00Z"),
        entry("unrelated", "2026-11-01T00:00:00Z"),
    ];
    save_list(&list, &renewed);
    assert!(model.watch_vault());
    let offers = model.update_offers();
    assert_eq!(offers.len(), 1);
    assert_eq!(
        (offers[0].secret.as_str(), offers[0].count),
        ("claude-worker", 1)
    );
    assert!(!model.watch_vault(), "nothing new since");

    let added = [
        entry("claude-worker", "2026-11-09T08:00:00Z"),
        entry("m1-github-issues", "2026-11-09T09:00:00Z"),
        entry("unrelated", "2026-11-01T00:00:00Z"),
    ];
    save_list(&list, &added);
    assert!(model.watch_vault());
    let names: Vec<String> = model
        .update_offers()
        .into_iter()
        .map(|o| o.secret)
        .collect();
    assert_eq!(names, ["claude-worker", "m1-github-issues"]);
    assert_eq!(model.vault_entries.len(), 3);
    let kept = notchling_engine::store::doc(&setup.data_dir, "machines/pending-updates")
        .unwrap()
        .expect("kept");
    assert!(kept.contains("m1-github-issues") && kept.contains("\"m1\""));
}

#[test]
fn a_rename_of_legacy_names_makes_no_update_offer() {
    let mut w = scripted_world(vec![with_job(mac("m1"))]);
    lock(&w.vault.entries).push(VaultEntry::new("sd-claude", Kind::Token));
    w.model.reload();
    let work = w.model.rename_work().expect("one to move");
    assert!(!w.model.watch_vault(), "nothing is watched while it moves");
    let failed = work.run();
    w.model.renamed(failed);
    assert!(!w.model.watch_vault());
    assert!(w.model.update_offers().is_empty());
}

#[test]
fn an_edit_waits_for_a_save_of_learned_facts_in_progress() {
    let mut w = scripted_world(vec![mac("m1")]);
    let released = Arc::new(Mutex::new(false));
    let (held_tx, held) = channel::<()>();
    let flag = released.clone();
    let saver = std::thread::spawn(move || {
        let _guard = write_lock();
        held_tx.send(()).unwrap();
        std::thread::sleep(Duration::from_millis(150));
        *lock(&flag) = true;
    });
    held.recv().unwrap();
    w.model.set_admin("m1", AdminAccess::RootSsh);
    assert!(
        *lock(&released),
        "the edit's read-modify-write ran after the other save, not across it"
    );
    saver.join().unwrap();
    assert_eq!(w.model.machine("m1").unwrap().admin(), AdminAccess::RootSsh);
    assert_eq!(*lock(&w.registry.saves), 1);
}

#[test]
fn renamed_legacy_secrets_show_a_banner_until_rename_moves_them() {
    let mut w = off_world(vec![mac("m1")]);
    lock(&w.vault.entries).push(VaultEntry::new("sd-claude", Kind::Token));
    lock(&w.vault.entries).push(VaultEntry::new("sd-worker-m1", Kind::Token));
    w.model.reload();
    assert_eq!(
        w.model.rename_banner().as_deref(),
        Some("Rename sd-worker-m1 and sd-claude to the new names")
    );
    let work = w.model.rename_work().expect("two to move");
    assert!(
        lock(&w.vault.renames).is_empty(),
        "nothing moves on the UI thread"
    );
    assert!(w.model.rename_work().is_none(), "one rename at a time");
    let failed = work.run();
    w.model.renamed(failed);
    assert_eq!(
        *lock(&w.vault.renames),
        [
            ("sd-worker-m1".to_string(), "m1-github-issues".to_string()),
            ("sd-claude".to_string(), "claude-worker".to_string())
        ]
    );
    assert_eq!(w.model.rename_banner(), None);
    assert_eq!(w.model.rename_note, None);
}

#[test]
fn renew_opens_the_secrets_page_or_says_why_not() {
    let mut w = off_world(vec![mac("m1")]);
    let mut entry = VaultEntry::new("claude-worker", Kind::Token);
    entry.url = Some("https://claude.ai".into());
    lock(&w.vault.entries).push(entry);
    lock(&w.vault.entries).push(VaultEntry::new("bare", Kind::Token));
    w.model.reload();
    let work = w.model.renew_work("claude-worker");
    assert!(
        lock(&w.vault.renewed).is_empty(),
        "nothing opens on the UI thread"
    );
    w.model.renewed(work.run());
    assert_eq!(w.model.notice, None);
    let work = w.model.renew_work("bare");
    w.model.renewed(work.run());
    assert_eq!(w.model.notice.as_deref(), Some("no page"));
    assert_eq!(*lock(&w.vault.renewed), ["claude-worker", "bare"]);
}

#[test]
fn each_poll_while_open_checks_the_machines_again_one_round_at_a_time() {
    let mut w = scripted_world(vec![mac("m1")]);
    assert!(w.model.check_after_poll(), "a poll ended: check again");
    assert!(
        !w.model.check_after_poll(),
        "not while the round it started still runs"
    );
    w.model.checked();
    assert!(w.model.check_after_poll());
    w.model.checked();
    let mut empty = scripted_world(Vec::new());
    assert!(
        !empty.model.check_after_poll(),
        "no machine, nothing to check"
    );
}
