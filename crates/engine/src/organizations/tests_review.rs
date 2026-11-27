use super::tests_support::{Rig, CLAUDE, VAULT};
use super::{folders, EditError, Kind, MigrateError, Organizations, Problem, State, View, DOC};
use crate::accounts::book::{write_accounts, Saved, ANTIGRAVITY_DOC, CODEX_DOC};
use crate::docfile::DocFile;
use crate::store;

fn saved(id: &str, email: &str, organization: Option<&str>) -> Saved {
    Saved {
        id: id.into(),
        email: email.into(),
        plan: None,
        organization: organization.map(str::to_owned),
    }
}

fn field(rig: &Rig, kind: Kind, key: &str) -> Option<String> {
    rig.orgs
        .listing(kind)
        .unwrap()
        .into_iter()
        .find(|(k, _)| *k == kind.normal(key))
        .and_then(|(_, f)| f)
}

fn with_acme() -> Rig {
    let rig = Rig::full("org-review");
    rig.orgs.migrate(Some(&rig.registry)).unwrap();
    rig.orgs.create("Acme", None, None).unwrap();
    rig
}

#[test]
fn a_cached_list_written_after_an_organization_was_set_keeps_the_new_one() {
    let rig = with_acme();
    let cases = [
        (
            Kind::Codex,
            CODEX_DOC,
            "daemon/accounts/codex.json",
            "u1:w1",
            "x@example.com",
        ),
        (
            Kind::Antigravity,
            ANTIGRAVITY_DOC,
            "daemon/accounts/antigravity.json",
            "g1",
            "g@example.com",
        ),
    ];
    for (kind, doc, old, id, email) in cases {
        let stale = vec![
            saved(id, email, Some("personal")),
            saved("n:new", "n@example.com", Some("acme")),
        ];
        rig.orgs.set_organization(kind, id, "acme").unwrap();
        let file = DocFile::new(&rig.data, doc, rig.data.join(old));
        assert!(write_accounts(&file, &stale));
        assert_eq!(field(&rig, kind, id).as_deref(), Some("acme"));
        assert_eq!(field(&rig, kind, "n:new").as_deref(), Some("acme"));
        let stale_none = vec![saved(id, email, None)];
        assert!(write_accounts(&file, &stale_none));
        assert_eq!(field(&rig, kind, id).as_deref(), Some("acme"));
        assert_eq!(field(&rig, kind, "n:new"), None);
    }
}

#[test]
fn creating_refuses_when_the_marking_cannot_be_done() {
    let rig = Rig::full("org-create-refused");
    store::put_doc(&rig.data, DOC, "not json").unwrap();
    assert!(matches!(
        rig.orgs.create("Acme", None, None),
        Err(EditError::Unreadable(_))
    ));
    assert_eq!(rig.orgs_text().as_deref(), Some("not json"));
}

#[test]
fn a_list_still_in_its_old_file_is_imported_backed_up_and_marked() {
    let rig = Rig::new("org-legacy");
    rig.tmp.write("data/daemon/accounts.json", CLAUDE);
    rig.tmp.write("data/daemon/vault.json", VAULT);
    let report = rig.orgs.migrate(None).unwrap();
    assert_eq!(report.stamped, 4);
    assert!(!rig.data.join("daemon/accounts.json").exists());
    assert!(!rig.data.join("daemon/vault.json").exists());
    assert_eq!(field(&rig, Kind::Claude, "c1").as_deref(), Some("personal"));
    assert_eq!(
        field(&rig, Kind::Vault, "github-read").as_deref(),
        Some("personal")
    );
    assert_eq!(rig.backup_text(Kind::Claude).as_deref(), Some(CLAUDE));
    assert_eq!(rig.backup_text(Kind::Vault).as_deref(), Some(VAULT));
}

#[test]
fn deleting_counts_what_is_stored_now_and_refuses_over_an_unreadable_store() {
    let rig = with_acme();
    rig.orgs
        .set_organization(Kind::Codex, "u1:w1", "acme")
        .unwrap();
    assert!(matches!(
        rig.orgs.delete("acme", &rig.registry),
        Err(EditError::Problems(p)) if matches!(p[0], Problem::NotEmpty(_))
    ));
    rig.orgs
        .set_organization(Kind::Codex, "u1:w1", "personal")
        .unwrap();
    rig.put(Kind::Vault, "not a list");
    assert!(matches!(
        rig.orgs.delete("acme", &rig.registry),
        Err(EditError::Unreadable(_))
    ));
    rig.put(Kind::Vault, "[]");
    assert_eq!(rig.orgs.delete("acme", &rig.registry), Ok(()));
    assert!(matches!(rig.orgs.state(), State::Loaded(d) if d.find("acme").is_none()));
}

#[test]
fn a_blank_shadow_value_reads_as_personal_and_is_a_problem() {
    let rig = with_acme();
    let State::Loaded(mut doc) = rig.orgs.state() else {
        panic!("loaded");
    };
    doc.shadow
        .accounts
        .insert("codex:u1:w1".into(), "  ".into());
    assert!(doc
        .problems()
        .iter()
        .any(|p| matches!(p, Problem::ShadowInvalid(k) if k == "codex:u1:w1")));
    let view = View::new(State::Loaded(doc));
    assert_eq!(view.resolve(Kind::Codex, "u1:w1", None).id, "personal");
}

#[test]
fn a_migrate_that_cannot_read_the_document_fails_without_writing() {
    let rig = Rig::full("org-migrate-fail");
    store::put_doc(&rig.data, DOC, "{").unwrap();
    assert!(matches!(
        rig.orgs.migrate(None),
        Err(MigrateError::Unreadable(_))
    ));
    assert_eq!(rig.orgs_text().as_deref(), Some("{"));
}

#[test]
fn a_colour_is_trimmed_before_it_is_checked() {
    let rig = with_acme();
    rig.orgs.set_color("acme", "  orange ").unwrap();
    let State::Loaded(doc) = rig.orgs.state() else {
        panic!("loaded");
    };
    assert_eq!(doc.find("acme").unwrap().color, "orange");
    assert_eq!(
        rig.orgs.create("Beta", None, Some(" teal ")),
        Ok("beta".into())
    );
    assert!(rig.orgs.set_color("acme", "  ").is_err());
}

#[test]
fn a_field_that_is_not_an_id_is_left_alone_and_never_reaches_the_shadow() {
    let rig = Rig::new("org-junk-field");
    rig.put(
        Kind::Codex,
        r#"{"version":1,"accounts":[{"id":"u1:w1","email":"x@example.com","plan":null,"organization":"Acme Corp"},{"id":"u2:w2","email":"y@example.com","plan":null}]}"#,
    );
    let report = rig.orgs.migrate(None).unwrap();
    assert_eq!(report.stamped, 1);
    assert_eq!(report.skipped.len(), 1);
    let State::Loaded(doc) = rig.orgs.state() else {
        panic!("the document must stay readable");
    };
    assert!(!doc.shadow.accounts.contains_key("codex:u1:w1"));
    assert_eq!(
        doc.shadow.accounts.get("codex:u2:w2").map(String::as_str),
        Some("personal")
    );
    assert_eq!(
        field(&rig, Kind::Codex, "u1:w1").as_deref(),
        Some("Acme Corp")
    );
    let resolved = rig.orgs.resolve(Kind::Codex, "u1:w1");
    assert_eq!((resolved.id.as_str(), resolved.known), ("personal", false));
    assert!(!rig.orgs.view().is_closed());
    assert!(rig.orgs.migrate(None).is_ok());
    assert!(!rig.orgs.view().is_closed());
}

#[test]
fn deleting_counts_the_machines_of_the_registry_it_is_given() {
    let rig = with_acme();
    let mut machines = rig.registry.all();
    machines[0].organization = Some("acme".into());
    assert!(rig.registry.save(&machines));
    assert!(matches!(
        rig.orgs.delete("acme", &rig.registry),
        Err(EditError::Problems(p)) if matches!(&p[0], Problem::NotEmpty(c) if c.machines == 1)
    ));
    machines[0].organization = Some("personal".into());
    assert!(rig.registry.save(&machines));
    assert_eq!(rig.orgs.delete("acme", &rig.registry), Ok(()));
}

#[cfg(unix)]
#[test]
fn folders_overlap_on_the_expanded_and_resolved_form_and_on_case_where_the_disk_ignores_it() {
    use std::path::Path;
    let rig = with_acme();
    let orgs = Organizations::new(&rig.data).with_home(Path::new("/Users/me"));
    orgs.create("Beta", None, None).unwrap();
    orgs.set_folders("acme", &["~/work".to_owned()]).unwrap();
    assert!(matches!(
        orgs.set_folders("beta", &["/Users/me/work".to_owned()]),
        Err(EditError::Problems(p)) if matches!(p[0], Problem::FolderTaken { .. })
    ));
    assert_eq!(
        orgs.set_folders("acme", &["~/work".to_owned(), "/Users/me/work".to_owned()]),
        Ok(())
    );
    let State::Loaded(doc) = orgs.state() else {
        panic!("loaded");
    };
    assert_eq!(doc.find("acme").unwrap().folders, ["~/work"]);
    let upper = orgs.set_folders("beta", &["/Users/ME/Work".to_owned()]);
    if folders::folds_case() {
        assert!(upper.is_err());
    } else {
        assert_eq!(upper, Ok(()));
    }
}

#[test]
fn a_remove_forgets_the_shadow_so_a_re_added_name_starts_in_personal() {
    let rig = with_acme();
    rig.orgs
        .set_organization(Kind::Vault, "github-read", "acme")
        .unwrap();
    rig.orgs.forget(Kind::Vault, "GitHub-Read").unwrap();
    let State::Loaded(doc) = rig.orgs.state() else {
        panic!("loaded");
    };
    assert!(!doc.shadow.vault.contains_key("github-read"));
    let before = rig.orgs_text();
    rig.orgs.forget(Kind::Vault, "github-read").unwrap();
    assert_eq!(rig.orgs_text(), before, "nothing to forget writes nothing");
}
