//! Renewed secrets behind plan, confirm, run (Swift `MachinesSetupEngineTests`, update offers,
//! Send again, the pending-updates file).

use super::engine_plan::{AdminAnswer, PlanKind};
use super::registry::SdWorkerSettings;
use super::step::{run, FixOutcome, StepId};
use super::tests_engine_world::*;
use super::usage::{SecretFormat, SecretUsage};
use std::sync::Arc;

fn issues_world() -> World {
    let mut s = Setup::new(|_, _| vec![step("issues-token"), step("token-scope")]);
    let mut m = mac("m1");
    m.set_sd_worker(Some(SdWorkerSettings::new("m1")));
    s.machines = vec![m];
    s.secrets = Secrets::with(&[("m1-github-issues", "fake-ghp-2222")]);
    s.usages = Arc::new(|ms| {
        ms.iter()
            .map(|m| SecretUsage {
                name: format!("{}-github-issues", m.name),
                machine: m.name.clone(),
                remote_path: super::shell::GITHUB_TOKEN.into(),
                format: SecretFormat::Raw,
            })
            .collect()
    });
    s.deliver = Arc::new(|u, ctx| {
        let value = ctx
            .step
            .secrets
            .value(&u.name)
            .map(|v| v.expose().to_string())
            .unwrap_or_default();
        let r = run::script(ctx, "cat > /dev/null", Some(value.clone().into_bytes()), 60);
        if u.machine == "m1" && r.code == 0 {
            FixOutcome::ok(format!("sent {value}"))
        } else {
            FixOutcome::fail("could not send")
        }
    });
    s.build()
}

#[test]
fn a_vault_change_only_offers_the_update_and_update_delivers_on_the_confirmed_plan() {
    let w = issues_world();
    w.engine.vault_value_changed("m1-github-issues");
    assert_eq!(w.engine.update_offers()["m1-github-issues"].len(), 1);
    assert!(w.runner.calls().is_empty(), "the offer sends nothing");

    let plan = w
        .engine
        .plan("", PlanKind::Update("m1-github-issues".into()))
        .unwrap();
    assert_eq!(plan.title, "Update 1 machine(s)");
    let out = w
        .engine
        .run(w.engine.confirm(plan.id, AdminAnswer::None).unwrap());
    assert!(out.ok, "{}", out.message);
    let sent: Vec<_> = w
        .runner
        .calls()
        .into_iter()
        .filter(|c| c.stdin.is_some())
        .collect();
    assert_eq!(sent.len(), 1);
    assert_eq!(
        sent[0].stdin_text(),
        "fake-ghp-2222",
        "the value travels on stdin only"
    );
    assert!(!sent[0].text().contains("fake-ghp-2222"));
    assert!(
        w.log("m1").contains("sent [secret:m1-github-issues]"),
        "{}",
        w.log("m1")
    );
    assert!(w.engine.update_offers().is_empty());
    assert!(!w.batches().is_empty(), "the step is checked again");
}

#[test]
fn send_again_is_only_for_the_machines_own_files() {
    let w = issues_world();
    let other = SecretUsage {
        name: "m1-github-issues".into(),
        machine: "m1".into(),
        remote_path: "/etc/passwd".into(),
        format: SecretFormat::Raw,
    };
    assert!(w
        .engine
        .plan("m1", PlanKind::SendAgain(other))
        .unwrap_err()
        .contains("not a secret file"));
    let usage = SecretUsage {
        remote_path: super::shell::GITHUB_TOKEN.into(),
        ..SecretUsage {
            name: "m1-github-issues".into(),
            machine: "m1".into(),
            remote_path: String::new(),
            format: SecretFormat::Raw,
        }
    };
    let out = w.click("m1", PlanKind::SendAgain(usage), AdminAnswer::None);
    assert!(out.ok, "{}", out.message);
    let _ = StepId::ISSUES_TOKEN;
}

#[test]
fn the_pending_offer_survives_a_new_engine_and_holds_names_only() {
    let dir = std::env::temp_dir().join(format!("notchling-offers-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let file = dir.join("pending-updates.json");
    let make = |f: &std::path::Path| {
        let mut s = Setup::new(|_, _| vec![]);
        let mut m = mac("m1");
        m.set_sd_worker(Some(SdWorkerSettings::new("m1")));
        s.machines = vec![m];
        s.secrets = Secrets::with(&[("m1-github-issues", "fake-ghp-3333")]);
        s.usages = Arc::new(|ms| {
            ms.iter()
                .map(|m| SecretUsage {
                    name: format!("{}-github-issues", m.name),
                    machine: m.name.clone(),
                    remote_path: super::shell::GITHUB_TOKEN.into(),
                    format: SecretFormat::Raw,
                })
                .collect()
        });
        s.offers_file = Some(f.to_path_buf());
        s.build()
    };
    let w = make(&file);
    w.engine.vault_value_changed("m1-github-issues");
    let text = std::fs::read_to_string(&file).unwrap();
    assert!(
        text.contains("\"version\": 1")
            && text.contains("m1-github-issues")
            && text.contains("\"m1\"")
    );
    assert!(!text.contains("fake-ghp-3333"));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(&file).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
    let again = make(&file);
    assert_eq!(again.engine.update_offers().len(), 1);
    again.engine.forget("m1");
    assert!(!file.exists(), "an empty offer removes the file");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_failed_delivery_stays_in_the_offer() {
    let mut s = Setup::new(|_, _| vec![step("issues-token")]);
    let mut m = mac("m1");
    m.set_sd_worker(Some(SdWorkerSettings::new("m1")));
    s.machines = vec![m];
    s.usages = Arc::new(|_| {
        vec![SecretUsage {
            name: "m1-github-issues".into(),
            machine: "m1".into(),
            remote_path: super::shell::GITHUB_TOKEN.into(),
            format: SecretFormat::Raw,
        }]
    });
    s.deliver = Arc::new(|_, _| FixOutcome::fail("could not send"));
    let w = s.build();
    w.engine.vault_value_changed("m1-github-issues");
    let out = w.click(
        "",
        PlanKind::Update("m1-github-issues".into()),
        AdminAnswer::None,
    );
    assert!(!out.ok);
    assert_eq!(w.engine.update_offers()["m1-github-issues"].len(), 1);
}

#[test]
fn every_fix_that_ran_is_audited_without_its_output() {
    let dir = std::env::temp_dir().join(format!("notchling-audit-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let file = dir.join("audit.jsonl");
    let mut s = Setup::new(|_, _| {
        vec![StepBuilder::new("a")
            .fix(super::step::StepFix::Run(super::step::FixPlan::new(
                "echo",
                false,
                |_| FixOutcome::ok("printed fake-claude-0000"),
            )))
            .build()]
    });
    s.secrets = Secrets::with(&[("claude-worker", "fake-claude-0000")]);
    s.audit = Some(file.clone());
    let w = s.build();
    assert!(
        w.click("m1", PlanKind::Fix(StepId("a".into())), AdminAnswer::None)
            .ok
    );
    let text = std::fs::read_to_string(&file).unwrap();
    assert_eq!(text.lines().count(), 1);
    assert!(text.contains("\"op\":\"machines.fix\"") && text.contains("\"machine\":\"m1\""));
    assert!(!text.contains("fake-claude") && !text.contains("printed"));
    let _ = std::fs::remove_dir_all(&dir);
}
