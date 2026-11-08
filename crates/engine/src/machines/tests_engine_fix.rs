//! The engine's Fixes behind plan → confirm → run (Swift `MachinesSetupEngineTests`, Fixes, admin,
//! Set up all, Running, renewed secrets).

use super::engine_plan::{AdminAnswer, AdminNeed, PlanKind};
use super::registry::AdminAccess;
use super::runner::{CommandResult, REFUSED};
use super::step::{run, FixOutcome, FixPlan, StepFix};
use super::tests_engine_world::*;
use std::sync::{Arc, Mutex};

fn admin_step() -> super::step::SetupStep {
    StepBuilder::new("adm")
        .fix(StepFix::Run(FixPlan::new("echo installing", true, |ctx| {
            let r = run::admin(ctx, "echo installing", 900);
            if r.code == 0 {
                FixOutcome::ok(format!("said {}", r.out.trim()))
            } else {
                FixOutcome::fail(r.err)
            }
        })))
        .build()
}

/// The fake machine behind sudo (or as root) echoes whatever came on stdin.
fn echo_stdin(s: &Setup) {
    s.runner.on_call(
        |a| {
            a.last()
                .is_some_and(|x| x.contains("sudo -S") || x.contains("env -i"))
        },
        |call| {
            let text = call
                .stdin
                .as_ref()
                .map(|d| String::from_utf8_lossy(d).into_owned())
                .unwrap_or_default();
            if let Some(sink) = &call.on_output {
                sink(super::runner::Channel::Stdout, "hunter2-");
                sink(super::runner::Channel::Stdout, "Pw!\ndone\n");
            }
            CommandResult::ok(&text)
        },
    );
}

#[test]
fn nothing_runs_without_a_shown_plan_and_a_plan_runs_once() {
    let counter = Arc::new(Mutex::new(Vec::new()));
    let c = counter.clone();
    let w = Setup::new(move |_, _| {
        vec![StepBuilder::new("a")
            .fix(counting_fix(&c, "a", true, false))
            .build()]
    })
    .build();
    let plan = w.engine.plan("m1", PlanKind::Fix(sid("a"))).unwrap();
    assert_eq!(plan.steps[0].display, "do a");
    assert_eq!(plan.admin, AdminNeed::None);
    assert!(lock(&counter).is_empty(), "a plan runs nothing");
    assert!(w.runner.calls().is_empty());

    let confirmed = w.engine.confirm(plan.id, AdminAnswer::None).unwrap();
    assert!(w.engine.run(confirmed).ok);
    assert_eq!(lock(&counter).clone(), vec!["a".to_string()]);
    assert!(
        w.engine.confirm(plan.id, AdminAnswer::None).is_err(),
        "one confirmation, one run"
    );
    assert!(w
        .engine
        .confirm(super::engine_plan::PlanId(plan.id.0 ^ 1), AdminAnswer::None)
        .is_err());
    assert!(w.log("m1").contains("▸ A") && w.log("m1").contains("✓ did a"));
}

#[test]
fn a_plan_whose_commands_changed_after_it_showed_runs_nothing() {
    let display = Arc::new(Mutex::new("echo one".to_string()));
    let counter = Arc::new(Mutex::new(0));
    let (d, c) = (display.clone(), counter.clone());
    let w = Setup::new(move |_, _| {
        let c = c.clone();
        let shown = lock(&d).clone();
        vec![StepBuilder::new("a")
            .fix(StepFix::Run(FixPlan::new(shown, false, move |_| {
                *lock(&c) += 1;
                FixOutcome::ok("ran")
            })))
            .build()]
    })
    .build();
    let plan = w.engine.plan("m1", PlanKind::Fix(sid("a"))).unwrap();
    *lock(&display) = "echo two".into();
    let c = w.engine.confirm(plan.id, AdminAnswer::None).unwrap();
    let out = w.engine.run(c);
    assert!(!out.ok);
    assert!(out.message.contains("needs a look"), "{}", out.message);
    assert_eq!(*lock(&counter), 0);
}

#[test]
fn an_old_plan_is_refused() {
    let counter = Arc::new(Mutex::new(Vec::new()));
    let c = counter.clone();
    let w = Setup::new(move |_, _| {
        vec![StepBuilder::new("a")
            .fix(counting_fix(&c, "a", true, false))
            .build()]
    })
    .build();
    let plan = w.engine.plan("m1", PlanKind::Fix(sid("a"))).unwrap();
    w.set_clock(T0 + super::engine_plan::PLAN_LIFETIME + 1.0);
    assert!(w.engine.confirm(plan.id, AdminAnswer::None).is_err());
    assert!(lock(&counter).is_empty());
}

#[test]
fn how_to_toggle_and_unknown_steps_have_no_plan() {
    let w = Setup::new(|_, _| {
        vec![
            step("h"),
            StepBuilder::new("running")
                .fix(StepFix::Toggle { needs_admin: false })
                .build(),
        ]
    })
    .build();
    assert!(w
        .engine
        .plan("m1", PlanKind::Fix(sid("h")))
        .unwrap_err()
        .contains("has no Fix"));
    assert!(w
        .engine
        .plan("m1", PlanKind::Fix(sid("running")))
        .unwrap_err()
        .contains("is a switch"));
    assert!(w
        .engine
        .plan("m1", PlanKind::Fix(sid("nope")))
        .unwrap_err()
        .contains("has no step"));
    assert!(w.engine.plan("ghost", PlanKind::Fix(sid("h"))).is_err());
    assert!(w.runner.calls().is_empty());
}

#[test]
fn a_fix_waits_for_its_needs_and_for_another_fix() {
    let counter = Arc::new(Mutex::new(Vec::new()));
    let c = counter.clone();
    let w = Setup::new(move |_, _| {
        vec![
            step("a"),
            StepBuilder::new("b")
                .needs(&["a"])
                .fix(counting_fix(&c, "b", true, false))
                .build(),
        ]
    })
    .build();
    w.answer("echo probe-a", "no", 1);
    w.refresh("m1", false);
    let out = w.fix("m1", "b", AdminAnswer::None);
    assert_eq!(out, FixOutcome::fail("waits for A"));
    assert!(lock(&counter).is_empty());
}

#[test]
fn the_admin_password_goes_only_to_stdin_and_is_masked() {
    let p = "hunter2-Pw!";
    let s = Setup::new(|_, _| vec![admin_step()]);
    echo_stdin(&s);
    let w = s.build();
    let plan = w.engine.plan("m1", PlanKind::Fix(sid("adm"))).unwrap();
    assert_eq!(plan.admin, AdminNeed::Password);
    let out = w.engine.run(w.engine.confirm(plan.id, pw(p)).unwrap());
    assert!(out.ok, "{}", out.message);
    let sudo: Vec<_> = w.runner.calls().into_iter().filter(is_sudo).collect();
    assert_eq!(sudo.len(), 1);
    assert_eq!(
        sudo[0].stdin_text(),
        format!("{p}\n"),
        "the password is sudo's stdin"
    );
    let remote = sudo[0].argv.last().unwrap();
    assert!(
        remote.contains("/usr/bin/sudo -S -k -p ") && remote.contains("/usr/bin/env -i PATH="),
        "{remote}"
    );
    for call in w.runner.calls() {
        assert!(!call.text().contains(p));
        assert!(!call.env.iter().any(|(_, v)| v.contains(p)));
    }
    let log = w.log("m1");
    assert!(log.contains("[admin password]"), "{log}");
    assert!(
        !log.contains("hunter2"),
        "a value cut between two chunks is still masked: {log}"
    );
    assert!(!out.message.contains(p) && out.message.contains("[admin password]"));
    assert!(!format!("{:?}", w.engine.results("m1")).contains(p));
}

#[test]
fn an_admin_plan_needs_its_answer_before_anything_runs() {
    let w = Setup::new(|_, _| vec![admin_step()]).build();
    for answer in [AdminAnswer::None, pw(""), AdminAnswer::RootConfirmed] {
        let plan = w.engine.plan("m1", PlanKind::Fix(sid("adm"))).unwrap();
        let err = w.engine.confirm(plan.id, answer).err().unwrap();
        assert!(err.contains("admin password"), "{err}");
    }
    assert!(
        w.runner.calls().is_empty(),
        "refused before any command ran"
    );
    assert!(w.engine.log("m1").is_empty());
}

#[test]
fn root_over_ssh_needs_the_confirmation_not_a_password() {
    let mut s = Setup::new(|_, _| vec![admin_step()]);
    let mut vps = linux("vps");
    vps.set_admin(AdminAccess::RootSsh);
    s.machines = vec![vps];
    echo_stdin(&s);
    let w = s.build();
    let plan = w.engine.plan("vps", PlanKind::Fix(sid("adm"))).unwrap();
    assert_eq!(plan.admin, AdminNeed::RootConfirm);
    assert!(w.engine.confirm(plan.id, pw("hunter2-Pw!")).is_err());
    assert!(w.runner.calls().is_empty());
    let out = w.click("vps", PlanKind::Fix(sid("adm")), AdminAnswer::RootConfirmed);
    assert!(out.ok, "{}", out.message);
    let call = w
        .runner
        .calls()
        .into_iter()
        .find(|c| c.argv.contains(&"root@vps.test".to_string()))
        .unwrap();
    assert!(call.stdin.is_none());
    assert!(!call.argv.last().unwrap().contains("sudo"));
}

#[test]
fn a_linux_admin_step_as_the_worker_is_refused_before_any_command_runs() {
    let refusal = "On Linux the worker account can't be the admin: its own shell files could read the password. Use root over SSH or another admin account.";
    let mut s = Setup::new(|_, _| vec![admin_step()]);
    let mut vps = linux("vps");
    vps.set_admin(AdminAccess::Sudo);
    s.machines = vec![vps];
    let w = s.build();
    let plan = w.engine.plan("vps", PlanKind::Fix(sid("adm"))).unwrap();
    assert_eq!(plan.admin, AdminNeed::Refused(refusal.into()));
    assert_eq!(
        w.engine
            .confirm(plan.id, pw("hunter2-Pw!"))
            .err()
            .as_deref(),
        Some(refusal)
    );
    assert!(w.runner.calls().is_empty());

    // Set up all reaches the step and stops there without asking
    w.answer("echo probe-adm", "bad", 1);
    w.refresh("vps", false);
    let asked = Arc::new(Mutex::new(0));
    let a = asked.clone();
    let plan = w.engine.plan("vps", PlanKind::SetUpAll).unwrap();
    assert_eq!(
        plan.steps
            .iter()
            .map(|s| s.id.raw().to_string())
            .collect::<Vec<_>>(),
        vec!["adm"]
    );
    let out = w.engine.run_with(
        w.engine.confirm(plan.id, AdminAnswer::None).unwrap(),
        &move |_| {
            *lock(&a) += 1;
            pw("hunter2-Pw!")
        },
    );
    assert!(!out.ok && out.message.contains(refusal), "{}", out.message);
    assert_eq!(*lock(&asked), 0);
    assert!(!w.runner.calls().iter().any(is_sudo));
}

#[test]
fn a_non_admin_step_never_gets_the_password_and_a_secret_never_reaches_argv() {
    let seen = Arc::new(Mutex::new(Some("untouched".to_string())));
    let s2 = seen.clone();
    let mut s = Setup::new(move |_, _| {
        let s2 = s2.clone();
        vec![StepBuilder::new("p")
            .fix(StepFix::Run(FixPlan::new("plain", false, move |ctx| {
                *lock(&s2) = ctx.admin_password.as_ref().map(|p| p.expose().to_string());
                // a step that put a vault value on a command line is refused before anything runs
                let r = run::local(
                    ctx,
                    vec!["/bin/echo".into(), "fake-claude-0000".into()],
                    None,
                    5,
                );
                FixOutcome {
                    ok: r.code == REFUSED,
                    message: r.err,
                }
            })))
            .build()]
    });
    s.secrets = Secrets::with(&[("claude-worker", "fake-claude-0000")]);
    let w = s.build();
    let out = w.fix("m1", "p", pw("hunter2-Pw!"));
    assert!(out.ok, "{}", out.message);
    assert_eq!(*lock(&seen), None);
    assert!(
        !w.runner
            .calls()
            .iter()
            .any(|c| c.text().contains("/bin/echo")),
        "the call never reached the runner"
    );
}

#[test]
fn set_up_all_runs_failed_steps_in_order_asks_each_admin_step_and_stops_at_the_first_failure() {
    let counter = Arc::new(Mutex::new(Vec::new()));
    let c = counter.clone();
    let w = Setup::new(move |_, _| {
        vec![
            StepBuilder::new("a")
                .fix(counting_fix(&c, "a", true, false))
                .build(),
            StepBuilder::new("b")
                .fix(counting_fix(&c, "b", true, true))
                .build(),
            StepBuilder::new("c")
                .fix(counting_fix(&c, "c", false, false))
                .build(),
            StepBuilder::new("d")
                .fix(counting_fix(&c, "d", true, false))
                .build(),
        ]
    })
    .build();
    for p in ["a", "b", "c", "d"] {
        w.answer(&format!("echo probe-{p}"), "no", 1);
    }
    w.refresh("m1", false);
    let plan = w.engine.plan("m1", PlanKind::SetUpAll).unwrap();
    assert_eq!(plan.steps.len(), 4);
    assert!(plan.steps[1].needs_admin);
    let asked = Arc::new(Mutex::new(Vec::new()));
    let a = asked.clone();
    let out = w.engine.run_with(
        w.engine.confirm(plan.id, AdminAnswer::None).unwrap(),
        &move |s| {
            lock(&a).push(s.id.raw().to_string());
            pw("hunter2-Pw!")
        },
    );
    assert_eq!(lock(&counter).clone(), vec!["a", "b", "c"]);
    assert_eq!(lock(&asked).clone(), vec!["b"]);
    assert!(
        !out.ok && out.message.starts_with("C: could not c"),
        "{}",
        out.message
    );
}

#[test]
fn cancelling_the_admin_question_stops_set_up_all() {
    let counter = Arc::new(Mutex::new(Vec::new()));
    let c = counter.clone();
    let w = Setup::new(move |_, _| {
        vec![StepBuilder::new("b")
            .fix(counting_fix(&c, "b", true, true))
            .build()]
    })
    .build();
    w.answer("echo probe-b", "no", 1);
    w.refresh("m1", false);
    let plan = w.engine.plan("m1", PlanKind::SetUpAll).unwrap();
    let out = w
        .engine
        .run(w.engine.confirm(plan.id, AdminAnswer::None).unwrap());
    assert_eq!(
        out,
        FixOutcome::fail("Set up all stopped before B: cancelled")
    );
    assert!(lock(&counter).is_empty());
}

#[test]
fn set_up_all_stops_at_a_step_that_was_not_listed_and_says_what_is_still_not_ok() {
    let counter = Arc::new(Mutex::new(Vec::new()));
    let c = counter.clone();
    let w = Setup::new(move |_, _| {
        vec![
            StepBuilder::new("a")
                .fix(counting_fix(&c, "a", true, false))
                .build(),
            StepBuilder::new("b")
                .fix(counting_fix(&c, "b", true, false))
                .build(),
        ]
    })
    .build();
    w.answer("echo probe-a", "no", 1);
    w.refresh("m1", false);
    let plan = w.engine.plan("m1", PlanKind::SetUpAll).unwrap();
    assert_eq!(plan.steps.len(), 1);
    // the Fix of a breaks b, which the user was never shown
    w.answer("echo probe-b", "no", 1);
    let out = w
        .engine
        .run(w.engine.confirm(plan.id, AdminAnswer::None).unwrap());
    assert_eq!(lock(&counter).clone(), vec!["a"]);
    assert_eq!(out, FixOutcome::fail("B needs a look; Set up again"));
}

#[test]
fn turning_the_worker_on_waits_for_every_step_before_it_but_off_never_does() {
    let s = Setup::new(|_, _| {
        vec![
            StepBuilder::new("a").title("Token scope").build(),
            StepBuilder::new("running")
                .fix(StepFix::Toggle { needs_admin: false })
                .build(),
        ]
    });
    s.runner.on_script("toggle mac", CommandResult::ok(""));
    let w = s.build();
    w.answer("echo probe-a", "too wide", 1);
    w.refresh("m1", false);
    let on = w.click("m1", PlanKind::Running(true), AdminAnswer::None);
    assert_eq!(on, FixOutcome::fail("waits for Token scope"));
    assert!(!w
        .runner
        .calls()
        .iter()
        .any(|c| c.text().contains("toggle mac")));
    let plan = w.engine.plan("m1", PlanKind::Running(false)).unwrap();
    assert_eq!(
        plan.steps[0].display, "toggle mac off",
        "the script is shown first"
    );
    let off = w
        .engine
        .run(w.engine.confirm(plan.id, AdminAnswer::None).unwrap());
    assert_eq!(off, FixOutcome::ok("Running is off"));
}
