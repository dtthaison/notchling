use super::*;
use crate::sessions::testutil::TempDir;
use crate::sessions::{AgentGroup, Probes, Roots};
use serde_json::json;

fn roots(dir: &TempDir) -> Roots {
    Roots {
        main_dir: dir.path("claude"),
        lanes_file: dir.path("none/lanes.json"),
    }
}

fn job(state: &str, tasks: u64, detail: &str) -> String {
    json!({
        "state": state, "tempo": "idle", "detail": detail, "template": "bg", "name": "main",
        "inFlight": {"tasks": tasks, "queued": 0, "kinds": ["local_bash"], "drainableMonitors": 0},
        "sessionId": "be50d36a-4348-4ba6-ab58-96959399a893",
        "linkScanPath": "/nonexistent/fee7356b.jsonl", "cwd": "/Users/me/app",
        "updatedAt": "2026-11-14T22:54:49.559Z"
    })
    .to_string()
}

fn live(status: &str) -> String {
    json!({
        "pid": std::process::id(), "sessionId": "fee7356b-b891-4f6f-b4e7-35f7093b251c",
        "jobId": "be50d36a", "status": status, "statusUpdatedAt": 1_794_697_000_000.0
    })
    .to_string()
}

#[test]
fn a_row_is_read_again_from_the_job_and_live_files_each_time() {
    let dir = TempDir::new("fresh");
    dir.write(
        "claude/jobs/be50d36a/state.json",
        &job("blocked", 4, "asks"),
    );
    dir.write("claude/sessions/1.json", &live("busy"));
    let probes = Probes::from_proc();
    let first = job_row(&roots(&dir), &probes, "be50d36a").unwrap();
    assert_eq!(first.group, AgentGroup::Working);
    assert!(first.at_prompt);
    assert_eq!(first.detail.as_deref(), Some("asks"));
    dir.write(
        "claude/jobs/be50d36a/state.json",
        &job("working", 4, "new turn"),
    );
    let next = job_row(&roots(&dir), &probes, "be50d36a").unwrap();
    assert!(!next.at_prompt);
    assert_eq!(next.detail.as_deref(), Some("new turn"));
    dir.write("claude/sessions/1.json", &live("idle"));
    dir.write(
        "claude/jobs/be50d36a/state.json",
        &job("done", 0, "finished"),
    );
    let idle = job_row(&roots(&dir), &probes, "be50d36a").unwrap();
    assert_eq!(idle.group, AgentGroup::Completed);
}

#[test]
fn a_job_that_is_gone_or_a_name_that_is_no_job_gives_nothing() {
    let dir = TempDir::new("fresh-none");
    dir.write("claude/jobs/be50d36a/state.json", &job("done", 0, "x"));
    let probes = Probes::from_proc();
    assert!(job_row(&roots(&dir), &probes, "ffffffff").is_none());
    for odd in ["", "..", ".hidden", "a/b", "..\\x"] {
        assert!(job_row(&roots(&dir), &probes, odd).is_none(), "{odd:?}");
    }
    dir.write("claude/jobs/be50d36a/state.json", "{ half a wri");
    assert!(job_row(&roots(&dir), &probes, "be50d36a").is_none());
}

#[test]
fn a_session_file_of_a_dead_process_is_not_the_live_status() {
    let dir = TempDir::new("fresh-dead");
    dir.write(
        "claude/jobs/be50d36a/state.json",
        &job("blocked", 4, "asks"),
    );
    let dead = live("busy").replace(&std::process::id().to_string(), "99999999");
    dir.write("claude/sessions/1.json", &dead);
    let row = job_row(&roots(&dir), &Probes::from_proc(), "be50d36a").unwrap();
    assert!(!row.at_prompt);
}
