use super::agent_plan::{
    digest, kind_refusal, refusal, step_refusal, FAMILY_CLUSTER, FAMILY_VAULT,
};
use super::claims::{Claims, HELD};
use super::engine_plan::{AdminAnswer, PlanKind};
use super::step::{FixOutcome, FixPlan, StepFix, StepId};
use super::tests_engine_world::*;
use std::sync::{Arc, Mutex};

fn counted(shown: &str, ran: &Arc<Mutex<usize>>) -> super::step::SetupStep {
    let ran = ran.clone();
    StepBuilder::new("a")
        .fix(StepFix::Run(FixPlan::new(shown, false, move |_| {
            *lock(&ran) += 1;
            FixOutcome::ok("done")
        })))
        .build()
}

#[test]
fn the_refused_steps_name_the_family_that_does_them() {
    for id in [
        StepId::ISSUES_TOKEN,
        StepId::TOKEN_SCOPE,
        StepId::CLAUDE_LOGIN,
        StepId::NODE_CODEX_LOGIN,
        StepId::NODE_AGY_LOGIN,
    ] {
        assert_eq!(step_refusal(&id), Some(FAMILY_VAULT), "{id:?}");
        assert_eq!(kind_refusal(&PlanKind::Fix(id.clone())), Some(FAMILY_VAULT));
        assert_eq!(kind_refusal(&PlanKind::Replace(id)), Some(FAMILY_VAULT));
    }
    assert_eq!(step_refusal(&StepId::NODE_JOINED), Some(FAMILY_CLUSTER));
    assert_eq!(step_refusal(&StepId::REACHABLE), None);
    assert_eq!(kind_refusal(&PlanKind::SetUpAll), None);
}

#[test]
fn a_plan_that_holds_a_refused_step_is_refused() {
    let w = Setup::new(|_, _| {
        vec![
            StepBuilder::new("a").build(),
            StepBuilder::new("claude-login")
                .fix(StepFix::Run(FixPlan::new("echo x", false, |_| {
                    FixOutcome::ok("")
                })))
                .build(),
        ]
    })
    .build();
    w.answer("echo probe-a", "no", 1);
    w.answer("echo probe-claude-login", "no", 1);
    w.refresh("m1", false);
    let plan = w.engine.plan("m1", PlanKind::SetUpAll).unwrap();
    assert_eq!(refusal(&plan), Some(FAMILY_VAULT));
}

#[test]
fn the_digest_follows_what_is_shown_and_not_the_plan_id() {
    let ran = Arc::new(Mutex::new(0));
    let first = {
        let r = ran.clone();
        Setup::new(move |_, _| vec![counted("echo one", &r)]).build()
    };
    let second = {
        let r = ran.clone();
        Setup::new(move |_, _| vec![counted("echo one", &r)]).build()
    };
    let changed = {
        let r = ran.clone();
        Setup::new(move |_, _| vec![counted("echo two", &r)]).build()
    };
    for w in [&first, &second, &changed] {
        w.answer("echo probe-a", "no", 1);
        w.refresh("m1", false);
    }
    let a = first.engine.plan("m1", PlanKind::SetUpAll).unwrap();
    let b = second.engine.plan("m1", PlanKind::SetUpAll).unwrap();
    let c = changed.engine.plan("m1", PlanKind::SetUpAll).unwrap();
    assert_ne!(a.id, b.id);
    assert_eq!(digest(&a), digest(&b));
    assert_ne!(digest(&a), digest(&c));
    assert_eq!(*lock(&ran), 0);
}

#[test]
fn a_run_is_refused_while_another_process_holds_the_machines_claim() {
    let dir = std::env::temp_dir().join(format!("nl-claim-engine-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let ran = Arc::new(Mutex::new(0));
    let r = ran.clone();
    let mut s = Setup::new(move |_, _| vec![counted("echo one", &r)]);
    s.offers_data = Some(dir.clone());
    let w = s.build();
    w.answer("echo probe-a", "no", 1);
    w.refresh("m1", false);
    let held = Claims::in_data(&dir).take("m1").unwrap();
    let plan = w.engine.plan("m1", PlanKind::SetUpAll).unwrap();
    let out = w
        .engine
        .run(w.engine.confirm(plan.id, AdminAnswer::None).unwrap());
    assert!(!out.ok);
    assert!(out.message.contains(HELD), "{}", out.message);
    assert_eq!(*lock(&ran), 0);
    drop(held);
    let again = w.engine.plan("m1", PlanKind::SetUpAll).unwrap();
    let out = w
        .engine
        .run(w.engine.confirm(again.id, AdminAnswer::None).unwrap());
    assert!(!out.message.contains(HELD), "{}", out.message);
    assert_eq!(*lock(&ran), 1);
    let _ = std::fs::remove_dir_all(&dir);
}
