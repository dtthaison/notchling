use super::agent_plan::{fix_refusal, refusal, FAMILY_TAILSCALE};
use super::engine_plan::{AdminAnswer, AdminNeed, ApprovedStep, PlanId, PlanKind, ShownPlan};
use super::registry::AdminAccess;
use super::step::{FixOutcome, FixPlan, StepFix, StepId};
use super::steps_tailscale_linux::{INSTALL, SIGN_IN};
use super::tests_engine_world::*;
use std::sync::{Arc, Mutex};

const ENTRY: &str = "m1-admin";
const VAULT_PW: &str = "test-vault-password";
const TYPED_PW: &str = "test-typed-password";

fn plan_of(id: StepId, display: &str) -> ShownPlan {
    ShownPlan {
        id: PlanId(0),
        machine: "w1".into(),
        kind: PlanKind::SetUpAll,
        title: "Set up all on w1".into(),
        steps: vec![ApprovedStep {
            id,
            title: "Tailscale".into(),
            display: display.into(),
            waiting: false,
            needs_admin: true,
        }],
        admin: AdminNeed::None,
    }
}

#[test]
fn the_tailscale_sign_in_is_refused_for_agents_and_the_install_is_not() {
    assert_eq!(
        fix_refusal(&StepId::NODE_TAILSCALE, SIGN_IN),
        Some(FAMILY_TAILSCALE)
    );
    assert_eq!(fix_refusal(&StepId::NODE_TAILSCALE, INSTALL), None);
    assert_eq!(fix_refusal(&StepId::REACHABLE, SIGN_IN), None);
    assert_eq!(
        fix_refusal(&StepId::CLAUDE_LOGIN, "echo x"),
        Some("vault"),
        "the login steps keep pointing at the Vault"
    );
    assert_eq!(
        refusal(&plan_of(StepId::NODE_TAILSCALE, SIGN_IN)),
        Some(FAMILY_TAILSCALE)
    );
    assert_eq!(refusal(&plan_of(StepId::NODE_TAILSCALE, INSTALL)), None);
}

fn limited_linux(limits: bool) -> World {
    let mut s = Setup::new(|m, _| {
        let mut list = vec![StepBuilder::new("reachable").build()];
        list.extend(super::steps_node::steps(m));
        list
    });
    let mut m = linux("w1");
    m.set_admin(AdminAccess::RootSsh);
    m.node = Some(super::registry::NodeSettings::default());
    s.machines = vec![m];
    s.agent_limits = limits;
    s.build()
}

fn sign_in_ran(w: &World) -> bool {
    w.runner
        .calls()
        .iter()
        .any(|c| c.argv.last().is_some_and(|s| s.contains("tailscale up")))
}

#[test]
fn an_engine_with_agent_limits_never_runs_the_tailscale_sign_in() {
    let probe = "tailscale status --json --peers=false";
    let logged_out = r#"{"BackendState":"NeedsLogin","Self":{"TailscaleIPs":[]}}"#;
    for (limits, runs) in [(true, false), (false, true)] {
        let w = limited_linux(limits);
        w.answer(probe, logged_out, 0);
        w.refresh("w1", false);
        let out = w.fix("w1", "node-tailscale", AdminAnswer::RootConfirmed);
        assert_eq!(sign_in_ran(&w), runs, "limits {limits}: {}", out.message);
        assert_eq!(out.ok, runs, "limits {limits}: {}", out.message);
    }
}

#[test]
fn agent_checks_never_run_the_slow_probes_even_after_a_fix_stamped_the_clock() {
    let w = Setup::new(|_, _| {
        vec![
            step("r"),
            StepBuilder::new("s").needs(&["r"]).slow().build(),
        ]
    })
    .build();
    w.refresh("m1", true);
    let after_fix = w.batches().len();
    assert_eq!(after_fix, 2, "phase 1, then the slow probes");
    w.set_clock(T0 + 11.0 * 60.0);
    w.engine.refresh_cheap(Some(&["m1".to_string()]));
    assert_eq!(
        w.batches().len(),
        after_fix + 1,
        "over ten minutes later, an agent check runs the cheap probe only"
    );
    let last = w.batches().last().unwrap().text();
    assert!(last.contains("echo probe-r") && !last.contains("echo probe-s"));
    w.refresh("m1", false);
    assert_eq!(
        w.batches().len(),
        after_fix + 3,
        "the app's own refresh still runs them when due"
    );
}

fn recording(id: &str, seen: &Arc<Mutex<Vec<Option<String>>>>) -> super::step::SetupStep {
    let seen = seen.clone();
    StepBuilder::new(id)
        .fix(StepFix::Run(FixPlan::new("do admin", true, move |ctx| {
            lock(&seen).push(ctx.admin_password.as_ref().map(|p| p.expose().to_string()));
            FixOutcome::ok("done")
        })))
        .build()
}

fn two_steps(seen: &Arc<Mutex<Vec<Option<String>>>>) -> Setup {
    let s = seen.clone();
    Setup::new(move |_, _| {
        vec![
            StepBuilder::new("a")
                .fix(StepFix::Run(FixPlan::new("do a", false, |_| {
                    FixOutcome::ok("done")
                })))
                .build(),
            recording("b", &s),
        ]
    })
}

fn failing(w: &World) {
    w.answer("echo probe-a", "no", 1);
    w.answer("echo probe-b", "no", 1);
    w.refresh("m1", false);
}

#[test]
fn a_set_up_all_with_an_admin_step_has_the_admin_need_of_a_single_fix() {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let w = two_steps(&seen).build();
    failing(&w);
    let plan = w.engine.plan("m1", PlanKind::SetUpAll).unwrap();
    assert_eq!(plan.admin, AdminNeed::None, "the app asks per step");
    assert_eq!(w.engine.admin_need_of(&plan), AdminNeed::Password);

    let mut root = two_steps(&seen);
    root.machines[0].set_admin(AdminAccess::RootSsh);
    let w = root.build();
    failing(&w);
    let plan = w.engine.plan("m1", PlanKind::SetUpAll).unwrap();
    assert_eq!(w.engine.admin_need_of(&plan), AdminNeed::RootConfirm);

    let plain = Setup::new(|_, _| {
        vec![StepBuilder::new("a")
            .fix(StepFix::Run(FixPlan::new("do a", false, |_| {
                FixOutcome::ok("done")
            })))
            .build()]
    })
    .build();
    plain.answer("echo probe-a", "no", 1);
    plain.refresh("m1", false);
    let plan = plain.engine.plan("m1", PlanKind::SetUpAll).unwrap();
    assert_eq!(plain.engine.admin_need_of(&plan), AdminNeed::None);

    let fix = plain.engine.plan("m1", PlanKind::Fix(sid("a"))).unwrap();
    assert_eq!(plain.engine.admin_need_of(&fix), fix.admin);
}

#[test]
fn the_password_given_with_a_set_up_all_beats_the_saved_entry() {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let mut s = two_steps(&seen);
    s.machines[0].set_admin_secret(Some(ENTRY));
    s.secrets = Secrets::with(&[(ENTRY, VAULT_PW)]);
    let w = s.build();
    failing(&w);
    let plan = w.engine.plan("m1", PlanKind::SetUpAll).unwrap();
    let out = w
        .engine
        .run_with(w.engine.confirm(plan.id, pw(TYPED_PW)).unwrap(), &|_| {
            AdminAnswer::None
        });
    assert!(out.message.starts_with("2 steps set up"), "{}", out.message);
    assert_eq!(*lock(&seen), vec![Some(TYPED_PW.to_string())]);
    assert_eq!(lock(&w.secrets.reads).get(ENTRY), None);
}

#[test]
fn the_entry_given_with_a_set_up_all_is_the_one_used_without_asking() {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let mut s = two_steps(&seen);
    s.machines[0].set_admin_secret(Some(ENTRY));
    s.secrets = Secrets::with(&[(ENTRY, VAULT_PW)]);
    let w = s.build();
    failing(&w);
    let plan = w.engine.plan("m1", PlanKind::SetUpAll).unwrap();
    let out = w.engine.run_with(
        w.engine
            .confirm(plan.id, AdminAnswer::Vault(ENTRY.into()))
            .unwrap(),
        &|_| panic!("a given answer is never asked again"),
    );
    assert!(out.message.starts_with("2 steps set up"), "{}", out.message);
    assert_eq!(*lock(&seen), vec![Some(VAULT_PW.to_string())]);
}

#[test]
fn a_set_up_all_confirmed_as_root_runs_its_admin_steps_without_asking() {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let mut s = two_steps(&seen);
    s.machines = vec![{
        let mut m = linux("m1");
        m.set_admin(AdminAccess::RootSsh);
        m
    }];
    let w = s.build();
    failing(&w);
    let plan = w.engine.plan("m1", PlanKind::SetUpAll).unwrap();
    let out = w.engine.run_with(
        w.engine
            .confirm(plan.id, AdminAnswer::RootConfirmed)
            .unwrap(),
        &|_| panic!("a given answer is never asked again"),
    );
    assert!(out.message.starts_with("2 steps set up"), "{}", out.message);
    assert_eq!(*lock(&seen), vec![None]);
}
