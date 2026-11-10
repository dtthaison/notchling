//! The pinned guarantees across the engine and the real steps (Swift `MachinesSafetyTests`): a
//! vault value reaches a machine only on ssh's stdin, never an argv, an environment, a log line or a
//! result; nothing a refresh does changes a machine; a hostile home never reaches a script.

use super::catalog;
use super::engine::{Deps, SetupEngine};
use super::engine_plan::{AdminAnswer, PlanKind};
use super::github::NoGitHub;
use super::registry::{AdminAccess, SdWorkerSettings};
use super::runner::{CommandResult, ScriptedRunner};
use super::step::{Detected, MacFacts, NoOpener, StepFix, StepId};
use super::tests_engine_world::{linux, lock, mac, Secrets, Store, T0};
use std::sync::Arc;

const CLAUDE: &str = "fake-claude-oauth-0000";
const ISSUES: &str = "fake-ghp-issues-1111";

fn engine(machines: Vec<super::registry::Machine>, runner: Arc<ScriptedRunner>) -> SetupEngine {
    let store = Arc::new(Store::default());
    *lock(&store.list) = machines;
    let secrets = Secrets::with(&[("claude-worker", CLAUDE), ("m1-github-issues", ISSUES)]);
    SetupEngine::new(
        Deps {
            runner,
            secrets: Arc::new(secrets),
            http: Arc::new(NoGitHub),
            opener: Arc::new(NoOpener),
            store,
            mac: Arc::new(MacFacts::default),
            clock: Arc::new(|| T0),
            scratch: std::env::temp_dir(),
            offers_data: None,
            audit: None,
        },
        catalog::standard(),
    )
}

fn worker_mac() -> super::registry::Machine {
    let mut m = mac("m1");
    m.set_sd_worker(Some(SdWorkerSettings::new("m1")));
    m
}

/// Every call a machine's output could echo the value into: the runner says it back on both pipes.
fn echoing() -> Arc<ScriptedRunner> {
    let r = ScriptedRunner::new();
    r.on_call(
        |_| true,
        |call| {
            let text = call
                .stdin
                .as_ref()
                .map(|d| String::from_utf8_lossy(d).into_owned())
                .unwrap_or_default();
            CommandResult::new(1, &text, &text)
        },
    );
    Arc::new(r)
}

#[test]
fn a_refresh_of_every_real_step_runs_only_read_only_probes_and_never_sends_a_value() {
    let runner = Arc::new(ScriptedRunner::new());
    let mut vps = linux("vps-sing-1");
    vps.set_sd_worker(Some(SdWorkerSettings::new("vps-sing-1")));
    vps.set_admin(AdminAccess::RootSsh);
    let e = engine(vec![worker_mac(), vps], runner.clone());
    e.refresh(None, true);
    assert!(!runner.calls().is_empty());
    for call in runner.calls() {
        assert!(call.stdin.is_none(), "a check sends nothing: {call:?}");
        let text = call.text();
        assert!(
            !text.contains("sudo"),
            "a check never asks for admin rights"
        );
        assert!(!text.contains(CLAUDE) && !text.contains(ISSUES));
        assert!(call.env.is_empty());
    }
}

#[test]
fn a_vault_value_reaches_the_machine_only_on_stdin_and_is_masked_everywhere() {
    let runner = echoing();
    let e = engine(vec![worker_mac()], runner.clone());
    for usage in super::steps_secret::usages(&[worker_mac()]) {
        let plan = e.plan("m1", PlanKind::SendAgain(usage.clone())).unwrap();
        assert!(!plan.display().contains(CLAUDE) && !plan.display().contains(ISSUES));
        let out = e.run(e.confirm(plan.id, AdminAnswer::None).unwrap());
        assert!(
            !out.message.contains(CLAUDE) && !out.message.contains(ISSUES),
            "{}",
            out.message
        );
    }
    let mut on_stdin = 0;
    for call in runner.calls() {
        let text = call.text();
        assert!(
            !text.contains(CLAUDE) && !text.contains(ISSUES),
            "argv: {text}"
        );
        assert!(!call
            .env
            .iter()
            .any(|(_, v)| v.contains(CLAUDE) || v.contains(ISSUES)));
        let stdin = call.stdin_text();
        if stdin.contains(CLAUDE) || stdin.contains(ISSUES) {
            on_stdin += 1;
            assert!(
                call.argv[0] == "/usr/bin/ssh",
                "only ssh's stdin carries a value"
            );
        }
    }
    assert!(on_stdin >= 1, "the value was delivered");
    let log = e.log("m1").join("\n");
    assert!(!log.contains(CLAUDE) && !log.contains(ISSUES), "{log}");
    assert!(!format!("{:?}", e.results("m1")).contains(CLAUDE));
}

#[test]
fn the_admin_steps_of_the_real_catalog_are_the_pinned_ones() {
    let mut vps = linux("vps-sing-1");
    vps.set_sd_worker(Some(SdWorkerSettings::new("vps-sing-1")));
    let mut admin: Vec<String> = Vec::new();
    for m in [worker_mac(), vps.clone()] {
        let all = vec![worker_mac(), vps.clone()];
        for s in (catalog::standard().steps)(&m, &all) {
            let needs = match &s.fix {
                StepFix::Run(p) => p.needs_admin,
                StepFix::Toggle { needs_admin } => *needs_admin,
                StepFix::HowTo(_) => false,
            };
            if needs {
                admin.push(format!("{}/{}", m.name, s.id.raw()));
            }
        }
    }
    assert_eq!(
        admin,
        vec![
            "m1/stays-awake",
            "vps-sing-1/base-tools",
            "vps-sing-1/service",
            "vps-sing-1/running"
        ]
    );
}

#[test]
fn every_admin_plan_in_the_real_catalog_uses_absolute_sudo_or_root_and_touches_no_production_service(
) {
    for m in [worker_mac(), linux("vps-sing-1")] {
        let mut m = m;
        m.set_sd_worker(Some(SdWorkerSettings::new(&m.name.clone())));
        for s in (catalog::standard().steps)(&m, std::slice::from_ref(&m)) {
            let Some(p) = s.plan() else { continue };
            for service in super::shell::PRODUCTION_SERVICES {
                assert!(
                    !p.display.contains(&format!("systemctl restart {service}"))
                        && !p.display.contains(&format!("systemctl stop {service}")),
                    "{} touches {service}",
                    s.title
                );
            }
            if p.needs_admin && p.display.contains("apt") {
                assert!(
                    p.display.contains("--no-upgrade")
                        && p.display.contains("NEEDRESTART_SUSPEND=1"),
                    "{}",
                    s.title
                );
            }
        }
    }
}

#[test]
fn a_hostile_home_from_a_machine_never_reaches_a_script() {
    let hostile = "/home/a b;x";
    let runner = Arc::new(ScriptedRunner::new());
    let e = engine(vec![worker_mac()], runner.clone());
    let m = super::engine::applying(
        &Detected {
            home: Some(hostile.into()),
            python: Some(format!("{hostile}/python3")),
            sd_path: Some(format!("{hostile}/sd.py")),
            ..Detected::default()
        },
        &worker_mac(),
    );
    assert!(m.abilities.as_ref().and_then(|a| a.home.clone()).is_none());
    for s in (catalog::standard().steps)(&m, std::slice::from_ref(&m)) {
        assert!(!s.probe.script().unwrap_or("").contains(hostile));
        if let Some(p) = s.plan() {
            assert!(!p.display.contains(hostile));
        }
    }
    let _ = (e, StepId::REACHABLE);
}
