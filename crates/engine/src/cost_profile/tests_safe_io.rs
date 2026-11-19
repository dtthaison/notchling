use super::action::{hash, Options, CONSENT_RULES};
use super::apply::apply;
use super::manifest::{Entry, Manifest, Part};
use super::model::Profile;
use super::plan::{plan, ID_SETTINGS};
use super::safe::{backup, guard, publish_new, replace_if, Publish};
use super::tests::*;
use super::undo::{undo, Outcome};
use crate::catalog::Tool;

#[test]
fn publishing_never_replaces_a_file_that_is_already_there() {
    let rig = Rig::new("publish");
    let path = rig.dir.join("a.md");
    rig.write(&path, "theirs");
    assert_eq!(publish_new(&path, b"ours"), Err(Publish::Exists));
    assert_eq!(rig.read(&path), "theirs");
    let fresh = rig.dir.join("b.md");
    assert_eq!(publish_new(&fresh, b"ours"), Ok(()));
    assert_eq!(rig.read(&fresh), "ours");
    assert_eq!(
        rig.names(&rig.dir)
            .iter()
            .filter(|n| n.ends_with(".tmp"))
            .count(),
        0
    );
}

#[test]
fn a_backup_name_already_taken_gets_the_next_free_one() {
    let rig = Rig::new("backup-name");
    let path = rig.dir.join("settings.json");
    rig.write(&path, "now");
    rig.write(&rig.dir.join("settings.json.notchling-setup-7"), "theirs");
    let made = backup(&path, "original", 7).expect("backup");
    assert_ne!(made, rig.dir.join("settings.json.notchling-setup-7"));
    assert_eq!(rig.read(&made), "original");
    assert_eq!(
        rig.read(&rig.dir.join("settings.json.notchling-setup-7")),
        "theirs"
    );
}

#[test]
fn a_file_changed_after_the_plan_is_never_overwritten() {
    let rig = Rig::new("replace");
    let path = rig.dir.join("a.md");
    rig.write(&path, "planned");
    rig.write(&path, "edited since");
    let out = replace_if(&path, "planned", false, b"ours");
    assert!(out.is_err());
    assert_eq!(rig.read(&path), "edited since");
    assert_eq!(replace_if(&path, "edited since", false, b"ours"), Ok(()));
    assert_eq!(rig.read(&path), "ours");
}

#[cfg(unix)]
#[test]
fn a_link_anywhere_below_a_tools_folder_is_refused() {
    let rig = Rig::new("guard").claude();
    let elsewhere = rig.dir.join("elsewhere");
    std::fs::create_dir_all(&elsewhere).expect("dir");
    std::os::unix::fs::symlink(&elsewhere, rig.roots.claude.join("agents")).expect("link");
    assert!(guard(&rig.roots, &rig.agent("x.md")).is_err());
    assert!(guard(&rig.roots, &rig.rule("x.md")).is_ok());
    assert!(guard(&rig.roots, &rig.dir.join("outside.md")).is_err());
}

#[test]
fn an_interrupted_apply_that_never_wrote_is_dropped_by_undo_and_the_file_stays() {
    let rig = Rig::new("pending").claude();
    let before = rig.read(&rig.settings());
    let (have, probe) = (avail(), Fake::all());
    let manifest = Manifest::default();
    let planned = {
        let e = env(&rig, &have, &probe, &manifest);
        plan(&e, &Profile::default(), &Options::default())
    };
    let action = planned.get(ID_SETTINGS).expect("settings");
    let journaled = Manifest {
        entries: vec![Entry {
            id: ID_SETTINGS.to_string(),
            tool: Tool::Claude,
            path: rig.settings(),
            part: Part::Keys,
            removed: false,
            hash: None,
            backup: None,
            existed: true,
            keys: action.keys.clone(),
            at: 1,
            pending: true,
            before_hash: Some(hash(&before)),
            released: false,
        }],
    };
    let (results, left) = undo(&env(&rig, &have, &probe, &journaled), Tool::Claude, 2);
    assert!(
        results.iter().all(|r| r.outcome != Outcome::Failed),
        "{results:?}"
    );
    assert!(left.entries.is_empty());
    assert_eq!(rig.read(&rig.settings()), before);
}

#[test]
fn an_apply_leaves_no_pending_entry_behind() {
    let rig = Rig::new("settled").claude();
    rig.write(&rig.rule("model-choice.md"), MODEL_CHOICE);
    let (have, probe) = (avail(), Fake::all());
    let manifest = Manifest::default();
    let e = env(&rig, &have, &probe, &manifest);
    let planned = plan(
        &e,
        &Profile::default(),
        &Options::consenting(&[CONSENT_RULES]),
    );
    let done = apply(&e, &planned, 1);
    assert!(!done.failed(), "{:?}", done.results);
    assert!(done.manifest.entries.iter().all(|x| !x.pending));
    let saved = Manifest::load(&rig.data).expect("saved");
    assert_eq!(saved, done.manifest);
}
