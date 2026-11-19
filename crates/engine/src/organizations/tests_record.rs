use super::tests_support::*;
use super::{
    Counts, Document, EditError, Kind, MachineSettings, Problem, KINDS, NAME_MAX, PERSONAL_ID,
};

fn doc_with(names: &[&str]) -> Document {
    let mut doc = Document::personal_only();
    for name in names {
        doc.create(name, None, None).unwrap();
    }
    doc
}

#[test]
fn names_are_unique_whatever_the_case_and_between_one_and_forty_characters() {
    let mut doc = doc_with(&["AptusFIT"]);
    assert_eq!(
        doc.create("aptusfit", None, None),
        Err(vec![Problem::NameTaken("aptusfit".into())])
    );
    assert_eq!(
        doc.create("  PERSONAL ", None, None),
        Err(vec![Problem::NameTaken("PERSONAL".into())])
    );
    assert_eq!(doc.create("   ", None, None), Err(vec![Problem::NameEmpty]));
    assert_eq!(
        doc.create("a\nb", None, None),
        Err(vec![Problem::NameEmpty])
    );
    let long = "x".repeat(NAME_MAX + 1);
    assert_eq!(
        doc.create(&long, None, None),
        Err(vec![Problem::NameTooLong])
    );
    assert!(doc.create(&"x".repeat(NAME_MAX), None, None).is_ok());
    assert_eq!(doc.organizations.len(), 3, "a refusal adds nothing");
}

#[test]
fn an_id_is_made_once_from_the_name_and_a_rename_never_changes_it() {
    let mut doc = Document::personal_only();
    let a = doc.create("Company A", None, None).unwrap();
    assert_eq!(a, "company-a");
    let b = doc.create("company a", None, None).unwrap_err();
    assert_eq!(b, vec![Problem::NameTaken("company a".into())]);
    doc.rename(&a, "Acme").unwrap();
    assert_eq!(doc.find("company-a").unwrap().name, "Acme");
    let again = doc.create("Company A", None, None).unwrap();
    assert_eq!(
        again, "company-a-2",
        "an id is never reused for another record"
    );
    let odd = doc.create("日本", None, None).unwrap();
    assert_eq!(odd, "organization");
    assert_eq!(
        doc.rename("nobody", "X"),
        Err(Problem::IdUnknown("nobody".into()))
    );
    assert_eq!(
        doc.rename(&a, "personal"),
        Err(Problem::NameTaken("personal".into()))
    );
    assert_eq!(
        doc.rename(&a, "ACME"),
        Ok(()),
        "a record may change only its own case"
    );
    assert!(doc.problems().is_empty());
}

#[test]
fn personal_can_be_renamed_but_never_deleted() {
    let mut doc = doc_with(&["Acme"]);
    doc.rename(PERSONAL_ID, "Me").unwrap();
    assert_eq!(doc.find(PERSONAL_ID).unwrap().name, "Me");
    assert_eq!(
        doc.delete(PERSONAL_ID, Counts::default()),
        Err(Problem::PersonalNotDeletable)
    );
    assert!(doc.find(PERSONAL_ID).is_some());
    let mut stripped = doc.clone();
    stripped.organizations.retain(|o| o.id != PERSONAL_ID);
    assert!(stripped.problems().contains(&Problem::PersonalMissing));
    assert!(Document::parse(&stripped.text().unwrap()).is_err());
}

#[test]
fn an_organization_goes_only_when_nothing_belongs_to_it() {
    let mut doc = doc_with(&["Acme"]);
    let busy = Counts {
        machines: 1,
        accounts: 0,
        secrets: 2,
    };
    assert_eq!(doc.delete("acme", busy), Err(Problem::NotEmpty(busy)));
    assert!(doc.find("acme").is_some());
    assert_eq!(
        doc.delete("ghost", Counts::default()),
        Err(Problem::IdUnknown("ghost".into()))
    );
    assert_eq!(doc.delete("acme", Counts::default()), Ok(()));
    assert!(doc.find("acme").is_none());
}

#[test]
fn a_folder_is_in_one_organization_only_and_is_kept_in_one_form() {
    let mut doc = doc_with(&["Acme", "Beta"]);
    doc.set_folders(
        "acme",
        &[
            "~/work/acme/".into(),
            "~/work/./acme".into(),
            "/srv/acme".into(),
        ],
    )
    .unwrap();
    assert_eq!(
        doc.find("acme").unwrap().folders,
        vec!["~/work/acme", "/srv/acme"]
    );
    assert_eq!(
        doc.set_folders("beta", &["~/work/acme".into()]),
        Err(Problem::FolderTaken {
            folder: "~/work/acme".into(),
            by: "acme".into()
        })
    );
    assert_eq!(
        doc.set_folders("beta", &["relative".into()]),
        Err(Problem::FolderInvalid("relative".into()))
    );
    assert_eq!(
        doc.set_folders("beta", &["/".into()]),
        Err(Problem::FolderInvalid("/".into()))
    );
    doc.set_folders("beta", &["~/work/acme/deploy".into()])
        .unwrap();
    doc.set_folders("acme", &[]).unwrap();
    assert!(doc.problems().is_empty());
}

#[test]
fn color_owner_and_machine_settings_are_checked() {
    let mut doc = doc_with(&["Acme"]);
    assert_eq!(
        doc.set_color("acme", "Orange!"),
        Err(Problem::ColorInvalid("Orange!".into()))
    );
    assert!(doc.set_color("acme", "orange").is_ok());
    assert_eq!(
        doc.set_github_owner("acme", Some("-bad-")),
        Err(Problem::OwnerInvalid("-bad-".into()))
    );
    assert!(doc.set_github_owner("acme", Some(" AptusFIT ")).is_ok());
    assert_eq!(
        doc.find("acme").unwrap().github_owner.as_deref(),
        Some("AptusFIT")
    );
    assert!(doc.set_github_owner("acme", None).is_ok());
    assert_eq!(doc.find("acme").unwrap().github_owner, None);
    let settings = MachineSettings {
        worker_login: Some("acme-claude".into()),
        ..MachineSettings::default()
    };
    assert!(doc
        .set_machine_settings("acme", Some(settings.clone()))
        .is_ok());
    assert_eq!(doc.find("acme").unwrap().machines, Some(settings));
}

#[test]
fn the_document_keeps_what_it_does_not_know_and_refuses_a_bad_one() {
    let text = r#"{"version":1,"organizations":[{"id":"personal","name":"Personal","color":"grey","folders":[],"later":{"a":1}}],"shadow":{"accounts":{},"vault":{}},"extra":true}"#;
    let doc = Document::parse(text).unwrap();
    let back: serde_json::Value = serde_json::from_str(&doc.text().unwrap()).unwrap();
    assert_eq!(back["extra"], true);
    assert_eq!(back["organizations"][0]["later"]["a"], 1);
    assert!(Document::parse(r#"{"version":1,"organizations":[{"id":"personal"}]}"#).is_err());
    assert!(Document::parse(r#"{"version":9,"organizations":[]}"#).is_err());
}

#[test]
fn an_edit_that_breaks_the_document_is_rolled_back() {
    let rig = Rig::new("org-record-edit");
    rig.orgs.create("Acme", None, None).unwrap();
    let before = rig.orgs_text();
    let refused = rig.orgs.edit(|doc| {
        doc.organizations[1].name = "personal".into();
        Ok(())
    });
    assert!(matches!(refused, Err(EditError::Problems(_))));
    assert_eq!(rig.orgs_text(), before);
    assert_eq!(
        rig.orgs.delete(PERSONAL_ID, &rig.registry),
        Err(EditError::Problems(vec![Problem::PersonalNotDeletable]))
    );
    assert_eq!(rig.orgs_text(), before);
}

#[test]
fn counts_and_delete_follow_what_belongs_to_each_organization() {
    let rig = Rig::full("org-record-counts");
    rig.orgs.migrate(Some(&rig.registry)).unwrap();
    rig.orgs.create("Acme", None, None).unwrap();
    assert_eq!(
        rig.orgs.counts(Some(&rig.registry))[PERSONAL_ID],
        Counts {
            machines: 2,
            accounts: 5,
            secrets: 2
        }
    );
    rig.orgs
        .set_organization(Kind::Vault, "github-read", "acme")
        .unwrap();
    let busy = Counts {
        machines: 0,
        accounts: 0,
        secrets: 1,
    };
    assert_eq!(
        rig.orgs.delete("acme", &rig.registry),
        Err(EditError::Problems(vec![Problem::NotEmpty(busy)]))
    );
    rig.orgs
        .set_organization(Kind::Vault, "github-read", PERSONAL_ID)
        .unwrap();
    assert_eq!(rig.orgs.delete("acme", &rig.registry), Ok(()));
    assert_eq!(rig.orgs.view().organizations().len(), 1);
    let total: usize = KINDS.iter().map(|k| rig.orgs.resolve_all(*k).len()).sum();
    assert_eq!(total, 7);
}

#[test]
fn two_writers_at_once_lose_no_organization() {
    let rig = Rig::new("org-record-writers");
    rig.orgs.migrate(None).unwrap();
    std::thread::scope(|scope| {
        for t in 0..4 {
            let orgs = rig.orgs.clone();
            scope.spawn(move || {
                for i in 0..5 {
                    orgs.create(&format!("Org {t} {i}"), None, None).unwrap();
                }
            });
        }
    });
    assert_eq!(rig.orgs.view().organizations().len(), 21);
}
