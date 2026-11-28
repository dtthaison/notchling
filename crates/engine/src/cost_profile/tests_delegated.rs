use super::action::{Kind, Options};
use super::manifest::Manifest;
use super::model::{parse_picks, Cell, Profile, Role};
use super::plan::{plan, ID_BUILDER, ID_RULE};
use super::render::{claude_rule, Features};
use super::service::Service;
use super::tests::{avail, env, Fake, Rig};
use crate::catalog::{Catalog, Tool};
use crate::install::ToolRoots;
use std::sync::Arc;

fn with_picks(spec: &str) -> Profile {
    Profile {
        picks: parse_picks(spec),
        ..Profile::default()
    }
}

fn service(rig: &Rig) -> Service {
    Service {
        roots: ToolRoots {
            claude: rig.roots.claude.clone(),
            codex: rig.roots.codex.clone(),
            antigravity: rig.roots.antigravity.clone(),
        },
        data_dir: rig.data.clone(),
        catalog: Arc::new(Catalog::built_in()),
        live: Arc::new(Vec::new),
        probe: Arc::new(Fake::all()),
        codex_snapshot: None,
    }
}

#[test]
fn without_delegated_picks_the_claude_rule_is_unchanged() {
    let have = avail();
    let base = claude_rule(&Profile::default(), &have, Features::default());
    assert!(base.contains("| code | builders, lanes, UI tasks, tests, refactors, production runs | notchling-builder | sonnet | high |"));
    assert!(base.contains("| fix | a bug fix | notchling-builder | sonnet | xhigh |"));
    assert!(!base.contains("notchling delegate agy"));
    assert!(!base.contains("notchling delegate codex"));
    assert!(!base.contains("not a Claude subagent"));
    for preset in ["saver", "balanced", "quality"] {
        let text = claude_rule(&Profile::preset(preset), &have, Features::default());
        assert!(!text.contains("not a Claude subagent"), "{preset}");
    }
    let claude_only = with_picks("code.claude=opus high; fix.claude=opus xhigh");
    let text = claude_rule(&claude_only, &have, Features::default());
    assert!(text.contains("| code | builders, lanes, UI tasks, tests, refactors, production runs | notchling-builder | opus | high |"));
    assert!(!text.contains("not a Claude subagent"));
}

#[test]
fn code_and_fix_agy_picks_route_both_rows_and_keep_the_claude_builder_fallback() {
    let rig = Rig::new("del-agy").claude();
    let profile = with_picks("code.agy=gemini-4-carbon high; fix.agy=gemini-4-carbon high");
    let (have, probe, manifest) = (avail(), Fake::all(), Manifest::default());
    let planned = plan(
        &env(&rig, &have, &probe, &manifest),
        &profile,
        &Options::default(),
    );
    let text = planned
        .get(ID_RULE)
        .and_then(|a| a.after.clone())
        .expect("rule");
    assert!(text.contains("| code | builders, lanes, UI tasks, tests, refactors, production runs | Antigravity (`notchling delegate agy`) | gemini-4-carbon | high |"));
    assert!(text.contains(
        "| fix | a bug fix | Antigravity (`notchling delegate agy`) | gemini-4-carbon | high |"
    ));
    assert!(text.contains("- Code and fixes go to Antigravity, not a Claude subagent: `/Applications/Notchling.app/Contents/MacOS/notchling delegate agy --model gemini-4-carbon --effort high --write --folder <worktree> --timeout 1400 -- \"<brief>\"`, run in the background. Antigravity can't run commands: the brief says to use only file tools, and you build, test and commit yourself. Use a notchling-builder (sonnet) only when Antigravity fails or is out of quota, and say so.\n"));
    let builder = planned.get(ID_BUILDER).expect("builder");
    assert_eq!(builder.kind, Kind::Create);
    let body = builder.after.as_deref().unwrap_or_default();
    assert!(body.contains("name: notchling-builder"));
    assert!(body.contains("model: sonnet\neffort: high"));
}

#[test]
fn code_codex_pick_routes_code_to_codex_and_leaves_fix_on_claude() {
    let have = avail();
    let profile = with_picks("code.codex=gpt-6-sol high");
    let text = claude_rule(&profile, &have, Features::default());
    assert!(text.contains("| code | builders, lanes, UI tasks, tests, refactors, production runs | Codex (`notchling delegate codex`) | gpt-6-sol | high |"));
    assert!(text.contains("| fix | a bug fix | notchling-builder | sonnet | xhigh |"));
    assert!(text.contains("- Code goes to Codex, not a Claude subagent: `/Applications/Notchling.app/Contents/MacOS/notchling delegate codex --model gpt-6-sol --effort high --write --folder <worktree> --timeout 1400 -- \"<brief>\"`, run in the background. Use a notchling-builder (sonnet) only when Codex fails or is out of quota, and say so.\n"));
    assert!(!text.contains("Antigravity can't run commands"));
}

#[test]
fn different_tools_or_models_for_code_and_fix_name_each_in_the_rule() {
    let have = avail();
    let two_models = with_picks("code.agy=gemini-4-hydrogen medium; fix.agy=gemini-4-carbon high");
    let text = claude_rule(&two_models, &have, Features::default());
    assert!(text.contains(
        "Code (gemini-4-hydrogen medium) and fixes (gemini-4-carbon high) go to Antigravity"
    ));
    assert!(text.contains("--model gemini-4-hydrogen --effort medium"));
    assert!(text.contains("--model gemini-4-carbon --effort high"));
    let mut two_tools = with_picks("code.agy=gemini-4-carbon high; fix.codex=gpt-6-astra high");
    two_tools
        .picks
        .insert((Role::Code, Tool::Claude), Cell::new("opus", "high"));
    let text = claude_rule(&two_tools, &have, Features::default());
    assert!(text.contains("| code | builders, lanes, UI tasks, tests, refactors, production runs | Antigravity (`notchling delegate agy`) | gemini-4-carbon | high |"));
    assert!(text
        .contains("| fix | a bug fix | Codex (`notchling delegate codex`) | gpt-6-astra | high |"));
    assert!(text.contains("Code goes to Antigravity (`/Applications/Notchling.app/Contents/MacOS/notchling delegate agy --model gemini-4-carbon --effort high"));
    assert!(text.contains("and fixes go to Codex (`/Applications/Notchling.app/Contents/MacOS/notchling delegate codex --model gpt-6-astra --effort high"));
    assert!(text.contains("Antigravity can't run commands"));
    assert!(text.contains("Use a notchling-builder (opus) only when Antigravity or Codex fails or is out of quota, and say so."));
}

#[test]
fn check_marks_the_claude_rule_outdated_after_a_delegated_pick_until_setup_applies() {
    let rig = Rig::new("del-check").claude();
    let svc = service(&rig);
    let mut initial = Profile::default();
    initial.habit_edits.insert("parallelCap".into(), 4);
    svc.save_profile(&initial).expect("save");
    let (_, applied, err) = svc.apply(&Options::default(), 1).expect("apply");
    assert!(!applied.failed() && err.is_none());
    let before = svc.check().expect("check");
    assert!(before.iter().all(|r| r.state == "in_step"), "{before:?}");
    let mut updated = svc.profile();
    let cell = Cell::new("gemini-4-carbon", "high");
    updated
        .picks
        .insert((Role::Code, Tool::Antigravity), cell.clone());
    updated.picks.insert((Role::Fix, Tool::Antigravity), cell);
    svc.save_profile(&updated).expect("save");
    let drifted = svc.check().expect("check");
    let rule = drifted.iter().find(|r| r.id == ID_RULE).expect("rule row");
    assert_eq!(rule.state, "outdated");
    let (_, applied, err) = svc.apply(&Options::default(), 2).expect("apply");
    assert!(!applied.failed() && err.is_none());
    let after = svc.check().expect("check");
    assert!(after.iter().all(|r| r.state == "in_step"), "{after:?}");
    let written = rig.read(&rig.rule("notchling-cost.md"));
    assert!(written.contains("Antigravity (`notchling delegate agy`)"));
}
