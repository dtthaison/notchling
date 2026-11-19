use super::action::{hash, Kind, Options, Plan, CONSENT_PSTACK, CONSENT_RULES};
use super::apply::{apply, Applied};
use super::check::{resolve, Choice};
use super::import::import_rules;
use super::keys::{claude_keys, codex_keys, undo_json_keys};
use super::manifest::Manifest;
use super::model::Profile;
use super::plan::{plan, ID_PSTACK, ID_RULE, ID_SETTINGS};
use super::pstack;
use super::tests::*;
use super::undo::{undo, Outcome};
use crate::catalog::Tool;
use serde_json::json;

fn rules() -> Options {
    Options::consenting(&[CONSENT_RULES])
}

fn run(
    rig: &Rig,
    manifest: &Manifest,
    profile: &Profile,
    opts: &Options,
    stamp: u64,
) -> (Plan, Applied) {
    let (have, probe) = (avail(), Fake::all());
    let e = env(rig, &have, &probe, manifest);
    let planned = plan(&e, profile, opts);
    let applied = apply(&e, &planned, stamp);
    (planned, applied)
}

fn undo_all(
    rig: &Rig,
    manifest: &Manifest,
    tool: Tool,
    stamp: u64,
) -> (Vec<super::undo::UndoResult>, Manifest) {
    let (have, probe) = (avail(), Fake::all());
    undo(&env(rig, &have, &probe, manifest), tool, stamp)
}

#[test]
fn an_inline_agents_table_is_never_given_a_second_declaration() {
    let text = "model = \"x\"\nagents = { max_threads = 3 }\n";
    let plan = codex_keys(text, &Profile::default(), &avail(), &Fake::all(), &[]).expect("plan");
    assert!(!plan.text.contains("[agents]"), "{}", plan.text);
    assert!(plan.text.contains("agents = { max_threads = 3 }"));
    let dotted = "model = \"x\"\nagents.max_threads = 3\n";
    let plan = codex_keys(dotted, &Profile::default(), &avail(), &Fake::all(), &[]).expect("plan");
    assert!(!plan.text.contains("[agents]"), "{}", plan.text);
    assert!(
        plan.text.contains("agents.default_subagent_model"),
        "{}",
        plan.text
    );
}

#[test]
fn an_old_rule_stays_when_its_replacement_is_skipped() {
    let rig = Rig::new("keep-old").claude();
    rig.write(&rig.rule("model-choice.md"), MODEL_CHOICE);
    rig.write(&rig.rule("notchling-cost.md"), "# my own file\n");
    let (_, done) = run(&rig, &Manifest::default(), &Profile::default(), &rules(), 1);
    assert!(rig.rule("model-choice.md").exists(), "{:?}", done.results);
    assert_eq!(rig.read(&rig.rule("notchling-cost.md")), "# my own file\n");
}

#[test]
fn an_old_rule_stays_when_its_replacement_fails_to_write() {
    let rig = Rig::new("keep-old-fail").claude();
    rig.write(&rig.rule("model-choice.md"), MODEL_CHOICE);
    let (have, probe, empty) = (avail(), Fake::all(), Manifest::default());
    let e = env(&rig, &have, &probe, &empty);
    let planned = plan(&e, &Profile::default(), &rules());
    rig.write(&rig.rule("notchling-cost.md"), "late arrival\n");
    let done = apply(&e, &planned, 2);
    assert!(done.failed());
    assert!(rig.rule("model-choice.md").exists());
    assert_eq!(rig.read(&rig.rule("notchling-cost.md")), "late arrival\n");
}

#[test]
fn an_unknown_effort_or_clause_is_kept_not_imported() {
    let text = "| Model | Effort | Use for |\n| --- | --- | --- |\n| sonnet | max | Routine code; never deploy |\n";
    let out = import_rules(text, &Profile::default());
    assert!(out.picks.is_empty(), "{:?}", out.picks);
    let kept = out.kept.join("\n");
    assert!(kept.contains("never deploy"), "{kept}");
    assert!(kept.contains("| Model | Effort | Use for |"), "{kept}");
    assert!(
        out.report.iter().all(|r| r.fate == "kept"),
        "{:?}",
        out.report
    );
}

#[test]
fn an_unrelated_table_keeps_its_header_and_separator() {
    let text = "| Name | Notes |\n| --- | --- |\n| deploy | never on friday |\n";
    let out = import_rules(text, &Profile::default());
    assert_eq!(out.kept.join("\n"), text.trim_end());
    assert!(out.report.iter().all(|r| r.fate == "kept"));
}

#[test]
fn a_known_row_with_its_own_effort_column_imports_that_effort() {
    let text = "| Model | Effort | Use for |\n| --- | --- | --- |\n| sonnet | max | Routine code: specified edits, tests, small features, mechanical refactors, git history digs. |\n";
    let out = import_rules(text, &Profile::default());
    assert_eq!(out.picks.len(), 1);
    assert_eq!(out.picks[0].1.to_string(), "sonnet max");
    assert!(out.kept.is_empty(), "{:?}", out.kept);
}

#[test]
fn two_imported_files_both_survive_and_both_undo() {
    let rig = Rig::new("two-imports").claude();
    rig.write(&rig.rule("model-a.md"), "# A\n\nnever deploy on friday\n");
    let (_, first) = run(&rig, &Manifest::default(), &Profile::default(), &rules(), 1);
    assert!(!first.failed(), "{:?}", first.results);
    assert!(!rig.rule("model-a.md").exists());
    rig.write(&rig.rule("model-b.md"), "# B\n\nalways squash\n");
    let (_, second) = run(&rig, &first.manifest, &first.profile, &rules(), 2);
    assert!(!second.failed(), "{:?}", second.results);
    let text = rig.read(&rig.rule("notchling-cost.md"));
    assert!(text.contains("never deploy on friday"), "{text}");
    assert!(text.contains("always squash"), "{text}");
    assert_eq!(
        second.manifest.entries.iter().filter(|e| e.removed).count(),
        2
    );
    let (results, left) = undo_all(&rig, &second.manifest, Tool::Claude, 3);
    assert!(
        results.iter().all(|r| r.outcome != Outcome::Conflict),
        "{results:?}"
    );
    assert!(left.entries.is_empty());
    assert_eq!(
        rig.read(&rig.rule("model-a.md")),
        "# A\n\nnever deploy on friday\n"
    );
    assert_eq!(rig.read(&rig.rule("model-b.md")), "# B\n\nalways squash\n");
}

#[test]
fn a_key_the_user_changed_stays_theirs_across_every_later_apply() {
    let rig = Rig::new("stays-theirs").claude();
    let (_, first) = run(
        &rig,
        &Manifest::default(),
        &Profile::default(),
        &Options::default(),
        1,
    );
    let edited = rig.read(&rig.settings()).replace("12000", "9000");
    rig.write(&rig.settings(), &edited);
    let saver = Profile::preset("saver");
    let (_, second) = run(&rig, &first.manifest, &saver, &Options::default(), 2);
    assert!(rig.read(&rig.settings()).contains("9000"));
    let (_, third) = run(&rig, &second.manifest, &saver, &Options::default(), 3);
    let text = rig.read(&rig.settings());
    assert!(text.contains("9000") && !text.contains("8000"), "{text}");
    assert!(!third.failed());
}

#[test]
fn a_record_for_a_model_that_left_the_list_is_kept() {
    let have = avail();
    let first = claude_keys(SETTINGS, &Profile::default(), &have, &[]).expect("plan");
    let mut gone = first.recs[1].clone();
    gone.path = vec![
        "modelSettings".into(),
        "claude-old-1".into(),
        "autoCompactWindow".into(),
    ];
    let mut prior = first.recs.clone();
    prior.push(gone.clone());
    let again = claude_keys(&first.text, &Profile::default(), &have, &prior).expect("plan");
    assert!(
        again.recs.iter().any(|r| r.path == gone.path),
        "{:?}",
        again.recs
    );
}

#[test]
fn keep_mine_keeps_the_prose_and_a_later_apply_leaves_it() {
    let rig = Rig::new("keep-prose").claude();
    let (_, first) = run(
        &rig,
        &Manifest::default(),
        &Profile::default(),
        &Options::default(),
        1,
    );
    let path = rig.rule("notchling-cost.md");
    let edited = format!("{}\n## Mine\nnever force push\n", rig.read(&path));
    rig.write(&path, &edited);
    let (have, probe) = (avail(), Fake::all());
    let kept = {
        let e = env(&rig, &have, &probe, &first.manifest);
        resolve(&e, &first.profile, ID_RULE, Choice::Keep, 2).expect("keep")
    };
    assert!(rig.read(&path).contains("never force push"));
    let saver = Profile::preset("saver");
    let (planned, _) = run(&rig, &kept.manifest, &saver, &Options::default(), 3);
    assert_eq!(planned.get(ID_RULE).map(|a| a.kind), Some(Kind::Skip));
    assert!(rig.read(&path).contains("never force push"));
}

#[test]
fn keep_mine_on_the_codex_block_is_not_overwritten_later() {
    let rig = Rig::new("keep-block").claude().codex();
    let (_, first) = run(
        &rig,
        &Manifest::default(),
        &Profile::default(),
        &Options::default(),
        1,
    );
    let path = rig.roots.codex.join("AGENTS.md");
    let edited = rig.read(&path).replace("Notchling", "My Own");
    assert_ne!(edited, rig.read(&path));
    rig.write(&path, &edited);
    let (have, probe) = (avail(), Fake::all());
    let kept = {
        let e = env(&rig, &have, &probe, &first.manifest);
        resolve(&e, &first.profile, "codex.agents_md", Choice::Keep, 2).expect("keep")
    };
    let saver = Profile::preset("saver");
    run(&rig, &kept.manifest, &saver, &Options::default(), 3);
    assert_eq!(rig.read(&path), edited);
}

#[test]
fn the_first_backup_outlives_any_number_of_later_applies() {
    let rig = Rig::new("many").claude();
    rig.write(&rig.roots.claude.join("plugins/cache/m/pstack/1/x"), "");
    rig.write(&rig.rule("pstack-models.md"), PSTACK_MODELS);
    let opts = Options::consenting(&[CONSENT_PSTACK]);
    let mut manifest = Manifest::default();
    let mut writes = 0;
    for n in 1..=26u64 {
        let profile = if n % 2 == 1 {
            Profile::preset("saver")
        } else {
            Profile::default()
        };
        let (_, done) = run(&rig, &manifest, &profile, &opts, n);
        writes += done
            .results
            .iter()
            .filter(|r| r.id == ID_PSTACK && r.done)
            .count();
        manifest = done.manifest;
    }
    assert!(writes > 22, "{writes}");
    let (results, _) = undo_all(&rig, &manifest, Tool::Claude, 99);
    assert!(
        results.iter().all(|r| r.outcome != Outcome::Failed),
        "{results:?}"
    );
    assert_eq!(rig.read(&rig.rule("pstack-models.md")), PSTACK_MODELS);
}

#[test]
fn nothing_changes_when_the_record_of_it_cannot_be_saved() {
    let rig = Rig::new("journal").claude();
    std::fs::write(rig.data.join("daemon"), "in the way").expect("block");
    let before = rig.read(&rig.settings());
    let (_, done) = run(
        &rig,
        &Manifest::default(),
        &Profile::default(),
        &Options::default(),
        1,
    );
    assert!(done.failed());
    assert_eq!(rig.read(&rig.settings()), before);
    assert!(!rig.rule("notchling-cost.md").exists());
    assert!(!rig.agent("notchling-builder.md").exists());
}

#[test]
fn an_unreadable_manifest_is_not_an_empty_one() {
    let rig = Rig::new("corrupt");
    let path = Manifest::path(&rig.data);
    std::fs::create_dir_all(path.parent().expect("dir")).expect("dir");
    std::fs::write(&path, "{not json").expect("write");
    let loaded = Manifest::load(&rig.data);
    assert!(format!("{loaded:?}").starts_with("Err"), "{loaded:?}");
    std::fs::write(&path, "{\"version\":1,\"entries\":[{\"id\":5}]}").expect("write");
    assert!(format!("{:?}", Manifest::load(&rig.data)).starts_with("Err"));
}

#[cfg(unix)]
#[test]
fn a_linked_settings_file_is_refused_not_followed() {
    let rig = Rig::new("link-settings").claude();
    let real = rig.dir.join("dotfiles-settings.json");
    std::fs::rename(rig.settings(), &real).expect("move");
    std::os::unix::fs::symlink(&real, rig.settings()).expect("link");
    let before = std::fs::read_to_string(&real).expect("read");
    let (planned, done) = run(
        &rig,
        &Manifest::default(),
        &Profile::default(),
        &Options::default(),
        1,
    );
    assert_eq!(planned.get(ID_SETTINGS).map(|a| a.kind), Some(Kind::Skip));
    assert_eq!(std::fs::read_to_string(&real).expect("read"), before);
    assert!(done.results.iter().all(|r| r.id != ID_SETTINGS));
}

#[cfg(unix)]
#[test]
fn a_linked_folder_above_a_file_is_refused_too() {
    let rig = Rig::new("link-folder").claude();
    let outside = rig.dir.join("elsewhere");
    std::fs::create_dir_all(&outside).expect("dir");
    std::fs::write(outside.join("model-choice.md"), MODEL_CHOICE).expect("write");
    std::os::unix::fs::symlink(&outside, rig.roots.claude.join("rules")).expect("link");
    let (_, done) = run(&rig, &Manifest::default(), &Profile::default(), &rules(), 1);
    assert!(
        outside.join("model-choice.md").exists(),
        "{:?}",
        done.results
    );
    assert!(!outside.join("notchling-cost.md").exists());
}

#[test]
fn undo_never_acts_on_a_path_the_manifest_names_outside_notchlings_files() {
    let rig = Rig::new("forged").claude();
    let victim = rig.dir.join("victim.txt");
    std::fs::write(&victim, "precious").expect("write");
    let forged = json!({"version": 1, "entries": [{
        "id": "claude.rule", "tool": Tool::Claude.name(), "path": victim.to_string_lossy(),
        "part": "file", "removed": false, "hash": hash("precious"), "backup": null,
        "existed": false, "at": 1, "keys": []
    }]});
    let manifest = Manifest::from_value(&forged).expect("manifest");
    let (results, left) = undo_all(&rig, &manifest, Tool::Claude, 2);
    assert_eq!(std::fs::read_to_string(&victim).expect("read"), "precious");
    assert!(
        results.iter().all(|r| r.outcome == Outcome::Failed),
        "{results:?}"
    );
    assert_eq!(left.entries.len(), 1);
}

#[test]
fn an_existing_object_and_its_comment_survive_undo() {
    let text =
        "{\n  \"modelSettings\": {\n    \"claude-sonnet-5-5\": {\n      // mine\n    }\n  }\n}\n";
    let have = avail();
    let planned = claude_keys(text, &Profile::default(), &have, &[]).expect("plan");
    assert!(planned.text.contains("autoCompactWindow"));
    let (back, conflicts) = undo_json_keys(&planned.text, &planned.recs).expect("undo");
    assert!(conflicts.is_empty());
    assert!(back.contains("// mine"), "{back}");
    assert!(back.contains("claude-sonnet-5-5"), "{back}");
    assert!(!back.contains("autoCompactWindow") && !back.contains("bashOutputMaxChars"));
}

#[test]
fn a_container_set_up_made_is_removed_again() {
    let planned = claude_keys("{}\n", &Profile::default(), &avail(), &[]).expect("plan");
    let (back, _) = undo_json_keys(&planned.text, &planned.recs).expect("undo");
    assert!(!back.contains("modelSettings"), "{back}");
}

#[test]
fn same_named_pointer_files_in_two_folders_have_two_ids_and_both_undo() {
    let rig = Rig::new("pointers").claude();
    rig.write(&rig.rule("model-choice.md"), MODEL_CHOICE);
    let top = rig.roots.claude.join("CLAUDE.md");
    let nested = rig.agent("CLAUDE.md");
    rig.write(&top, "top: see model-choice.md\n");
    rig.write(&nested, "nested: see model-choice.md\n");
    let (planned, done) = run(&rig, &Manifest::default(), &Profile::default(), &rules(), 1);
    let mut ids: Vec<&str> = planned.actions.iter().map(|a| a.id.as_str()).collect();
    let count = ids.len();
    ids.sort();
    ids.dedup();
    assert_eq!(ids.len(), count, "{ids:?}");
    assert!(!done.failed(), "{:?}", done.results);
    assert!(rig.read(&top).contains("notchling-cost.md"));
    assert!(rig.read(&nested).contains("notchling-cost.md"));
    let (_, left) = undo_all(&rig, &done.manifest, Tool::Claude, 2);
    assert!(left.entries.is_empty());
    assert_eq!(rig.read(&top), "top: see model-choice.md\n");
    assert_eq!(rig.read(&nested), "nested: see model-choice.md\n");
}

#[test]
fn managing_pstack_keeps_comments_unknown_lines_and_their_order() {
    let existing = "# top comment\n# Keep this reviewer for security work\nreview-security: opus max\nfeature, refactoring: haiku low\n# between\nmy own role: sonnet high\nbug-fix: haiku low\n";
    let out = pstack::render(&Profile::default(), &avail(), Some(existing));
    let lines: Vec<&str> = out.lines().collect();
    let at = |needle: &str| {
        lines
            .iter()
            .position(|l| l.contains(needle))
            .unwrap_or(usize::MAX)
    };
    assert!(at("# Keep this reviewer for security work") < at("review-security: opus max"));
    assert!(at("review-security: opus max") < at("feature, refactoring:"));
    assert!(at("feature, refactoring:") < at("# between"));
    assert!(at("# between") < at("my own role: sonnet high"));
    assert!(at("my own role: sonnet high") < at("bug-fix:"));
    assert!(out.contains("feature, refactoring: sonnet high"), "{out}");
    assert!(out.contains("bug-fix: sonnet xhigh"), "{out}");
    assert!(out.contains("hardest tasks:"), "{out}");
}
