use super::action::{Kind, Options};
use super::apply::{apply_only, before_text_hash, Only};
use super::manifest::Manifest;
use super::model::Profile;
use super::plan::{plan, ID_RULE, ID_SETTINGS};
use super::tests::*;
use super::undo::undo_only;
use crate::catalog::Tool;

fn setup(rig: &Rig) -> super::action::Plan {
    let (have, probe, manifest) = (avail(), Fake::all(), Manifest::default());
    plan(
        &env(rig, &have, &probe, &manifest),
        &Profile::default(),
        &Options::default(),
    )
}

fn only(ids: &[(&str, Option<String>)]) -> Only {
    ids.iter()
        .map(|(id, h)| (id.to_string(), h.clone()))
        .collect()
}

#[test]
fn an_unticked_file_is_not_written() {
    let rig = Rig::new("only-unticked").claude();
    let p = setup(&rig);
    let rule = p.get(ID_RULE).expect("rule");
    assert_eq!(rule.kind, Kind::Create);
    let settings_before = rig.read(&rig.settings());
    let (have, probe, manifest) = (avail(), Fake::all(), Manifest::default());
    let keep = only(&[(ID_RULE, before_text_hash(rule))]);
    let done = apply_only(&env(&rig, &have, &probe, &manifest), &p, 1000, Some(&keep));
    assert!(!done.failed(), "{:?}", done.results);
    assert!(rule.path.exists());
    assert_eq!(rig.read(&rig.settings()), settings_before);
    assert!(done.results.iter().all(|r| r.id == ID_RULE));
}

#[test]
fn a_file_changed_after_the_plan_is_reported_stale_and_left_alone() {
    let rig = Rig::new("only-stale").claude();
    let p = setup(&rig);
    let settings = p.get(ID_SETTINGS).expect("settings");
    assert_eq!(settings.kind, Kind::Update);
    let expected = before_text_hash(settings);
    let changed = format!("{}\n", rig.read(&rig.settings()).trim_end());
    std::fs::write(&settings.path, format!("{changed}\n\n")).unwrap();
    let on_disk = rig.read(&rig.settings());
    let p = setup(&rig);
    let (have, probe, manifest) = (avail(), Fake::all(), Manifest::default());
    let keep = only(&[(ID_SETTINGS, expected)]);
    let done = apply_only(&env(&rig, &have, &probe, &manifest), &p, 1000, Some(&keep));
    let result = done.results.iter().find(|r| r.id == ID_SETTINGS).unwrap();
    assert!(result.stale);
    assert!(!result.done);
    assert_eq!(rig.read(&rig.settings()), on_disk);
}

#[test]
fn an_undo_restores_only_the_kept_files() {
    let rig = Rig::new("only-undo").claude();
    let p = setup(&rig);
    let (have, probe, manifest) = (avail(), Fake::all(), Manifest::default());
    let done = super::apply::apply(&env(&rig, &have, &probe, &manifest), &p, 1000);
    assert!(!done.failed(), "{:?}", done.results);
    let rule = p.get(ID_RULE).unwrap().path.clone();
    let settings_after = rig.read(&rig.settings());
    let manifest = done.manifest.clone();
    let e = env(&rig, &have, &probe, &manifest);
    let entry = manifest
        .of_tool(Tool::Claude)
        .into_iter()
        .find(|x| x.id == ID_RULE)
        .unwrap();
    let now = super::undo::current_hash(&e, entry);
    let keep = only(&[(ID_RULE, now)]);
    let (results, _) = undo_only(&e, Tool::Claude, 2000, Some(&keep));
    assert_eq!(results.len(), 1);
    assert!(!rule.exists());
    assert_eq!(rig.read(&rig.settings()), settings_after);
}

#[test]
fn an_undo_of_a_file_changed_since_is_stale() {
    let rig = Rig::new("only-undo-stale").claude();
    let p = setup(&rig);
    let (have, probe, manifest) = (avail(), Fake::all(), Manifest::default());
    let done = super::apply::apply(&env(&rig, &have, &probe, &manifest), &p, 1000);
    let rule = p.get(ID_RULE).unwrap().path.clone();
    let manifest = done.manifest.clone();
    let e = env(&rig, &have, &probe, &manifest);
    let entry = manifest
        .of_tool(Tool::Claude)
        .into_iter()
        .find(|x| x.id == ID_RULE)
        .unwrap();
    let asked = super::undo::current_hash(&e, entry);
    std::fs::write(&rule, "edited by hand\n").unwrap();
    let keep = only(&[(ID_RULE, asked)]);
    let (results, _) = undo_only(&e, Tool::Claude, 2000, Some(&keep));
    assert_eq!(results[0].detail.as_deref(), Some("stale"));
    assert_eq!(std::fs::read_to_string(&rule).unwrap(), "edited by hand\n");
}
