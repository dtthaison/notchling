use super::tests_support::*;
use super::{Kind, MigrateError, KINDS, PERSONAL_ID};
use crate::machines::registry::{backup_path_v2, RegistryState};
use serde_json::Value;

fn json(text: &str) -> Value {
    serde_json::from_str(text).unwrap()
}

fn entries(kind: Kind, text: &str) -> Vec<Value> {
    kind.entries(&json(text)).unwrap().clone()
}

#[test]
fn every_store_moves_to_personal_with_its_shadow_and_a_backup() {
    let rig = Rig::full("org-migrate");
    let report = rig.orgs.migrate(Some(&rig.registry)).unwrap();

    assert!(report.created);
    assert_eq!(report.stamped, 7);
    assert_eq!(report.restored, 0);
    assert_eq!(report.machines, 2);
    assert!(report.gaps.is_empty() && report.skipped.is_empty());
    assert_eq!(report.backups.len(), 4);

    for kind in KINDS {
        let text = rig.text(kind).unwrap();
        for entry in entries(kind, &text) {
            assert_eq!(entry["organization"], PERSONAL_ID, "{kind:?} {entry}");
        }
    }
    let claude = entries(Kind::Claude, &rig.text(Kind::Claude).unwrap());
    assert_eq!(claude[0]["email"], "a@example.com");
    assert_eq!(claude[1]["needsLogin"], true);
    assert_eq!(claude[1]["futureField"], 7, "fields we don't know stay");
    let codex = entries(Kind::Codex, &rig.text(Kind::Codex).unwrap());
    assert_eq!(codex[1]["plan"], Value::Null);
    let vault = entries(Kind::Vault, &rig.text(Kind::Vault).unwrap());
    assert_eq!(vault[1]["userVar"], "APTUS_DEPLOY_USER");

    let doc = rig.orgs.view();
    let shadow = &doc.document().shadow;
    assert_eq!(shadow.accounts.len(), 5);
    assert_eq!(shadow.accounts["claude:c1"], PERSONAL_ID);
    assert_eq!(shadow.accounts["codex:u2:w2"], PERSONAL_ID);
    assert_eq!(shadow.accounts["antigravity:g1"], PERSONAL_ID);
    assert_eq!(shadow.vault.len(), 2);
    assert_eq!(shadow.vault["aptus-deploy"], PERSONAL_ID);
    assert!(!doc.is_closed() && doc.only_personal());

    assert_eq!(rig.backup_text(Kind::Claude).as_deref(), Some(CLAUDE));
    assert_eq!(rig.backup_text(Kind::Codex).as_deref(), Some(CODEX));
    assert_eq!(
        rig.backup_text(Kind::Antigravity).as_deref(),
        Some(ANTIGRAVITY)
    );
    assert_eq!(rig.backup_text(Kind::Vault).as_deref(), Some(VAULT));
    assert_eq!(
        std::fs::read_to_string(backup_path_v2(&rig.registry.old_own)).unwrap(),
        REGISTRY_V2
    );

    let registry = rig.registry_text().unwrap();
    assert_eq!(json(&registry)["version"], 3);
    let RegistryState::Loaded { machines, .. } = rig.registry.load() else {
        panic!("not loaded");
    };
    assert_eq!(machines.len(), 2);
    assert!(machines
        .iter()
        .all(|m| m.organization.as_deref() == Some(PERSONAL_ID)));
}

#[cfg(unix)]
#[test]
fn the_backups_are_private_files() {
    use std::os::unix::fs::PermissionsExt;
    let rig = Rig::full("org-migrate-private");
    rig.orgs.migrate(Some(&rig.registry)).unwrap();
    let mode = |p: std::path::PathBuf| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
    assert_eq!(
        mode(super::backup::path(&rig.data, Kind::Vault.doc())),
        0o600
    );
    assert_eq!(mode(rig.data.join(super::backup::DIR)), 0o700);
}

#[test]
fn a_second_run_changes_nothing_and_a_backup_is_kept_once() {
    let rig = Rig::full("org-migrate-twice");
    rig.orgs.migrate(Some(&rig.registry)).unwrap();
    let after: Vec<Option<String>> = KINDS.iter().map(|k| rig.text(*k)).collect();
    let orgs = rig.orgs_text();
    let registry = rig.registry_text();

    let report = rig.orgs.migrate(Some(&rig.registry)).unwrap();
    assert!(!report.created);
    assert_eq!(
        (report.stamped, report.restored, report.machines),
        (0, 0, 0)
    );
    assert!(report.backups.is_empty());
    let again: Vec<Option<String>> = KINDS.iter().map(|k| rig.text(*k)).collect();
    assert_eq!(after, again);
    assert_eq!(orgs, rig.orgs_text());
    assert_eq!(registry, rig.registry_text());

    rig.put(
        Kind::Claude,
        r#"[{"id":"c9","email":"new@example.com","plan":"Pro","needsLogin":false}]"#,
    );
    let report = rig.orgs.migrate(Some(&rig.registry)).unwrap();
    assert_eq!(report.stamped, 1);
    assert!(
        report.backups.is_empty(),
        "the backup is the first text only"
    );
    assert_eq!(rig.backup_text(Kind::Claude).as_deref(), Some(CLAUDE));
}

#[test]
fn a_new_data_folder_gets_only_the_document() {
    let rig = Rig::new("org-migrate-fresh");
    let report = rig.orgs.migrate(Some(&rig.registry)).unwrap();
    assert!(report.created);
    assert_eq!(report.stamped, 0);
    assert!(rig.backups().is_empty());
    assert!(KINDS.iter().all(|k| rig.text(*k).is_none()));
    assert_eq!(rig.registry_text(), None);
    let doc = rig.orgs.view();
    assert_eq!(doc.organizations().len(), 1);
    assert_eq!(doc.organizations()[0].id, PERSONAL_ID);
}

#[test]
fn a_store_that_is_not_a_list_is_left_alone() {
    let rig = Rig::new("org-migrate-skip");
    rig.put(Kind::Vault, "not json at all");
    rig.put(Kind::Codex, r#"{"version":1,"nothing":true}"#);
    rig.put(Kind::Claude, CLAUDE);
    let report = rig.orgs.migrate(None).unwrap();
    assert_eq!(report.skipped.len(), 2);
    assert_eq!(rig.text(Kind::Vault).as_deref(), Some("not json at all"));
    assert_eq!(
        rig.text(Kind::Codex).as_deref(),
        Some(r#"{"version":1,"nothing":true}"#)
    );
    assert_eq!(report.stamped, 2);
}

#[test]
fn a_backup_that_cannot_be_made_stops_the_whole_migration() {
    let rig = Rig::full("org-migrate-nobackup");
    std::fs::create_dir_all(rig.data.join(super::backup::DIR)).unwrap();
    std::fs::create_dir_all(super::backup::path(&rig.data, Kind::Vault.doc())).unwrap();
    let before: Vec<Option<String>> = KINDS.iter().map(|k| rig.text(*k)).collect();
    let failed = rig.orgs.migrate(Some(&rig.registry));
    assert!(matches!(failed, Err(MigrateError::Store(_))), "{failed:?}");
    let after: Vec<Option<String>> = KINDS.iter().map(|k| rig.text(*k)).collect();
    assert_eq!(before, after, "no store changed");
    assert_eq!(rig.orgs_text(), None, "and no document was made");
}

#[test]
fn registry_v3_is_kept_and_an_old_v1_text_is_left_for_its_own_migration() {
    let rig = Rig::new("org-migrate-registry");
    let v3 = r#"{"version":3,"machines":[{"name":"a","host":"a","user":"u","organization":"personal"},{"name":"b","host":"b","user":"u"}]}"#;
    crate::store::put_doc(&rig.data, "machines/registry", v3).unwrap();
    assert_eq!(rig.registry.adopt_organizations(), Ok(1));
    assert!(!backup_path_v2(&rig.registry.old_own).exists());
    let machines = rig.registry.all();
    assert!(machines.iter().all(|m| m.organization.is_some()));
    assert_eq!(rig.registry.adopt_organizations(), Ok(0));
}
