//! Replace key runs only from its own click (Swift `MachinesSafetyTests`
//! `testNoPathRunsReplaceKeyWithoutItsOwnClick`): every other way into the engine (the refreshes,
//! the Add sheet's check, a renewed vault value, Set up all and the Deploy key's own Fix) reaches
//! the Deploy key step and never deletes a key on GitHub or makes a new one. Nothing starts ssh or
//! `gh`: a scripted runner answers the batched checks.

use super::catalog;
use super::engine::{Deps, SetupEngine};
use super::engine_plan::{AdminAnswer, PlanKind};
use super::fakes_b::{linux, mac, REPO};
use super::github::NoGitHub;
use super::registry::Machine;
use super::runner::{CommandResult, ScriptedRunner};
use super::step::{MacFacts, NoOpener, StepId};
use super::tests_engine_world::{batch_probes, lock, Secrets, Store, T0};
use std::sync::Arc;

const VALUE: &str = "zZ9-some-vault-value-0123456789";

fn reachable_text(m: &Machine) -> &'static str {
    if m.is_linux() {
        "os=Linux\narch=x86_64\nuid=1000\nuser=agent\nhome=/home/agent\nversion=Ubuntu 24.04\ndisk_kb=83886080\ndocker=none"
    } else {
        "os=Darwin\narch=arm64\nuid=501\nuser=tester\nhome=/Users/tester\nversion=26.1\ndisk_kb=125829120\ndocker=none"
    }
}

/// Answers Reachable with the machine's facts and every other probe and call with an empty
/// success, recording each.
fn reachable_runner(m: &Machine) -> Arc<ScriptedRunner> {
    let runner = ScriptedRunner::new();
    let text = reachable_text(m);
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
                    text
                } else {
                    ""
                };
                out.push_str(&format!("{answer}\n@@{nonce} {i} 0@@\n"));
            }
            CommandResult::ok(&out)
        },
    );
    Arc::new(runner)
}

fn engine(m: &Machine, runner: Arc<ScriptedRunner>) -> SetupEngine {
    let store = Arc::new(Store::default());
    *lock(&store.list) = vec![m.clone()];
    let issues = format!("{}-github-issues", m.name);
    let secrets = Secrets::with(&[("claude-worker", VALUE), (issues.as_str(), VALUE)]);
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
            offers_file: None,
            audit: None,
        },
        catalog::standard(),
    )
}

#[test]
fn no_path_runs_replace_key_without_its_own_click() {
    for m in [mac("m1"), linux("vps-sing-1")] {
        let runner = reachable_runner(&m);
        let e = engine(&m, runner.clone());
        let name = m.name.clone();
        let deploy = StepId::deploy_key(REPO);

        e.refresh(Some(std::slice::from_ref(&name)), true);
        e.refresh(None, true);
        let _ = e.check_draft(&m);
        for secret in ["claude-worker".to_string(), format!("{name}-github-issues")] {
            e.vault_value_changed(&secret);
        }
        let all = e.plan(&name, PlanKind::SetUpAll).expect("steps to set up");
        assert!(
            all.steps.iter().any(|s| s.id == deploy),
            "{name}: Set up all did not list the Deploy key step"
        );
        let confirmed = e.confirm(all.id, AdminAnswer::None).unwrap();
        let _ = e.run_with(confirmed, &|_| AdminAnswer::None);
        if let Ok(fix) = e.plan(&name, PlanKind::Fix(deploy.clone())) {
            let _ = e.run(e.confirm(fix.id, AdminAnswer::None).unwrap());
        }

        assert!(!runner.calls().is_empty(), "the checks did run");
        let results = e.results(&name);
        assert!(
            results.contains_key(&deploy),
            "{name}: the Deploy key step was never evaluated"
        );
        assert!(
            !super::steps_repo::offers_replace(results.get(&deploy)),
            "{name}: Replace key is not offered here"
        );
        for call in runner.calls() {
            let text = call.text();
            assert!(
                !text.contains("deploy-key delete"),
                "{name}: a deploy key was deleted without a click: {text}"
            );
            assert!(
                !text.contains("agents-dtthaison-lingeva-mono.new"),
                "{name}: a new key was made without a click: {text}"
            );
        }
    }
}
