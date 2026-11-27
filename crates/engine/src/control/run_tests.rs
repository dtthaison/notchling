use super::run::is_untrusted;
use super::*;
use std::cell::RefCell;

#[test]
fn job_id_in_launch_output_across_versions() {
    assert_eq!(
        job_id_in_launch_output(
            "backgrounded · 6ce4c19c · fix-hook (idle — send a prompt to start)"
        )
        .as_deref(),
        Some("6ce4c19c")
    );
    let v318 = "note: background session 8f985af0 keeps its own saved options, so the flags you passed started a copy as 6220cc2b.\nbackgrounded · 6220cc2b (idle — send a prompt to start)\n  claude agents          list sessions\n";
    assert_eq!(job_id_in_launch_output(v318).as_deref(), Some("6220cc2b"));
    assert_eq!(job_id_in_launch_output("error: no such session"), None);
    assert_eq!(job_id_in_launch_output("backgrounded · notahexid"), None);
    assert_eq!(job_id_in_launch_output("backgrounded · 6ce4c19"), None);
}

#[test]
fn the_untrusted_refusal_is_recognised() {
    assert!(is_untrusted(
        "Error: Workspace not trusted. Run claude here once."
    ));
    assert!(!is_untrusted("backgrounded · 6ce4c19c"));
}

fn stop_then_start() -> Plan {
    Plan {
        steps: vec![
            Step::Run(RunStep {
                command: Command::new(&["claude", "stop", "ab12"]),
                timeout_secs: 60,
                expect: Expect::ExitZero,
                required: true,
                keep_going: false,
                pause_after_ms: 1500,
            }),
            Step::Run(RunStep {
                command: Command::new(&["claude", "--bg", "--resume", "c1"]),
                timeout_secs: 60,
                expect: Expect::JobId,
                required: true,
                keep_going: false,
                pause_after_ms: 0,
            }),
        ],
        outcome: Outcome::NewJob,
    }
}

fn pauses() -> (RefCell<Vec<u64>>, ()) {
    (RefCell::new(Vec::new()), ())
}

#[test]
fn a_good_move_pauses_after_the_stop_and_reports_the_new_job() {
    let fake = FakeLauncher::new();
    fake.push_output(Ok(LaunchOutput::ok("stopped ab12")));
    fake.push_output(Ok(LaunchOutput::ok("backgrounded · 9a8b7c6d (idle)")));
    let (waited, ()) = pauses();
    let report = execute(&stop_then_start(), &fake, &|ms| {
        waited.borrow_mut().push(ms)
    })
    .expect("ran");
    assert_eq!(report.job_id.as_deref(), Some("9a8b7c6d"));
    assert_eq!(*waited.borrow(), [1500]);
    assert_eq!(fake.commands().len(), 2);
}

#[test]
fn a_failed_stop_ends_the_plan_before_the_resume() {
    let fake = FakeLauncher::new();
    fake.push_output(Ok(LaunchOutput::failed(1, "no such job\n")));
    let result = execute(&stop_then_start(), &fake, &|_| {});
    assert_eq!(result, Err(ControlError::Failed("no such job".into())));
    assert_eq!(fake.commands().len(), 1, "nothing resumed");

    let silent = FakeLauncher::new();
    silent.push_output(Ok(LaunchOutput::failed(4, "")));
    assert_eq!(
        execute(&stop_then_start(), &silent, &|_| {}),
        Err(ControlError::Failed(
            "claude stop exited with code 4.".into()
        ))
    );
}

#[test]
fn start_output_without_an_id_is_the_error_and_untrusted_is_told_apart() {
    let start = |output: &str| {
        let fake = FakeLauncher::new();
        fake.push_output(Ok(LaunchOutput::ok(output)));
        let plan = Plan {
            steps: vec![stop_then_start().steps[1].clone()],
            outcome: Outcome::NewJob,
        };
        execute(&plan, &fake, &|_| {})
    };
    assert_eq!(
        start("Workspace not trusted\n"),
        Err(ControlError::Untrusted("Workspace not trusted".into()))
    );
    assert_eq!(start("boom"), Err(ControlError::Failed("boom".into())));
    assert_eq!(
        start(""),
        Err(ControlError::Failed(
            "claude --bg exited with code 0.".into()
        ))
    );
}

#[test]
fn a_timeout_names_the_command_and_the_limit() {
    let fake = FakeLauncher::new();
    fake.push_output(Err(LaunchError::Timeout(60)));
    assert_eq!(
        execute(&stop_then_start(), &fake, &|_| {}),
        Err(ControlError::Failed(
            "claude stop didn't finish in 60 s.".into()
        ))
    );
}

fn rm_plan(current_keeps_going: bool) -> Plan {
    let rm = |required, keep_going| {
        Step::Run(RunStep {
            command: Command::new(&["claude", "rm", "ab12"]),
            timeout_secs: 120,
            expect: Expect::ExitZero,
            required,
            keep_going,
            pause_after_ms: 0,
        })
    };
    Plan {
        steps: vec![
            rm(true, current_keeps_going),
            rm(false, false),
            rm(false, false),
        ],
        outcome: Outcome::Done,
    }
}

#[test]
fn best_effort_steps_do_not_stop_the_plan() {
    let fake = FakeLauncher::new();
    fake.push_output(Ok(LaunchOutput::ok("")));
    fake.push_output(Ok(LaunchOutput::failed(1, "already gone")));
    let report = execute(&rm_plan(true), &fake, &|_| {}).expect("ran");
    assert_eq!(report.ignored_failures, ["already gone"]);
    assert_eq!(fake.commands().len(), 3);
}

#[test]
fn a_failing_current_copy_is_the_error_after_every_copy_was_tried() {
    let failing = FakeLauncher::new();
    failing.push_output(Ok(LaunchOutput::failed(1, "denied")));
    failing.push_output(Ok(LaunchOutput::failed(1, "second failure")));
    assert_eq!(
        execute(&rm_plan(true), &failing, &|_| {}),
        Err(ControlError::Failed("denied".into())),
        "the first failure is the plan's, later ones don't replace it"
    );
    assert_eq!(failing.commands().len(), 3, "the stale copies were removed");
}

#[test]
fn a_required_step_without_keep_going_still_ends_the_plan() {
    let failing = FakeLauncher::new();
    failing.push_output(Ok(LaunchOutput::failed(1, "denied")));
    assert_eq!(
        execute(&rm_plan(false), &failing, &|_| {}),
        Err(ControlError::Failed("denied".into()))
    );
    assert_eq!(failing.commands().len(), 1);
}

#[test]
fn a_trust_script_that_does_not_play_stops_before_starting() {
    let plan = plan_new_session(&NewSession {
        folder: "/work/app",
        trust: true,
        ..NewSession::default()
    })
    .expect("plan");
    let fake = FakeLauncher::new();
    fake.push_pty(Ok(false));
    assert_eq!(
        execute(&plan, &fake, &|_| {}),
        Err(ControlError::TrustFailed)
    );
    assert!(fake.commands().is_empty());

    let good = FakeLauncher::new();
    good.push_output(Ok(LaunchOutput::ok("backgrounded · 0a1b2c3d (idle)")));
    let report = execute(&plan, &good, &|_| {}).expect("ran");
    assert_eq!(report.job_id.as_deref(), Some("0a1b2c3d"));
    assert_eq!(good.calls().len(), 2, "the script, then the start");
    assert!(matches!(good.calls()[0], launcher::Call::Pty(_)));
}
