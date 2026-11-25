use super::tests_support::*;
use super::{Kind, Source, KINDS, PERSONAL_ID};
use crate::accounts::book::{read_accounts, Saved};
use crate::accounts::claude::ClaudeAccount;
use crate::docfile::DocFile;
use crate::machines::registry::{Machine, RegistryState};
use crate::vault::entry::{Kind as VaultKind, VaultEntry};
use crate::vault::list::{self, VaultIndex};
use serde_json::{json, Value};
use std::path::Path;

fn without_field(text: &str) -> Value {
    let mut value: Value = serde_json::from_str(text).unwrap();
    let strip = |list: &mut Vec<Value>| {
        for entry in list {
            entry.as_object_mut().unwrap().remove("organization");
        }
    };
    if let Some(list) = value.as_array_mut() {
        strip(list);
    } else if let Some(list) = value["accounts"].as_array_mut() {
        strip(list);
    }
    value
}

#[test]
fn with_no_document_every_answer_is_personal_and_nothing_is_closed() {
    let rig = Rig::full("org-personal");
    let view = rig.orgs.view();
    assert!(view.only_personal() && !view.is_closed());
    for kind in KINDS {
        for (key, resolved) in rig.orgs.resolve_all(kind) {
            assert_eq!(resolved.id, PERSONAL_ID, "{kind:?} {key}");
            assert_eq!(resolved.source, Source::Default);
            assert!(resolved.known);
        }
        let listing = rig.orgs.listing(kind).unwrap();
        assert!(
            view.gaps(kind, &listing).is_empty(),
            "no notice with one organization"
        );
    }
    assert_eq!(view.machine(None).id, PERSONAL_ID);
    assert_eq!(view.machine(Some("")).id, PERSONAL_ID);
    for folder in [
        "/",
        "/work/aptusfit/app",
        "/Users/me/code/x",
        "relative/path",
        "",
    ] {
        let found = view.folder_here(Path::new(folder), Path::new("/Users/me"));
        assert_eq!(found.organization, PERSONAL_ID, "{folder}");
        assert_eq!(found.folder, None);
    }
    assert_eq!(rig.orgs_text(), None, "answering writes nothing");
    assert_eq!(rig.backups(), Vec::<String>::new());
}

#[test]
fn nothing_the_stores_say_changes_but_the_field_after_the_migration() {
    let rig = Rig::full("org-personal-same");
    let claude_before: Vec<ClaudeAccount> = serde_json::from_str(CLAUDE).unwrap();
    let vault_dir = rig.data.clone();
    let vault_before = VaultIndex::in_data(&vault_dir).read().unwrap();
    let codex_file = DocFile::new(&rig.data, "accounts/codex", rig.tmp.path("codex-old.json"));
    let codex_before = read_accounts(&codex_file);
    let machines_before = rig.registry.all();

    rig.orgs.migrate(Some(&rig.registry)).unwrap();

    let claude_after: Vec<ClaudeAccount> =
        serde_json::from_str(&rig.text(Kind::Claude).unwrap()).unwrap();
    assert_eq!(claude_after.len(), claude_before.len());
    for (a, b) in claude_after.iter().zip(&claude_before) {
        assert_eq!(
            (&a.id, &a.email, &a.plan, a.needs_login),
            (&b.id, &b.email, &b.plan, b.needs_login)
        );
        assert_eq!(a.organization.as_deref(), Some(PERSONAL_ID));
    }
    let vault_after = VaultIndex::in_data(&vault_dir).read().unwrap();
    assert_eq!(vault_after.len(), vault_before.len());
    for (a, b) in vault_after.iter().zip(&vault_before) {
        let mut stripped = a.clone();
        stripped.organization = None;
        assert_eq!(&stripped, b);
        assert_eq!(a.organization.as_deref(), Some(PERSONAL_ID));
    }
    let codex_after = read_accounts(&codex_file);
    let strip = |list: &[Saved]| -> Vec<(String, String, Option<String>)> {
        list.iter()
            .map(|s| (s.id.clone(), s.email.clone(), s.plan.clone()))
            .collect()
    };
    assert_eq!(strip(&codex_after), strip(&codex_before));

    let machines_after = rig.registry.all();
    assert_eq!(machines_after.len(), machines_before.len());
    for (a, b) in machines_after.iter().zip(&machines_before) {
        let mut stripped = a.clone();
        stripped.organization = None;
        assert_eq!(&stripped, b);
        assert_eq!(a.organization_id(), PERSONAL_ID);
    }
    assert!(matches!(
        rig.registry.load(),
        RegistryState::Loaded {
            migrated: false,
            ..
        }
    ));

    for kind in KINDS {
        let before = match kind {
            Kind::Claude => CLAUDE,
            Kind::Codex => CODEX,
            Kind::Antigravity => ANTIGRAVITY,
            Kind::Vault => VAULT,
        };
        assert_eq!(
            without_field(&rig.text(kind).unwrap()),
            serde_json::from_str::<Value>(before).unwrap(),
            "{kind:?}: only the field was added"
        );
    }
}

#[test]
fn a_record_with_no_organization_is_written_exactly_as_before() {
    let account = ClaudeAccount {
        id: "c1".into(),
        email: "a@example.com".into(),
        plan: "Pro".into(),
        needs_login: false,
        organization: None,
    };
    assert_eq!(
        serde_json::to_value(&account).unwrap(),
        json!({"id": "c1", "email": "a@example.com", "plan": "Pro", "needsLogin": false})
    );
    let entry = VaultEntry::new("npm", VaultKind::Token);
    let text = String::from_utf8(list::encode(&[entry])).unwrap();
    assert!(!text.contains("organization"), "{text}");
    let machine = Machine::new("m1", "m1.tail", "me");
    assert!(!serde_json::to_string(&machine)
        .unwrap()
        .contains("organization"));
    assert_eq!(machine.organization_id(), PERSONAL_ID);

    let rig = Rig::new("org-personal-bytes");
    let file = DocFile::new(&rig.data, "accounts/codex", rig.tmp.path("c.json"));
    assert!(crate::accounts::book::write_accounts(
        &file,
        &[Saved {
            id: "u:w".into(),
            email: "a@example.com".into(),
            plan: Some("Plus".into()),
            organization: None,
        }]
    ));
    assert_eq!(
        crate::store::doc(&rig.data, "accounts/codex")
            .unwrap()
            .unwrap(),
        serde_json::to_string_pretty(&json!({"version": 1, "accounts": [
            {"id": "u:w", "email": "a@example.com", "plan": "Plus"}
        ]}))
        .unwrap()
    );
}

#[test]
fn an_older_v2_registry_is_read_as_it_was_and_saving_it_keeps_a_copy() {
    let rig = Rig::new("org-personal-registry");
    crate::store::put_doc(&rig.data, "machines/registry", REGISTRY_V2).unwrap();
    let machines = rig.registry.all();
    assert_eq!(machines.len(), 2);
    assert!(machines.iter().all(|m| m.organization.is_none()));
    assert_eq!(
        rig.registry_text().as_deref(),
        Some(REGISTRY_V2),
        "a read writes nothing"
    );
    assert!(rig.registry.save(&machines));
    let saved: Value = serde_json::from_str(&rig.registry_text().unwrap()).unwrap();
    assert_eq!(saved["version"], 3);
    assert_eq!(
        std::fs::read_to_string(crate::machines::registry::backup_path_v2(
            &rig.registry.old_own
        ))
        .unwrap(),
        REGISTRY_V2
    );
    assert!(rig.registry.save(&machines));
    assert_eq!(rig.registry.all(), machines);
}

#[test]
fn a_registry_of_a_later_version_is_still_refused() {
    let rig = Rig::new("org-personal-future");
    let future = r#"{"version":5,"machines":[]}"#;
    crate::store::put_doc(&rig.data, "machines/registry", future).unwrap();
    assert!(matches!(rig.registry.load(), RegistryState::Unreadable(_)));
    assert!(!rig.registry.save(&[]));
    assert_eq!(rig.registry_text().as_deref(), Some(future));
    assert!(rig.registry.adopt_organizations().is_err());
    assert_eq!(rig.registry_text().as_deref(), Some(future));
}
