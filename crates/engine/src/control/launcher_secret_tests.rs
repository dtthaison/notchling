use super::*;
use crate::cluster::grant_files::GrantValue;
use std::time::Duration;

const MARKER: &str = "NL-MARKER-701";

fn with_token() -> Command {
    Command::new(&["claude", "--bg"])
        .with_secret_env(LOGIN_TOKEN_VAR, GrantValue::new(MARKER.into()))
}

#[test]
fn the_fake_records_the_key_and_never_the_value() {
    let fake = FakeLauncher::new();
    fake.run(&with_token(), Duration::from_secs(1)).unwrap();
    let shown = format!("{:?}", fake.calls());
    assert!(!shown.contains(MARKER), "the recorded call shows a value");
    let recorded = &fake.commands()[0];
    assert!(recorded.secret_env.contains_key(LOGIN_TOKEN_VAR));
    assert!(recorded.secret_env[LOGIN_TOKEN_VAR].expose().is_empty());
    assert_eq!(recorded.argv, ["claude", "--bg"]);
}

#[test]
fn a_debug_print_of_a_command_hides_the_value() {
    let shown = format!("{:?}", with_token());
    assert!(!shown.contains(MARKER), "a command printed its value");
    assert!(shown.contains(LOGIN_TOKEN_VAR));
}

#[test]
fn commands_with_different_values_differ() {
    let other = Command::new(&["claude", "--bg"])
        .with_secret_env(LOGIN_TOKEN_VAR, GrantValue::new("NL-MARKER-702".into()));
    assert!(with_token() != other);
    assert!(with_token() == with_token());
}

fn step(argv: &[&str], expect: Expect) -> Step {
    Step::Run(RunStep {
        command: Command::new(argv),
        timeout_secs: 5,
        expect,
        required: true,
        keep_going: false,
        pause_after_ms: 0,
    })
}

#[test]
fn only_claude_steps_of_a_plan_get_the_token() {
    let plan = Plan {
        steps: vec![
            step(&["git", "status"], Expect::ExitZero),
            step(&["claude", "--bg", "x"], Expect::JobId),
        ],
        outcome: Outcome::NewJob,
    };
    let got = plan.with_claude_secret_env(LOGIN_TOKEN_VAR, &GrantValue::new(MARKER.into()));
    let counts: Vec<usize> = got
        .steps
        .iter()
        .map(|s| match s {
            Step::Run(r) => r.command.secret_env.len(),
            Step::Pty(_) => 99,
        })
        .collect();
    assert_eq!(counts, [0, 1]);
    assert!(
        !format!("{got:?}").contains(MARKER),
        "a plan printed a value"
    );
}

#[cfg(unix)]
#[test]
fn the_real_launcher_sets_the_value_on_the_child_only() {
    let command = Command::new(&[
        "/bin/sh",
        "-c",
        "test \"$CLAUDE_CODE_OAUTH_TOKEN\" = NL-MARKER-701 && echo has",
    ])
    .with_secret_env(LOGIN_TOKEN_VAR, GrantValue::new(MARKER.into()));
    let launcher = RealLauncher::with_claude(std::path::PathBuf::from("/nonexistent/claude"));
    let got = launcher.run(&command, Duration::from_secs(10)).unwrap();
    assert_eq!(got.output.trim(), "has");
}
