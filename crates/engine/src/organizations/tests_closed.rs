use super::tests_support::*;
use super::{EditError, Kind, MigrateError, State, KINDS, PERSONAL_ID};
use crate::machines::registry::backup_path_v2;
use std::path::Path;

const BAD: [(&str, &str); 6] = [
    ("garbage", "not json"),
    ("other version", r#"{"version":2,"organizations":[]}"#),
    (
        "no personal",
        r#"{"version":1,"organizations":[{"id":"acme","name":"Acme","color":"grey","folders":[]}]}"#,
    ),
    (
        "same name twice",
        r#"{"version":1,"organizations":[{"id":"personal","name":"Personal"},{"id":"a","name":"personal"}]}"#,
    ),
    (
        "same id twice",
        r#"{"version":1,"organizations":[{"id":"personal","name":"Personal"},{"id":"a","name":"A"},{"id":"a","name":"B"}]}"#,
    ),
    (
        "one folder in two",
        r#"{"version":1,"organizations":[{"id":"personal","name":"Personal","folders":["~/w"]},{"id":"a","name":"A","folders":["~/w/"]}]}"#,
    ),
];

fn snapshot(rig: &Rig) -> Vec<Option<String>> {
    let mut all: Vec<Option<String>> = KINDS.iter().map(|k| rig.text(*k)).collect();
    all.push(rig.orgs_text());
    all.push(rig.registry_text());
    all
}

#[test]
fn an_unreadable_document_closes_the_view_and_nothing_is_written() {
    for (tag, bad) in BAD {
        let rig = Rig::full("org-closed");
        crate::store::put_doc(&rig.data, super::DOC, bad).unwrap();
        let before = snapshot(&rig);

        assert!(matches!(rig.orgs.state(), State::Unreadable(_)), "{tag}");
        let view = rig.orgs.view();
        assert!(view.is_closed() && view.why_closed().is_some(), "{tag}");
        assert!(view.only_personal());

        let migrated = rig.orgs.migrate(Some(&rig.registry));
        assert!(
            matches!(migrated, Err(MigrateError::Unreadable(_))),
            "{tag}"
        );
        assert!(matches!(
            rig.orgs.create("Acme", None, None),
            Err(EditError::Unreadable(_))
        ));
        assert!(matches!(
            rig.orgs.rename(PERSONAL_ID, "Me"),
            Err(EditError::Unreadable(_))
        ));
        assert!(matches!(
            rig.orgs.set_organization(Kind::Claude, "c1", PERSONAL_ID),
            Err(EditError::Unreadable(_))
        ));
        assert!(matches!(
            rig.orgs.forget(Kind::Claude, "c1"),
            Err(EditError::Unreadable(_))
        ));
        assert_eq!(snapshot(&rig), before, "{tag}: nothing was written over");
        assert!(rig.backups().is_empty(), "{tag}");
        assert!(!backup_path_v2(&rig.registry.old_own).exists(), "{tag}");
    }
}

#[test]
fn a_closed_view_still_reads_the_fields_but_places_every_folder_in_personal() {
    let rig = Rig::full("org-closed-read");
    rig.orgs.migrate(None).unwrap();
    rig.orgs.create("AptusFIT", None, None).unwrap();
    rig.orgs
        .set_organization(Kind::Claude, "c2", "aptusfit")
        .unwrap();
    rig.orgs
        .set_folders("aptusfit", &["/work/aptus".to_owned()])
        .unwrap();
    crate::store::put_doc(&rig.data, super::DOC, "not json").unwrap();

    let view = rig.orgs.view();
    assert!(view.is_closed());
    let resolved = rig.orgs.resolve(Kind::Claude, "c2");
    assert_eq!(resolved.id, "aptusfit");
    assert!(!resolved.known, "the closed view knows only Personal");
    assert_eq!(
        view.folder_here(Path::new("/work/aptus/app"), Path::new("/home/me"))
            .organization,
        PERSONAL_ID
    );
}

#[test]
fn a_closed_view_is_not_the_same_as_a_missing_document() {
    let rig = Rig::new("org-closed-missing");
    let view = rig.orgs.view();
    assert!(!view.is_closed());
    assert!(matches!(rig.orgs.state(), State::Missing));
}

#[cfg(unix)]
#[test]
fn a_database_that_is_a_link_is_unreadable_and_is_not_followed() {
    let rig = Rig::new("org-closed-link");
    std::fs::create_dir_all(&rig.data).unwrap();
    let victim = rig.tmp.write("victim.db", "mine");
    std::os::unix::fs::symlink(&victim, crate::store::db_path(&rig.data)).unwrap();
    assert!(matches!(rig.orgs.state(), State::Unreadable(_)));
    assert!(rig.orgs.view().is_closed());
    assert!(rig.orgs.migrate(None).is_err());
    assert_eq!(std::fs::read_to_string(victim).unwrap(), "mine");
}
