//! No vault value in anything the window shows or holds (Swift `MachinesSafetyTests`
//! `testNoDisplayTextOrScriptContainsAVaultValue`), over the real step catalog of a Mac and a Linux
//! machine: every Set up sheet, Set up all, the Running switch and Update N machines, with the two
//! delivery Fixes run for real against a scripted runner. A value reaches only the stdin of the
//! call that writes its file; no argv, environment, sheet, log, line, result or model field holds
//! it or an 8-character piece of it.

use std::collections::BTreeMap;
use std::sync::Arc;

use notchling_engine::accounts::Secret;
use notchling_engine::machines::registry::{AdminAccess, Machine, MachineOs};
use notchling_engine::machines::runner::{CommandResult, ScriptedRunner};
use notchling_engine::machines::step::{SecretSource, StepId};
use notchling_engine::vault::entry::VaultEntry;

use super::model::Sheet;
use super::model_tests::{mac, with_job, world_with_secrets};

const LONG_TOKEN: &str =
    "sk-ant-oat01-Q7w9Zr2LmX4pVb8NcT1yHs6KdJ3fGa5UeR0iOq2WzXc7VbN9mL1kJ4hG6fD8sA3pO5iU7yT9rE";

struct Values(BTreeMap<String, String>);

impl SecretSource for Values {
    fn value(&self, name: &str) -> Option<Secret> {
        self.0.get(name).map(Secret::new)
    }
    fn entry(&self, _name: &str) -> Option<VaultEntry> {
        None
    }
}

fn linux_machine() -> Machine {
    let mut m = Machine::new("vps-sing-1", "vps-sing-1.test", "agent");
    m.os = Some(MachineOs::Linux);
    m.set_admin(AdminAccess::RootSsh);
    with_job(m)
}

fn reachable_text(m: &Machine) -> &'static str {
    if m.is_linux() {
        "os=Linux\narch=x86_64\nuid=1000\nuser=agent\nhome=/home/agent\nversion=Ubuntu 24.04\ndisk_kb=83886080\ndocker=none"
    } else {
        "os=Darwin\narch=arm64\nuid=501\nuser=worker\nhome=/Users/worker\nversion=26.1\ndisk_kb=125829120\ndocker=none"
    }
}

/// Answers the batched checks (Reachable with the machine's facts, every other probe with an empty
/// success) and every other call with an empty success, recording each.
fn reachable_runner(m: &Machine) -> Arc<ScriptedRunner> {
    const PROBE: &str = "out=$( /bin/sh -c ";
    let runner = ScriptedRunner::new();
    let text = reachable_text(m);
    runner.on_call(
        |argv| argv.last().is_some_and(|s| s.contains(PROBE)),
        move |call| {
            let script = call.argv.last().cloned().unwrap_or_default();
            let Some(at) = script.find("@@") else {
                return CommandResult::ok("");
            };
            let nonce: String = script[at + 2..].chars().take(16).collect();
            let mut out = String::new();
            for (i, probe) in script.split(PROBE).skip(1).enumerate() {
                let answer = if probe.contains("os=$(uname -s)") {
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

/// The value, and the pieces of it a cut or a split could leave.
fn pieces(value: &str) -> Vec<String> {
    let chars: Vec<char> = value.chars().collect();
    let mid = chars.len() / 2;
    vec![
        value.to_string(),
        chars[..8].iter().collect(),
        chars[mid..mid + 8].iter().collect(),
        chars[chars.len() - 8..].iter().collect(),
    ]
}

#[test]
fn no_display_text_or_script_contains_a_vault_value() {
    for machine in [with_job(mac("m1")), linux_machine()] {
        let name = machine.name.clone();
        let issues = format!("{name}-github-issues");
        let reversed: String = LONG_TOKEN[13..].chars().rev().collect();
        let values = BTreeMap::from([
            ("claude-worker".to_string(), LONG_TOKEN.to_string()),
            (issues.clone(), format!("github_pat_{reversed}")),
            (
                "unrelated".to_string(),
                format!("zZ9-unrelated-vault-value-{name}-0123456789"),
            ),
        ]);
        let needles: Vec<String> = values.values().flat_map(|v| pieces(v)).collect();
        let runner = reachable_runner(&machine);
        let mut w = world_with_secrets(
            vec![machine.clone()],
            runner.clone(),
            runner.clone(),
            Arc::new(Values(values.clone())),
        );
        let engine = w.model.parts.engine.clone();
        engine.refresh(None, true);

        // every confirmation the window can show, and the two delivery Fixes run for real
        let mut shown: Vec<String> = Vec::new();
        let mut delivered = Vec::new();
        for step in engine.steps(&w.model.machine(&name).unwrap()) {
            w.model.request_set_up(&step.id, &name);
            let Some(Sheet::Fix(prompt)) = w.model.sheet() else {
                continue;
            };
            shown.push(prompt.text());
            if step.id == StepId::CLAUDE_LOGIN || step.id == StepId::ISSUES_TOKEN {
                let run = w.model.confirm().expect("a delivery runs");
                let (done, outcome) = run.execute(&engine);
                delivered.push(outcome.clone());
                w.model.finished(&done, &outcome);
            } else {
                w.model.cancel_sheet();
            }
        }
        assert_eq!(delivered.len(), 2, "{name}: both deliveries ran");
        assert!(delivered.iter().all(|o| o.ok), "{name}: {delivered:?}");
        w.model.request_set_up_all(&name);
        if let Some(Sheet::Fix(all)) = w.model.sheet() {
            shown.push(all.text());
            w.model.cancel_sheet();
        }
        // off is always possible; on waits for every step before Running
        w.model.request_toggle(&name, false);
        if let Some(Sheet::Fix(toggle)) = w.model.sheet() {
            shown.push(toggle.text());
            w.model.cancel_sheet();
        }
        w.model.request_toggle(&name, true);
        if let Some(Sheet::Fix(toggle)) = w.model.sheet() {
            shown.push(toggle.text());
            w.model.cancel_sheet();
        }
        // Update N machines for both renewed values
        for secret in ["claude-worker", issues.as_str()] {
            engine.vault_value_changed(secret);
            w.model.request_update(secret);
            let Some(Sheet::Fix(update)) = w.model.sheet() else {
                panic!("{name}: no Update sheet for {secret}");
            };
            shown.push(update.text());
            let run = w.model.confirm().expect("the update runs");
            let (done, outcome) = run.execute(&engine);
            assert!(outcome.ok, "{name}: {}", outcome.message);
            w.model.finished(&done, &outcome);
        }
        assert!(
            w.model.update_notes.is_empty(),
            "{:?}",
            w.model.update_notes
        );

        // 1. what ran: a value only in the stdin of the call that writes its file
        let calls = runner.calls();
        for value in [&values["claude-worker"], &values[&issues]] {
            let carrying: Vec<_> = calls
                .iter()
                .filter(|c| c.stdin_text().contains(value.as_str()))
                .collect();
            assert!(
                carrying.len() >= 2,
                "{name}: delivered by the Fix and by Update"
            );
            for call in carrying {
                assert!(
                    call.argv.last().is_some_and(|a| a.contains("cat > ")),
                    "{name}: a value went to a call that writes no file"
                );
            }
        }
        for call in &calls {
            let env: Vec<String> = call.env.iter().map(|(k, v)| format!("{k}={v}")).collect();
            let text = format!("{} {}", call.text(), env.join(" "));
            for needle in &needles {
                assert!(
                    !text.contains(needle.as_str()),
                    "{name}: argv or env carries a value piece: {}",
                    &text[..text.len().min(200)]
                );
            }
        }

        // 2. what the window says: sheets, scripts, logs, lines, results, notes
        let mut said = shown.clone();
        said.extend(engine.log(&name));
        said.extend(w.model.outcomes.values().map(|l| l.text.clone()));
        said.extend(w.model.update_notes.values().cloned());
        said.push(format!("{:?}", engine.results(&name)));
        for step in engine.steps(&w.model.machine(&name).unwrap()) {
            said.extend(step.probe.script().map(str::to_string));
            said.extend(step.plan().map(|p| p.display.clone()));
            said.extend(step.replace.as_ref().map(|r| r.display.clone()));
        }
        assert!(!shown.is_empty());
        for text in &said {
            for needle in &needles {
                assert!(
                    !text.contains(needle.as_str()),
                    "{name}: shown text carries a value piece: {}",
                    &text[..text.len().min(200)]
                );
            }
        }

        // 3. what the model holds
        let held = format!(
            "{:?} {:?} {:?} {:?} {:?} {:?} {:?} {:?} {:?}",
            w.model.sheet(),
            w.model.password,
            w.model.outcomes,
            w.model.update_notes,
            w.model.vault_entries,
            w.model.machines,
            w.model.items,
            w.model.notice,
            w.model.update_offers(),
        );
        for needle in &needles {
            assert!(
                !held.contains(needle.as_str()),
                "{name}: the model holds a value piece"
            );
        }
    }
}
