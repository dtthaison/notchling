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
    let tmp = super::fakes::TempDir::new("offers");
    let dir = tmp.join("data");
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
        s.offers_data = Some(f.to_path_buf());
        s.build()
    };
    let w = make(&dir);
    w.engine.vault_value_changed("m1-github-issues");
    let text = crate::store::doc(&dir, super::offers::OFFERS_DOC)
        .unwrap()
        .expect("the offer is a document");
    assert!(
        text.contains("\"version\": 1")
            && text.contains("m1-github-issues")
            && text.contains("\"m1\"")
    );
    assert!(!text.contains("fake-ghp-3333"));
    assert!(
        !super::offers::old_offers_file(&dir).exists(),
        "no file is made"
    );
    let again = make(&dir);
    assert_eq!(again.engine.update_offers().len(), 1);
    again.engine.forget("m1");
    assert_eq!(
        crate::store::doc(&dir, super::offers::OFFERS_DOC).unwrap(),
        None,
        "an empty offer removes the document"
    );
}

fn offers_world(data: &std::path::Path) -> World {
    let mut s = Setup::new(|_, _| vec![]);
    let mut m = mac("m1");
    m.set_sd_worker(Some(SdWorkerSettings::new("m1")));
    s.machines = vec![m];
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
    s.offers_data = Some(data.to_path_buf());
    s.build()
}

const OLD_OFFERS: &str = r#"{"version":1,"offers":{"m1-github-issues":["m1"]}}"#;

#[test]
fn the_old_offers_file_moves_in_once_and_a_bad_one_stays() {
    let tmp = super::fakes::TempDir::new("offers-import");
    let dir = tmp.join("data");
    let old = super::offers::old_offers_file(&dir);
    std::fs::create_dir_all(old.parent().unwrap()).unwrap();
    std::fs::write(&old, OLD_OFFERS).unwrap();
    let w = offers_world(&dir);
    assert_eq!(w.engine.update_offers().len(), 1);
    assert!(!old.exists(), "moved into the database");
    assert!(crate::store::doc(&dir, super::offers::OFFERS_DOC)
        .unwrap()
        .is_some());

    let bad = tmp.join("bad");
    let bad_file = super::offers::old_offers_file(&bad);
    std::fs::create_dir_all(bad_file.parent().unwrap()).unwrap();
    std::fs::write(&bad_file, "{ not json").unwrap();
    let w = offers_world(&bad);
    assert!(w.engine.update_offers().is_empty());
    assert!(bad_file.exists(), "left where it is");
    assert!(!crate::store::exists(&bad), "and no database made for it");
}

#[test]
fn an_empty_offer_makes_no_database() {
    let tmp = super::fakes::TempDir::new("offers-empty");
    let dir = tmp.join("data");
    let w = offers_world(&dir);
    w.engine.forget("m1");
    assert!(!dir.exists(), "nothing to keep, nothing made");
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
