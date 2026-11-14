use super::catalog;
use super::engine::{Deps, SetupEngine};
use super::engine_plan::PlanKind;
use super::fakes::{Ctx, TestMachines};
use super::github::NoGitHub;
use super::masks::Masks;
use super::registry::Machine;
use super::runner::{CommandResult, ScriptedRunner};
use super::step::{CheckResult, CheckState, Probe, RemoteOutput, SetupStep, StepFix, StepId};
use super::step::{MacFacts, NoOpener};
use super::steps_versions::TOOL_VERSIONS_SCRIPT;
use super::tests_engine_world::{batch_probes, lock, sid, Secrets, Setup, Store, T0};
use crate::versions::{Method, Tool, ToolState};
use std::sync::Arc;

const RECORDED: &str = "claude.path=/home/w/.local/bin/claude\nclaude.link=/home/w/.local/share/claude/versions/2.1.324\ncodex.path=/usr/local/bin/codex\ncodex.link=../lib/node_modules/@openai/codex/bin/codex.js\nagy.path=\nnotchling.path=\nclaude.version=2.1.324 (Claude Code)\ncodex.version=codex-cli 0.166.0\n";

fn step(machine: &Machine) -> SetupStep {
    catalog::steps(machine, std::slice::from_ref(machine))
        .into_iter()
        .find(|s| s.id == StepId::TOOL_VERSIONS)
        .expect("the Tool Versions step")
}

fn parse(text: &str, ctx: &Ctx) -> CheckResult {
    let s = step(&ctx.machine);
    let Probe::Remote { parse, .. } = &s.probe else {
        panic!("no remote probe");
    };
    parse(
        &RemoteOutput {
            text: text.into(),
            code: 0,
        },
        &ctx.step(),
    )
}

fn tools_of(result: &CheckResult) -> crate::versions::WorkerTools {
    result.detected.tools.clone().expect("detected tools")
}

#[test]
fn probe_reads_versions_and_methods() {
    let result = parse(RECORDED, &Ctx::mac());
    assert_eq!(result.state, CheckState::Pass("2 tools found".into()));
    let tools = tools_of(&result);
    assert_eq!(tools.at_ms, 1_800_000_000_000);
    match &tools.tools[&Tool::Claude] {
        ToolState::Installed {
            version, method, ..
        } => {
            assert_eq!(version.as_ref().map(|v| v.as_str()), Some("2.1.324"));
            assert_eq!(*method, Method::Native);
        }
        other => panic!("{other:?}"),
    }
    match &tools.tools[&Tool::Codex] {
        ToolState::Installed {
            version, method, ..
        } => {
            assert_eq!(version.as_ref().map(|v| v.as_str()), Some("0.166.0"));
            assert_eq!(*method, Method::Npm);
        }
        other => panic!("{other:?}"),
    }
    let rows: Vec<(String, bool, String)> = result
        .rows
        .iter()
        .map(|r| (r.name.clone(), r.ok, r.detail.clone()))
        .collect();
    assert_eq!(rows.len(), 4);
    assert_eq!(
        rows[0],
        (
            "Claude Code".into(),
            true,
            "2.1.324 · native installer".into()
        )
    );
    assert_eq!(rows[1], ("Codex".into(), true, "0.166.0 · npm".into()));
    assert_eq!(
        rows[2],
        ("Antigravity".into(), false, "Not installed".into())
    );
}

#[test]
fn absent_tools_show_not_installed() {
    let none = "claude.path=\ncodex.path=\nagy.path=\nnotchling.path=\n";
    let result = parse(none, &Ctx::mac());
    assert_eq!(
        result.state,
        CheckState::Pass("No agent tools found".into())
    );
    assert!(result
        .rows
        .iter()
        .all(|r| !r.ok && r.detail == "Not installed"));
    assert!(tools_of(&result)
        .tools
        .values()
        .all(|s| *s == ToolState::NotInstalled));
    let nothing = parse("", &Ctx::mac());
    assert!(matches!(nothing.state, CheckState::CantTell(_)));
    assert!(nothing.detected.tools.is_none());
}

#[test]
fn an_agy_with_no_version_shows_its_path_and_method() {
    let text = "agy.path=/home/w/.local/bin/agy\nagy.link=\n";
    let result = parse(text, &Ctx::mac());
    assert_eq!(
        result.rows[2].detail,
        "version unknown · standalone installer"
    );
}

#[test]
fn hostile_probe_output_keeps_nothing() {
    let mut text =
        String::from("claude.path=/home/w/.local/bin/claude\nclaude.version=2.1.3;id\ncodex.path=/tmp/../etc/codex\ncodex.version=codex-cli 0.1.0\n");
    for i in 0..200 {
        text.push_str(&format!("noise{i}=x\n"));
    }
    text.push_str("agy.path=/home/w/.local/bin/agy\n");
    let result = parse(&text, &Ctx::mac());
    let tools = tools_of(&result);
    match &tools.tools[&Tool::Claude] {
        ToolState::Installed { version, .. } => assert!(version.is_none()),
        other => panic!("{other:?}"),
    }
    match &tools.tools[&Tool::Codex] {
        ToolState::Installed { path, method, .. } => {
            assert_eq!(path, "");
            assert_eq!(*method, Method::Unknown);
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(tools.tools[&Tool::Agy], ToolState::NotInstalled);
}

#[test]
fn masked_output_never_shows_a_vault_value() {
    let secret = "ghp_vaultvalue1234567890";
    let mut ctx = Ctx::mac();
    ctx.masks = Masks::new();
    ctx.masks.hide("vault", secret);
    let text = format!(
        "claude.path=/home/w/.local/bin/claude\nclaude.version={secret}\ncodex.path=/home/w/{secret}/codex\n"
    );
    let result = parse(&text, &ctx);
    let masked = result.masked(&ctx.masks);
    let shown = format!("{:?}", masked.rows) + masked.state.text();
    assert!(!shown.contains(secret), "{shown}");
    let tools = serde_json::to_string(&tools_of(&masked)).unwrap();
    assert!(!tools.contains(secret), "{tools}");
}

#[test]
fn tool_versions_has_no_fix_and_is_not_in_set_up_all() {
    let m = TestMachines::mac("m1");
    let s = step(&m);
    assert!(matches!(s.fix, StepFix::HowTo(_)));
    assert!(s.plan().is_none());
    let world = Setup::new(catalog::steps).build();
    assert!(world
        .engine
        .plan("m1", PlanKind::Fix(sid("tool-versions")))
        .is_err());
}

#[test]
fn script_never_runs_agy_version_or_an_installer() {
    let script = TOOL_VERSIONS_SCRIPT;
    for word in [
        "agy --version",
        "install",
        "curl",
        "npm",
        "brew",
        "wget",
        "sudo",
        "rm ",
        "mv ",
    ] {
        assert!(!script.contains(word), "{word}");
    }
    assert!(!script.replace("2>/dev/null", "").contains('>'));
    assert!(script.contains("readlink"));
    let versions = script.lines().filter(|l| l.contains("--version")).count();
    assert_eq!(versions, 1);
    assert!(script.contains("for t in claude codex notchling; do"));
}

#[test]
fn it_sits_right_after_claude_login() {
    let m = TestMachines::mac("m1");
    let ids: Vec<String> = catalog::steps(&m, std::slice::from_ref(&m))
        .iter()
        .map(|s| s.id.raw().to_string())
        .collect();
    let at = ids.iter().position(|i| i == "claude-login").unwrap();
    assert_eq!(ids[at + 1], "tool-versions");
}

pub(super) fn engine_with(m: &Machine, data: Option<std::path::PathBuf>) -> SetupEngine {
    let runner = ScriptedRunner::new();
    runner.on_call(
        |argv| {
            argv.last()
                .is_some_and(|s| s.contains("out=$( /bin/sh -c "))
        },
        move |call| {
            let script = call.argv.last().cloned().unwrap_or_default();
            let Some((nonce, probes)) = batch_probes(&script) else {
                return CommandResult::ok("");
            };
            let mut out = String::new();
            for (i, p) in probes.iter().enumerate() {
                let answer = if p.contains("os=$(uname -s)") {
                    "os=Darwin\narch=arm64\nuid=501\nuser=tester\nhome=/Users/tester\nversion=26.1\ndisk_kb=125829120\ndocker=none"
                } else if p.contains("readlink") {
                    RECORDED
                } else {
                    ""
                };
                out.push_str(&format!("{answer}\n@@{nonce} {i} 0@@\n"));
            }
            CommandResult::ok(&out)
        },
    );
    let store = Arc::new(Store::default());
    *lock(&store.list) = vec![m.clone()];
    SetupEngine::new(
        Deps {
            runner: Arc::new(runner),
            secrets: Arc::new(Secrets::default()),
            http: Arc::new(NoGitHub),
            opener: Arc::new(NoOpener),
            store,
            mac: Arc::new(MacFacts::default),
            clock: Arc::new(|| T0),
            scratch: std::env::temp_dir(),
            offers_data: data,
            audit: None,
        },
        catalog::standard(),
    )
}

#[test]
fn a_refresh_keeps_the_worker_tools_in_its_result() {
    let m = TestMachines::mac("m1");
    let e = engine_with(&m, None);
    e.refresh(Some(&["m1".to_string()]), false);
    let results = e.results("m1");
    let result = results.get(&StepId::TOOL_VERSIONS).expect("a result");
    assert_eq!(result.state, CheckState::Pass("2 tools found".into()));
    assert!(result.detected.tools.is_some());
}
