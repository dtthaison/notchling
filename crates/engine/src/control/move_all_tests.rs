//! Ports of `MoveAllPlanTests`, plus the stop and resume plans built from a move.

use super::super::plan::{Command, Expect, Step};
use super::*;
use crate::sessions::agent::sample;
use crate::sessions::testutil::TempDir;

fn lane(name: &str, path: &str, account: &str) -> Lane {
    Lane {
        name: name.into(),
        path: path.into(),
        account_id: account.into(),
        email: format!("{name}@x.com"),
        folder: None,
    }
}

fn agent(id: &str, dir: Option<&str>, target: AgentTarget, label: &str) -> Agent {
    let mut a = sample(target, Some("/t/x.jsonl"));
    a.id = id.into();
    a.name = id.into();
    a.cwd = "/p".into();
    a.label = label.into();
    a.config_dir = dir.map(String::from);
    a
}

fn job(id: &str, dir: Option<&str>, job_id: &str) -> Agent {
    agent(id, dir, AgentTarget::Job(job_id.into()), "Working")
}

fn job_ids(plan: &MoveAllPlan) -> Vec<&str> {
    plan.jobs.iter().map(|j| j.job_id.as_str()).collect()
}

// Port of MoveAllPlanTests.testAccountInALane.
#[test]
fn account_in_a_lane() {
    let lanes = [
        lane("a", "/lanes/a", "acct-a"),
        lane("b", "/lanes/b", "acct-b"),
    ];
    let agents = [
        job("main-job", None, "m1"),
        job("a-job", Some("/lanes/a"), "a1"),
        agent(
            "a-stopped",
            Some("/lanes/a"),
            AgentTarget::Job("a2".into()),
            "Stopped",
        ),
        agent(
            "a-term",
            Some("/lanes/a"),
            AgentTarget::Process(42),
            "Working",
        ),
        job("b-job", Some("/lanes/b"), "b1"),
        agent(
            "b-term",
            Some("/lanes/b"),
            AgentTarget::Process(43),
            "Working",
        ),
    ];
    let plan = MoveAllPlan::make(&agents, &lanes, Some("main"), Some("acct-a"));
    assert_eq!(job_ids(&plan), ["a1", "b1"]);
    assert_eq!(plan.close_lane.as_deref(), Some("a"));
    assert_eq!(plan.switch_main.as_deref(), Some("acct-a"));
    assert_eq!(plan.signed_out, ["a-term"]);
}

// Port of MoveAllPlanTests.testMainAccountOnlyGathersLaneSessions.
#[test]
fn main_account_only_gathers_lane_sessions() {
    let lanes = [
        lane("a", "/lanes/a", "acct-a"),
        lane("b", "/lanes/b", "acct-b"),
    ];
    let agents = [job("b-job", Some("/lanes/b"), "b1")];
    let plan = MoveAllPlan::make(&agents, &lanes, Some("main"), Some("main"));
    assert_eq!(job_ids(&plan), ["b1"]);
    assert_eq!(plan.close_lane, None);
    assert_eq!(plan.switch_main, None);
}

// Port of MoveAllPlanTests.testNothingToDo.
#[test]
fn nothing_to_do() {
    let lanes = [lane("a", "/lanes/a", "acct-a")];
    let agents = [job("main-job", None, "m1")];
    assert!(MoveAllPlan::make(&agents, &lanes, Some("main"), None).is_empty());
    // A free account: only the main login switches.
    let free = MoveAllPlan::make(&agents, &lanes, Some("main"), Some("acct-free"));
    assert_eq!(free.switch_main.as_deref(), Some("acct-free"));
    assert_eq!(free.close_lane, None);
    assert!(free.jobs.is_empty());
}

#[test]
fn a_job_without_a_transcript_stays() {
    let lanes = [lane("b", "/lanes/b", "acct-b")];
    let mut no_transcript = job("b-job", Some("/lanes/b"), "b1");
    no_transcript.transcript_path = None;
    let plan = MoveAllPlan::make(&[no_transcript], &lanes, Some("main"), None);
    assert!(plan.jobs.is_empty());
}

#[test]
fn each_job_stops_in_its_lane_and_resumes_on_the_main_login_with_its_flags() {
    let tmp = TempDir::new("moveall");
    // The lane folder is a temp one, so lanes.json points at it.
    let lane_dir = tmp.path("lane-b");
    let lane_path = lane_dir.to_string_lossy().into_owned();
    tmp.write(
        "data/lanes.json",
        &format!(
            r#"[{{"name":"b","path":{},"accountID":"acct-b","email":"b@x.com"}}]"#,
            serde_json::to_string(&lane_path).expect("json")
        ),
    );
    tmp.write(
        "lane-b/jobs/b1/state.json",
        r#"{"state":"working","respawnFlags":["--model","opus","--bg","--resume","old"]}"#,
    );
    let roots = Roots::new(&tmp.path("home"), &tmp.path("data"));
    let plan = MoveAllPlan {
        jobs: vec![MoveJob {
            agent_id: "b-job".into(),
            job_id: "b1".into(),
            name: "fix".into(),
            cwd: "/work".into(),
            transcript: "/h/.claude/projects/p/1f2e-uuid.jsonl".into(),
        }],
        ..MoveAllPlan::default()
    };
    let moves = plan_move_all_jobs(&plan, &roots);
    assert_eq!(moves.len(), 1);
    let Step::Run(stop) = &moves[0].stop.steps[0] else {
        panic!("a run step");
    };
    assert_eq!(stop.command.argv, ["claude", "stop", "b1"]);
    assert_eq!(
        stop.command.env.get("CLAUDE_CONFIG_DIR"),
        Some(&Some(lane_path))
    );
    let Step::Run(resume) = &moves[0].resume.steps[0] else {
        panic!("a run step");
    };
    assert_eq!(
        resume.command,
        Command::new(&["claude", "--model", "opus", "--bg", "--resume", "1f2e-uuid"])
            .in_dir("/work")
            .for_config_dir(None)
    );
    assert_eq!(resume.expect, Expect::JobId);
}
