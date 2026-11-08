//! The session planners against a temp `~/.claude` and lanes, nothing run.

use super::*;
use crate::sessions::agent::sample;
use crate::sessions::testutil::TempDir;
use crate::sessions::{Agent, AgentTarget, Roots};

struct Rig {
    tmp: TempDir,
    roots: Roots,
    lane: String,
}

/// A main folder and one lane folder, both under a temp root.
fn rig(tag: &str) -> Rig {
    let tmp = TempDir::new(tag);
    let lane = tmp.path("lane-b").to_string_lossy().into_owned();
    tmp.write(
        "data/lanes.json",
        &format!(
            r#"[{{"name":"b","path":{},"accountID":"acct-b","email":"b@x.com"}}]"#,
            serde_json::to_string(&lane).expect("json")
        ),
    );
    let roots = Roots::new(&tmp.path("home"), &tmp.path("data"));
    Rig { tmp, roots, lane }
}

fn job(id: &str) -> Agent {
    let mut a = sample(
        AgentTarget::Job(id.into()),
        Some("/h/.claude/projects/p/1f2e-uuid.jsonl"),
    );
    a.name = "fix".into();
    a.cwd = "/work".into();
    a
}

fn runs(plan: &Plan) -> Vec<&RunStep> {
    plan.steps
        .iter()
        .filter_map(|s| match s {
            Step::Run(r) => Some(r),
            Step::Pty(_) => None,
        })
        .collect()
}

fn config(command: &Command) -> Option<Option<String>> {
    command.env.get(CONFIG_DIR_VAR).cloned()
}

#[test]
fn stop_runs_claude_stop_in_the_jobs_lane() {
    let r = rig("stop");
    r.tmp.write(
        "lane-b/jobs/ab12/state.json",
        r#"{"state":"working","updatedAt":"2026-11-04T03:00:00.000Z"}"#,
    );
    let plan = plan_stop(&job("ab12"), &r.roots).expect("plan");
    let steps = runs(&plan);
    assert_eq!(steps.len(), 1);
    assert_eq!(steps[0].command.argv, ["claude", "stop", "ab12"]);
    assert_eq!(config(&steps[0].command), Some(Some(r.lane.clone())));
    assert_eq!(steps[0].expect, Expect::ExitZero);
    assert_eq!(steps[0].timeout_secs, 60);
    assert_eq!(steps[0].pause_after_ms, 1500);
    assert_eq!(plan.outcome, Outcome::Done);

    // A main-login job removes the variable so it cannot inherit a lane's.
    r.tmp
        .write("home/.claude/jobs/m1/state.json", r#"{"state":"working"}"#);
    let main = plan_stop(&job("m1"), &r.roots).expect("plan");
    assert_eq!(config(&runs(&main)[0].command), Some(None));
}

#[test]
fn only_background_jobs_are_controlled() {
    let r = rig("notjob");
    let terminal = sample(AgentTarget::Process(5), None);
    assert_eq!(plan_stop(&terminal, &r.roots), Err(PlanError::NotAJob));
    assert_eq!(plan_remove(&terminal, &r.roots), Err(PlanError::NotAJob));
    assert_eq!(
        plan_move(&terminal, None, &r.roots),
        Err(PlanError::NotAJob)
    );
    let mut no_transcript = job("ab12");
    no_transcript.transcript_path = None;
    assert_eq!(
        plan_move(&no_transcript, None, &r.roots),
        Err(PlanError::NoTranscript)
    );
}

#[test]
fn remove_runs_rm_on_every_copy_and_only_the_current_one_counts() {
    let r = rig("remove");
    // Moved to the lane: a stopped copy stays in the main folder.
    r.tmp.write(
        "home/.claude/jobs/ab12/state.json",
        r#"{"state":"stopped","updatedAt":"2026-11-02T08:00:00.000Z"}"#,
    );
    r.tmp.write(
        "lane-b/jobs/ab12/state.json",
        r#"{"state":"working","updatedAt":"2026-11-03T08:00:00.000Z"}"#,
    );
    let plan = plan_remove(&job("ab12"), &r.roots).expect("plan");
    let steps = runs(&plan);
    assert_eq!(steps.len(), 2);
    assert_eq!(steps[0].command.argv, ["claude", "rm", "ab12"]);
    assert_eq!(config(&steps[0].command), Some(Some(r.lane.clone())));
    assert!(steps[0].required && steps[0].keep_going);
    assert_eq!(config(&steps[1].command), Some(None));
    assert!(!steps[1].required);
}

/// Swift's remove loop runs `claude rm` in every folder and keeps only the current copy's result.
fn moved_job(tag: &str) -> Rig {
    let r = rig(tag);
    r.tmp.write(
        "home/.claude/jobs/ab12/state.json",
        r#"{"state":"stopped","updatedAt":"2026-11-02T08:00:00.000Z"}"#,
    );
    r.tmp.write(
        "lane-b/jobs/ab12/state.json",
        r#"{"state":"working","updatedAt":"2026-11-03T08:00:00.000Z"}"#,
    );
    r
}

#[test]
fn a_failed_current_copy_still_lets_the_stale_copy_go() {
    let r = moved_job("remove-current-fails");
    let plan = plan_remove(&job("ab12"), &r.roots).expect("plan");
    let fake = FakeLauncher::new();
    fake.push_output(Ok(LaunchOutput::failed(1, "denied")));
    let result = execute(&plan, &fake, &|_| {});
    assert_eq!(result, Err(ControlError::Failed("denied".into())));
    let commands = fake.commands();
    assert_eq!(commands.len(), 2, "the stale copy is removed anyway");
    assert_eq!(config(&commands[1]), Some(None));
}

#[test]
fn a_failed_stale_copy_is_not_the_removes_failure() {
    let r = moved_job("remove-stale-fails");
    let plan = plan_remove(&job("ab12"), &r.roots).expect("plan");
    let fake = FakeLauncher::new();
    fake.push_output(Ok(LaunchOutput::ok("")));
    fake.push_output(Ok(LaunchOutput::failed(1, "already gone")));
    let report = execute(&plan, &fake, &|_| {}).expect("the current copy went");
    assert_eq!(report.ignored_failures, ["already gone"]);
    assert_eq!(fake.commands().len(), 2);
}

#[test]
fn remove_of_a_job_with_no_files_still_tries_the_main_login() {
    let r = rig("remove-none");
    let plan = plan_remove(&job("zz99"), &r.roots).expect("plan");
    assert_eq!(runs(&plan).len(), 1);
    assert_eq!(config(&runs(&plan)[0].command), Some(None));
}

#[test]
fn new_session_starts_bg_in_the_folder_with_the_lanes_config_dir() {
    let plan = plan_new_session(&NewSession {
        folder: "/work/app",
        config_dir: Some("/lanes/b"),
        flags: &[],
        trust: false,
    })
    .expect("plan");
    let steps = runs(&plan);
    assert_eq!(plan.steps.len(), 1);
    assert_eq!(steps[0].command.argv, ["claude", "--bg"]);
    assert_eq!(steps[0].command.cwd.as_deref(), Some("/work/app"));
    assert_eq!(config(&steps[0].command), Some(Some("/lanes/b".into())));
    assert_eq!(steps[0].expect, Expect::JobId);
    assert_eq!(plan.outcome, Outcome::NewJob);

    let main = plan_new_session(&NewSession {
        folder: "/w",
        flags: &["--model".to_string(), "opus".to_string()],
        ..NewSession::default()
    })
    .expect("plan");
    assert_eq!(
        runs(&main)[0].command.argv,
        ["claude", "--model", "opus", "--bg"]
    );
    assert_eq!(config(&runs(&main)[0].command), Some(None));
    assert_eq!(
        plan_new_session(&NewSession::default()),
        Err(PlanError::NoFolder)
    );
}

#[test]
fn trust_puts_the_hidden_pty_script_first() {
    let plan = plan_new_session(&NewSession {
        folder: "/work/app",
        config_dir: Some("/lanes/b"),
        flags: &[],
        trust: true,
    })
    .expect("plan");
    assert_eq!(plan.steps.len(), 2);
    let Step::Pty(script) = &plan.steps[0] else {
        panic!("the trust script comes first");
    };
    assert_eq!(script.command.cwd.as_deref(), Some("/work/app"));
    assert_eq!(config(&script.command), Some(Some("/lanes/b".into())));
    assert!(matches!(plan.steps[1], Step::Run(_)));
}

#[test]
fn move_stops_in_the_old_lane_then_resumes_in_the_target_with_the_jobs_flags() {
    let r = rig("move");
    r.tmp.write(
        "home/.claude/jobs/ab12/state.json",
        r#"{"state":"working","respawnFlags":["--model","opus","--bg","--resume","old","--permission-mode","plan"]}"#,
    );
    let plan = plan_move(&job("ab12"), Some(&r.lane), &r.roots).expect("plan");
    let steps = runs(&plan);
    assert_eq!(steps.len(), 2);
    assert_eq!(steps[0].command.argv, ["claude", "stop", "ab12"]);
    assert_eq!(
        config(&steps[0].command),
        Some(None),
        "stopped where it lives now"
    );
    assert_eq!(
        steps[1].command.argv,
        [
            "claude",
            "--model",
            "opus",
            "--permission-mode",
            "plan",
            "--bg",
            "--resume",
            "1f2e-uuid"
        ]
    );
    assert_eq!(steps[1].command.cwd.as_deref(), Some("/work"));
    assert_eq!(config(&steps[1].command), Some(Some(r.lane.clone())));
    assert_eq!(steps[1].expect, Expect::JobId);
    assert_eq!(plan.outcome, Outcome::NewJob);
}

#[test]
fn moving_back_where_a_copy_exists_sends_no_flags() {
    let r = rig("move-back");
    r.tmp.write(
        "home/.claude/jobs/ab12/state.json",
        r#"{"state":"stopped","updatedAt":"2026-11-02T08:00:00.000Z","respawnFlags":["--model","opus"]}"#,
    );
    r.tmp.write(
        "lane-b/jobs/ab12/state.json",
        r#"{"state":"working","updatedAt":"2026-11-03T08:00:00.000Z","respawnFlags":["--model","opus"]}"#,
    );
    let plan = plan_move(&job("ab12"), None, &r.roots).expect("plan");
    let steps = runs(&plan);
    assert_eq!(
        config(&steps[0].command),
        Some(Some(r.lane.clone())),
        "stopped in the lane"
    );
    assert_eq!(
        steps[1].command.argv,
        ["claude", "--bg", "--resume", "1f2e-uuid"]
    );
    assert_eq!(config(&steps[1].command), Some(None));
}

#[test]
fn a_job_without_state_keeps_its_name_and_an_empty_cwd_means_home() {
    let r = rig("move-name");
    let mut agent = job("ab12");
    agent.cwd = String::new();
    let plan = plan_move(&agent, None, &r.roots).expect("plan");
    let start = runs(&plan)[1];
    assert_eq!(
        start.command.argv,
        ["claude", "--name", "fix", "--bg", "--resume", "1f2e-uuid"]
    );
    assert_eq!(
        start.command.cwd.as_deref(),
        Some(r.tmp.path("home").to_string_lossy().as_ref())
    );
}
