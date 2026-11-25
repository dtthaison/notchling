use super::action::{Kind, Options};
use super::apply::{apply, Applied};
use super::check::{check, resolve, Choice};
use super::manifest::Manifest;
use super::model::{Profile, Role};
use super::plan::{plan, ID_RULE};
use super::tests::*;
use super::undo::{undo, Outcome};
use crate::catalog::Tool;

struct World {
    rig: Rig,
    applied: Applied,
    profile: Profile,
}

fn world(tag: &str) -> World {
    let rig = Rig::new(tag).claude().codex().agy();
    rig.write(&rig.rule("model-choice.md"), MODEL_CHOICE);
    rig.write(&rig.agent("orchestrator.md"), ORCHESTRATOR);
    rig.write(&rig.roots.claude.join("plugins/cache/m/pstack/1/x"), "");
    rig.write(&rig.rule("pstack-models.md"), PSTACK_MODELS);
    rig.write(
        &rig.roots.codex.join("config.toml"),
        "model = \"gpt-6-sol\"\n",
    );
    rig.write(&rig.roots.codex.join("AGENTS.md"), "# mine\n");
    let (have, probe, empty) = (avail(), Fake::all(), Manifest::default());
    let applied = {
        let e = env(&rig, &have, &probe, &empty);
        let planned = plan(&e, &Profile::default(), &everything());
        apply(&e, &planned, 4242)
    };
    let profile = applied.profile.clone();
    World {
        rig,
        applied,
        profile,
    }
}

fn snapshot(rig: &Rig) -> Vec<(String, String)> {
    fn walk(dir: &std::path::Path, out: &mut Vec<(String, String)>) {
        let Ok(read) = std::fs::read_dir(dir) else {
            return;
        };
        for e in read.filter_map(|e| e.ok()) {
            let p = e.path();
            if p.is_dir() {
                walk(&p, out);
            } else if !p.to_string_lossy().contains("notchling-setup")
                && !p.to_string_lossy().contains("/data/")
            {
                out.push((
                    p.to_string_lossy().into_owned(),
                    std::fs::read_to_string(&p).unwrap_or_default(),
                ));
            }
        }
    }
    let mut out = Vec::new();
    walk(&rig.dir, &mut out);
    out.sort();
    out
}

#[test]
fn undo_puts_every_tool_back_byte_for_byte() {
    let rig = Rig::new("undo-all").claude().codex().agy();
    rig.write(&rig.rule("model-choice.md"), MODEL_CHOICE);
    rig.write(&rig.agent("orchestrator.md"), ORCHESTRATOR);
    rig.write(&rig.roots.claude.join("plugins/cache/m/pstack/1/x"), "");
    rig.write(&rig.rule("pstack-models.md"), PSTACK_MODELS);
    rig.write(
        &rig.roots.codex.join("config.toml"),
        "model = \"gpt-6-sol\"\n",
    );
    rig.write(&rig.roots.codex.join("AGENTS.md"), "# mine\n");
    let original = snapshot(&rig);
    let (have, probe, empty) = (avail(), Fake::all(), Manifest::default());
    let applied = {
        let e = env(&rig, &have, &probe, &empty);
        apply(&e, &plan(&e, &Profile::default(), &everything()), 77)
    };
    assert!(!applied.failed(), "{:?}", applied.results);
    assert_ne!(snapshot(&rig), original);
    assert!(!rig.rule("model-choice.md").exists());
    let mut manifest = applied.manifest.clone();
    for tool in [Tool::Antigravity, Tool::Codex, Tool::Claude] {
        let (results, next) = {
            let e = env(&rig, &have, &probe, &manifest);
            undo(&e, tool, 78)
        };
        assert!(
            results
                .iter()
                .all(|r| matches!(r.outcome, Outcome::Restored | Outcome::Removed)),
            "{tool:?} {results:?}"
        );
        manifest = next;
    }
    assert!(manifest.entries.is_empty());
    let after = snapshot(&rig);
    let strip = |v: &[(String, String)]| -> Vec<(String, String)> {
        v.iter()
            .map(|(p, t)| {
                let json = p.ends_with("settings.json");
                let t = if json {
                    serde_json::from_str::<serde_json::Value>(t)
                        .map(|v| v.to_string())
                        .unwrap_or_default()
                } else {
                    t.clone()
                };
                (p.clone(), t)
            })
            .collect()
    };
    assert_eq!(strip(&after), strip(&original));
}

#[test]
fn undo_is_per_tool() {
    let w = world("undo-tool");
    let (have, probe) = (avail(), Fake::all());
    let e = env(&w.rig, &have, &probe, &w.applied.manifest);
    let (results, next) = undo(&e, Tool::Codex, 5);
    assert!(results.iter().all(|r| r.outcome != Outcome::Conflict));
    assert!(next.of_tool(Tool::Codex).is_empty());
    assert!(!next.of_tool(Tool::Claude).is_empty());
    assert!(w.rig.rule("notchling-cost.md").exists());
    assert_eq!(w.rig.read(&w.rig.roots.codex.join("AGENTS.md")), "# mine\n");
    assert_eq!(
        w.rig.read(&w.rig.roots.codex.join("config.toml")),
        "model = \"gpt-6-sol\"\n"
    );
    let _ = &w.profile;
}

#[test]
fn undo_never_overwrites_a_file_changed_since_apply() {
    let w = world("undo-conflict");
    let path = w.rig.rule("notchling-cost.md");
    let edited = format!("{}\n- my habit\n", w.rig.read(&path));
    w.rig.write(&path, &edited);
    let (have, probe) = (avail(), Fake::all());
    let e = env(&w.rig, &have, &probe, &w.applied.manifest);
    let (results, next) = undo(&e, Tool::Claude, 6);
    let hit = results.iter().find(|r| r.path == path).expect("rule row");
    assert_eq!(hit.outcome, Outcome::Conflict);
    assert_eq!(w.rig.read(&path), edited);
    assert!(next.get("claude.rule").is_some());
    assert!(next.get("claude.agent.builder").is_none());
    assert!(!w.rig.agent("notchling-builder.md").exists());
}

#[test]
fn a_user_file_that_came_back_is_a_conflict_and_a_missing_one_is_restored() {
    let w = world("undo-returned");
    let own = w.rig.rule("model-choice.md");
    w.rig.write(&own, "I wrote this again\n");
    let (have, probe) = (avail(), Fake::all());
    let e = env(&w.rig, &have, &probe, &w.applied.manifest);
    let (results, _) = undo(&e, Tool::Claude, 7);
    let hit = results.iter().find(|r| r.path == own).expect("row");
    assert_eq!(hit.outcome, Outcome::Conflict);
    assert_eq!(w.rig.read(&own), "I wrote this again\n");
}

#[test]
fn only_the_key_that_changed_is_a_conflict() {
    let w = world("undo-keys");
    let settings = w.rig.settings();
    let text = w.rig.read(&settings).replace("12000", "13000");
    w.rig.write(&settings, &text);
    let (have, probe) = (avail(), Fake::all());
    let e = env(&w.rig, &have, &probe, &w.applied.manifest);
    let (results, next) = undo(&e, Tool::Claude, 8);
    let hit = results.iter().find(|r| r.path == settings).expect("row");
    assert_eq!(hit.outcome, Outcome::Conflict);
    assert_eq!(hit.detail.as_deref(), Some("bashOutputMaxChars"));
    let now = w.rig.read(&settings);
    assert!(now.contains("13000"));
    assert!(
        now.contains("\"autoCompactWindow\": 400000")
            || now.contains("\"autoCompactWindow\":400000")
    );
    assert_eq!(next.get("claude.settings").map(|e| e.keys.len()), Some(1));
}

#[test]
fn the_codex_block_goes_and_the_users_own_text_stays() {
    let w = world("undo-block");
    let path = w.rig.roots.codex.join("AGENTS.md");
    assert!(w.rig.read(&path).starts_with("# mine\n"));
    assert!(w.rig.read(&path).contains("notchling:notchling-cost"));
    let (have, probe) = (avail(), Fake::all());
    let e = env(&w.rig, &have, &probe, &w.applied.manifest);
    let (_, _) = undo(&e, Tool::Codex, 9);
    assert_eq!(w.rig.read(&path), "# mine\n");
}

#[test]
fn a_codex_file_notchling_made_is_removed_by_undo() {
    let rig = Rig::new("undo-created").codex();
    let (have, probe, empty) = (avail(), Fake::all(), Manifest::default());
    let applied = {
        let e = env(&rig, &have, &probe, &empty);
        apply(&e, &plan(&e, &Profile::default(), &Options::default()), 3)
    };
    assert!(rig.roots.codex.join("AGENTS.md").exists());
    let e = env(&rig, &have, &probe, &applied.manifest);
    let (results, _) = undo(&e, Tool::Codex, 4);
    assert_eq!(results[0].outcome, Outcome::Removed);
    assert!(!rig.roots.codex.join("AGENTS.md").exists());
}

#[test]
fn check_reports_each_kind_of_drift_and_keep_mine_imports_the_edit() {
    let w = world("check");
    let (have, probe) = (avail(), Fake::all());
    let rows = |m: &Manifest| check(&env(&w.rig, &have, &probe, m), &w.profile);
    let all = rows(&w.applied.manifest);
    assert!(all.iter().all(|r| r.state == "in_step"), "{all:?}");
    let path = w.rig.rule("notchling-cost.md");
    let text = w.rig.read(&path).replace("| code | builders, lanes, UI tasks, tests, refactors, production runs | notchling-builder | sonnet | high |", "| code | builders, lanes, UI tasks, tests, refactors, production runs | notchling-builder | opus | medium |");
    assert!(text.contains("opus | medium"));
    w.rig.write(&path, &text);
    std::fs::remove_file(w.rig.agent("notchling-reader.md")).expect("remove");
    let settings = w.rig.settings();
    w.rig
        .write(&settings, &w.rig.read(&settings).replace("12000", "9000"));
    let all = rows(&w.applied.manifest);
    let state = |id: &str| {
        all.iter()
            .find(|r| r.id == id)
            .map(|r| r.state)
            .unwrap_or("none")
    };
    assert_eq!(state("claude.rule"), "edited");
    assert_eq!(state("claude.agent.reader"), "missing");
    assert_eq!(state("claude.settings"), "changed_by_tool");
    assert_eq!(state("claude.agent.builder"), "in_step");
    let e = env(&w.rig, &have, &probe, &w.applied.manifest);
    let kept = resolve(&e, &w.profile, "claude.rule", Choice::Keep, 10).expect("keep");
    assert_eq!(
        kept.profile
            .cell(Role::Code, Tool::Claude)
            .map(|c| c.to_string())
            .as_deref(),
        Some("opus medium")
    );
    assert!(kept.profile.is_custom());
    assert!(w.rig.read(&path).contains("opus | medium"));
    let after = check(&env(&w.rig, &have, &probe, &kept.manifest), &kept.profile);
    assert_eq!(
        after
            .iter()
            .find(|r| r.id == "claude.rule")
            .map(|r| (r.state, r.detail.as_deref())),
        Some(("skipped", Some("kept")))
    );
}

#[test]
fn restore_rewrites_the_file_from_the_profile() {
    let w = world("restore");
    let path = w.rig.rule("notchling-cost.md");
    let good = w.rig.read(&path);
    w.rig.write(&path, "garbage\n");
    let (have, probe) = (avail(), Fake::all());
    let e = env(&w.rig, &have, &probe, &w.applied.manifest);
    let out = resolve(&e, &w.profile, ID_RULE, Choice::Restore, 11).expect("restore");
    assert!(out.applied.as_ref().is_some_and(|a| !a.failed()));
    assert_eq!(w.rig.read(&path), good);
    let others: Vec<_> = out
        .applied
        .expect("applied")
        .results
        .iter()
        .map(|r| r.id.clone())
        .collect();
    assert_eq!(others, [ID_RULE]);
    assert!(resolve(&e, &w.profile, "nope", Choice::Keep, 1).is_err());
    let _ = Kind::Create;
}

#[test]
fn a_reapply_after_a_profile_change_keeps_the_first_backup_so_undo_reaches_the_original() {
    let rig = Rig::new("reapply").claude();
    let original = rig.read(&rig.settings());
    let (have, probe, empty) = (avail(), Fake::all(), Manifest::default());
    let first = {
        let e = env(&rig, &have, &probe, &empty);
        apply(&e, &plan(&e, &Profile::default(), &Options::default()), 1)
    };
    let saver = Profile::preset("saver");
    let second = {
        let e = env(&rig, &have, &probe, &first.manifest);
        apply(&e, &plan(&e, &saver, &Options::default()), 2)
    };
    assert!(!second.failed(), "{:?}", second.results);
    assert!(rig.read(&rig.rule("notchling-cost.md")).contains("(Saver)"));
    assert!(rig.read(&rig.settings()).contains("8000"));
    let e = env(&rig, &have, &probe, &second.manifest);
    let (results, _) = undo(&e, Tool::Claude, 3);
    assert!(
        results
            .iter()
            .all(|r| matches!(r.outcome, Outcome::Restored | Outcome::Removed)),
        "{results:?}"
    );
    let back: serde_json::Value = serde_json::from_str(&rig.read(&rig.settings())).expect("json");
    let was: serde_json::Value = serde_json::from_str(&original).expect("json");
    assert_eq!(back, was);
    assert!(!rig.rule("notchling-cost.md").exists());
}
