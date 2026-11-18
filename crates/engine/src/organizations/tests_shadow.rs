use super::tests_support::*;
use super::{EditError, Gap, Kind, Problem, Source, KINDS, PERSONAL_ID};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Serialize, Deserialize)]
struct OldClaude {
    id: String,
    email: String,
    plan: String,
    #[serde(rename = "needsLogin", default)]
    needs_login: bool,
}

#[derive(Serialize, Deserialize)]
struct OldVault {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    ask: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    expires: Option<String>,
    #[serde(default, rename = "howTo", skip_serializing_if = "Option::is_none")]
    how_to: Option<String>,
    kind: String,
    name: String,
    #[serde(default)]
    note: String,
    #[serde(default, rename = "secretVar")]
    secret_var: String,
    #[serde(default)]
    updated: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    url: Option<String>,
    #[serde(default, rename = "userVar")]
    user_var: String,
}

fn old_claude_save(rig: &Rig) {
    let list: Vec<OldClaude> = serde_json::from_str(&rig.text(Kind::Claude).unwrap()).unwrap();
    rig.put(Kind::Claude, &serde_json::to_string(&list).unwrap());
}

fn old_book_write(rig: &Rig, kind: Kind) {
    let root: Value = serde_json::from_str(&rig.text(kind).unwrap()).unwrap();
    let list: Vec<Value> = root["accounts"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| json!({"id": a["id"], "email": a["email"], "plan": a["plan"]}))
        .collect();
    let text = serde_json::to_string_pretty(&json!({"version": 1, "accounts": list})).unwrap();
    rig.put(kind, &text);
}

fn old_vault_save(rig: &Rig, rename: Option<(&str, &str)>) {
    let mut list: Vec<OldVault> = serde_json::from_str(&rig.text(Kind::Vault).unwrap()).unwrap();
    if let Some((from, to)) = rename {
        for entry in list.iter_mut().filter(|e| e.name == from) {
            entry.name = to.into();
        }
    }
    rig.put(Kind::Vault, &serde_json::to_string_pretty(&list).unwrap());
}

fn field(rig: &Rig, kind: Kind, key: &str) -> Option<String> {
    rig.orgs
        .listing(kind)
        .unwrap()
        .into_iter()
        .find(|(k, _)| *k == kind.normal(key))
        .and_then(|(_, f)| f)
}

fn two_orgs() -> Rig {
    let rig = Rig::full("org-shadow");
    rig.orgs.migrate(Some(&rig.registry)).unwrap();
    assert_eq!(
        rig.orgs
            .create("AptusFIT", Some("AptusFIT"), Some("orange")),
        Ok("aptusfit".into())
    );
    rig.orgs
        .set_organization(Kind::Claude, "c2", "aptusfit")
        .unwrap();
    rig.orgs
        .set_organization(Kind::Codex, "u1:w1", "aptusfit")
        .unwrap();
    rig.orgs
        .set_organization(Kind::Antigravity, "g1", "aptusfit")
        .unwrap();
    rig.orgs
        .set_organization(Kind::Vault, "Aptus-Deploy", "aptusfit")
        .unwrap();
    rig
}

#[test]
fn setting_an_organization_writes_the_field_and_the_shadow_together() {
    let rig = two_orgs();
    assert_eq!(field(&rig, Kind::Claude, "c2").as_deref(), Some("aptusfit"));
    assert_eq!(
        field(&rig, Kind::Claude, "c1").as_deref(),
        Some(PERSONAL_ID)
    );
    let view = rig.orgs.view();
    let shadow = &view.document().shadow;
    assert_eq!(shadow.accounts["claude:c2"], "aptusfit");
    assert_eq!(shadow.accounts["codex:u1:w1"], "aptusfit");
    assert_eq!(shadow.accounts["antigravity:g1"], "aptusfit");
    assert_eq!(shadow.vault["aptus-deploy"], "aptusfit");
    assert_eq!(shadow.accounts["claude:c1"], PERSONAL_ID);
    assert_eq!(
        rig.backup_text(Kind::Vault).as_deref(),
        Some(VAULT),
        "the backup is still the text before any organization"
    );
}

#[test]
fn an_older_build_that_saves_each_list_drops_the_field_and_the_shadow_brings_it_back() {
    let rig = two_orgs();
    old_claude_save(&rig);
    old_book_write(&rig, Kind::Codex);
    old_book_write(&rig, Kind::Antigravity);
    old_vault_save(&rig, None);
    for (kind, key) in [
        (Kind::Claude, "c2"),
        (Kind::Codex, "u1:w1"),
        (Kind::Antigravity, "g1"),
        (Kind::Vault, "aptus-deploy"),
    ] {
        assert_eq!(field(&rig, kind, key), None, "{kind:?} lost the field");
        let resolved = rig.orgs.resolve(kind, key);
        assert_eq!(resolved.id, "aptusfit", "{kind:?}");
        assert_eq!(resolved.source, Source::Shadow, "{kind:?}");
    }
    assert_eq!(rig.orgs.resolve(Kind::Claude, "c1").id, PERSONAL_ID);

    let report = rig.orgs.migrate(Some(&rig.registry)).unwrap();
    assert_eq!(report.restored, 7, "every entry the old saves stripped");
    assert!(report.gaps.is_empty());
    for (kind, key) in [
        (Kind::Claude, "c2"),
        (Kind::Codex, "u1:w1"),
        (Kind::Antigravity, "g1"),
        (Kind::Vault, "aptus-deploy"),
    ] {
        assert_eq!(
            field(&rig, kind, key).as_deref(),
            Some("aptusfit"),
            "{kind:?}"
        );
    }
    assert_eq!(
        field(&rig, Kind::Claude, "c1").as_deref(),
        Some(PERSONAL_ID)
    );
}

#[test]
fn the_field_wins_when_it_differs_from_the_shadow_and_the_shadow_is_fixed() {
    let rig = two_orgs();
    let text = rig
        .text(Kind::Claude)
        .unwrap()
        .replace("aptusfit", PERSONAL_ID);
    rig.put(Kind::Claude, &text);
    assert_eq!(rig.orgs.resolve(Kind::Claude, "c2").id, PERSONAL_ID);
    assert_eq!(rig.orgs.resolve(Kind::Claude, "c2").source, Source::Field);
    rig.orgs.migrate(None).unwrap();
    assert_eq!(
        rig.orgs.view().document().shadow.accounts["claude:c2"],
        PERSONAL_ID
    );
    assert_eq!(
        field(&rig, Kind::Claude, "c2").as_deref(),
        Some(PERSONAL_ID)
    );
}

#[test]
fn a_vault_entry_renamed_by_an_older_build_reads_as_personal_and_raises_one_notice() {
    let rig = two_orgs();
    old_vault_save(&rig, Some(("Aptus-Deploy", "aptusfit-deploy")));
    let resolved = rig.orgs.resolve(Kind::Vault, "aptusfit-deploy");
    assert_eq!(resolved.id, PERSONAL_ID);
    assert_eq!(resolved.source, Source::Default);

    let view = rig.orgs.view();
    let listing = rig.orgs.listing(Kind::Vault).unwrap();
    assert_eq!(
        view.gaps(Kind::Vault, &listing),
        vec![Gap {
            kind: Kind::Vault,
            key: "aptusfit-deploy".into()
        }]
    );
    assert!(view
        .gaps(Kind::Claude, &rig.orgs.listing(Kind::Claude).unwrap())
        .is_empty());

    let first = rig.orgs.migrate(None).unwrap();
    assert_eq!(
        first.gaps,
        vec![Gap {
            kind: Kind::Vault,
            key: "aptusfit-deploy".into()
        }]
    );
    assert_eq!(
        field(&rig, Kind::Vault, "aptusfit-deploy").as_deref(),
        Some(PERSONAL_ID),
        "it is Personal until you move it"
    );
    let second = rig.orgs.migrate(None).unwrap();
    assert!(second.gaps.is_empty(), "the notice comes once");
}

#[test]
fn with_only_personal_a_new_unmarked_entry_raises_no_notice() {
    let rig = Rig::full("org-shadow-quiet");
    rig.orgs.migrate(None).unwrap();
    old_vault_save(&rig, Some(("github-read", "github-read-2")));
    let view = rig.orgs.view();
    assert!(view
        .gaps(Kind::Vault, &rig.orgs.listing(Kind::Vault).unwrap())
        .is_empty());
    assert!(rig.orgs.migrate(None).unwrap().gaps.is_empty());
}

#[test]
fn creating_a_second_organization_first_marks_what_has_no_mark_without_a_notice() {
    let rig = Rig::full("org-shadow-create");
    rig.orgs.migrate(None).unwrap();
    old_vault_save(&rig, Some(("github-read", "github-read-2")));
    rig.orgs.create("AptusFIT", None, None).unwrap();
    assert_eq!(
        field(&rig, Kind::Vault, "github-read-2").as_deref(),
        Some(PERSONAL_ID)
    );
    assert!(rig.orgs.migrate(None).unwrap().gaps.is_empty());
}

#[test]
fn setting_an_organization_refuses_an_unknown_organization_or_entry_and_writes_nothing() {
    let rig = two_orgs();
    let before: Vec<Option<String>> = KINDS.iter().map(|k| rig.text(*k)).collect();
    let orgs = rig.orgs_text();
    assert_eq!(
        rig.orgs.set_organization(Kind::Claude, "c1", "nobody"),
        Err(EditError::Problems(vec![Problem::IdUnknown(
            "nobody".into()
        )]))
    );
    assert_eq!(
        rig.orgs.set_organization(Kind::Claude, "zzz", "aptusfit"),
        Err(EditError::NoSuchEntry)
    );
    let after: Vec<Option<String>> = KINDS.iter().map(|k| rig.text(*k)).collect();
    assert_eq!(before, after);
    assert_eq!(orgs, rig.orgs_text());
}

#[test]
fn forgetting_an_entry_drops_only_its_shadow() {
    let rig = two_orgs();
    rig.orgs.forget(Kind::Vault, "APTUS-DEPLOY").unwrap();
    let view = rig.orgs.view();
    assert!(!view.document().shadow.vault.contains_key("aptus-deploy"));
    assert!(view.document().shadow.accounts.contains_key("claude:c2"));
}

#[test]
fn the_new_writers_keep_the_field() {
    use crate::accounts::book::{read_accounts, write_accounts, Saved};
    use crate::accounts::claude::ClaudeAccount;
    use crate::docfile::DocFile;
    use crate::vault::entry::{Kind as VaultKind, VaultEntry};
    use crate::vault::list;

    let rig = Rig::new("org-shadow-writers");
    let file = DocFile::new(&rig.data, "accounts/codex", rig.tmp.path("codex.json"));
    let saved = vec![Saved {
        id: "u:w".into(),
        email: "a@example.com".into(),
        plan: None,
        organization: Some("aptusfit".into()),
    }];
    assert!(write_accounts(&file, &saved));
    assert_eq!(read_accounts(&file), saved);

    let account = ClaudeAccount {
        id: "c1".into(),
        email: "a@example.com".into(),
        plan: "Pro".into(),
        needs_login: false,
        organization: Some("aptusfit".into()),
    };
    let text = serde_json::to_string(std::slice::from_ref(&account)).unwrap();
    assert_eq!(
        serde_json::from_str::<Vec<ClaudeAccount>>(&text).unwrap(),
        vec![account]
    );

    let mut entry = VaultEntry::new("npm", VaultKind::Token);
    entry.organization = Some("aptusfit".into());
    let decoded = list::decode(&list::encode(std::slice::from_ref(&entry)));
    assert_eq!(decoded, vec![entry]);
}
