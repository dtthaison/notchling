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
        &job("working", 0, "new turn"),
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

#[cfg(unix)]
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

#[test]
fn job_state_reads_the_job_file_and_knows_a_folder_without_one() {
    let dir = TempDir::new("fresh-state");
    assert_eq!(job_state(&roots(&dir), "be50d36a"), None);
    dir.write("claude/jobs/be50d36a/other.txt", "x");
    assert_eq!(job_state(&roots(&dir), "be50d36a").as_deref(), Some(""));
    dir.write("claude/jobs/be50d36a/state.json", &job("stopped", 0, "x"));
    assert_eq!(
        job_state(&roots(&dir), "be50d36a").as_deref(),
        Some("stopped")
    );
    for odd in ["", "..", "a/b"] {
        assert_eq!(job_state(&roots(&dir), odd), None, "{odd:?}");
    }
}

#[test]
fn job_status_is_the_live_session_files_status() {
    let dir = TempDir::new("fresh-status");
    dir.write("claude/jobs/be50d36a/state.json", &job("done", 0, "x"));
    let probes = Probes::from_proc();
    assert_eq!(job_status(&roots(&dir), &probes, "be50d36a"), None);
    dir.write("claude/sessions/1.json", &live("waiting"));
    assert_eq!(
        job_status(&roots(&dir), &probes, "be50d36a").as_deref(),
        Some("waiting")
    );
}

#[test]
fn a_stale_busy_job_status_reads_as_idle() {
    let dir = TempDir::new("fresh-stale-busy");
    let path = "claude/jobs/be50d36a/state.json";
    let mut state: serde_json::Value = serde_json::from_str(&job("done", 0, "x")).unwrap();
    state["updatedAt"] = json!("1970-01-01T00:05:01.000Z");
    dir.write(path, &state.to_string());
    let mut file: serde_json::Value = serde_json::from_str(&live("busy")).unwrap();
    file["statusUpdatedAt"] = json!(1_000.0);
    dir.write("claude/sessions/1.json", &file.to_string());
    let probes = Probes::from_proc();
    assert_eq!(
        job_status(&roots(&dir), &probes, "be50d36a").as_deref(),
        Some("idle")
    );
    file["statusUpdatedAt"] = json!(2_000.0);
    dir.write("claude/sessions/1.json", &file.to_string());
    assert_eq!(
        job_status(&roots(&dir), &probes, "be50d36a").as_deref(),
        Some("busy")
    );
    file["statusUpdatedAt"] = json!(1_000.0);
    dir.write("claude/sessions/1.json", &file.to_string());
    std::fs::remove_file(dir.path(path)).unwrap();
    assert_eq!(
        job_status(&roots(&dir), &probes, "be50d36a").as_deref(),
        Some("busy")
    );
}

#[test]
fn job_process_is_the_pid_and_start_the_files_record() {
    let dir = TempDir::new("fresh-process");
    dir.write("claude/jobs/be50d36a/state.json", &job("done", 0, "x"));
    assert_eq!(
        job_process(&roots(&dir), &Probes::from_proc(), "be50d36a"),
        None
    );
    let mut file: serde_json::Value = serde_json::from_str(&live("busy")).unwrap();
    file["startedAt"] = json!(1_794_000_000_000_i64);
    dir.write("claude/sessions/1.json", &file.to_string());
    assert_eq!(
        job_process(&roots(&dir), &Probes::from_proc(), "be50d36a"),
        Some((std::process::id(), Some(1_794_000_000_000)))
    );
}

#[cfg(unix)]
#[test]
fn job_process_ignores_a_session_file_of_a_dead_pid() {
    let dir = TempDir::new("fresh-process-dead");
    dir.write("claude/jobs/be50d36a/state.json", &job("done", 0, "x"));
    let mut old: serde_json::Value = serde_json::from_str(&live("busy")).unwrap();
    old["pid"] = json!(4_000_000_000_u64);
    old["startedAt"] = json!(1_i64);
    dir.write("claude/sessions/0.json", &old.to_string());
    assert_eq!(
        job_process(&roots(&dir), &Probes::from_proc(), "be50d36a"),
        None
    );
    let mut file: serde_json::Value = serde_json::from_str(&live("busy")).unwrap();
    file["startedAt"] = json!(2_i64);
    dir.write("claude/sessions/1.json", &file.to_string());
    assert_eq!(
        job_process(&roots(&dir), &Probes::from_proc(), "be50d36a"),
        Some((std::process::id(), Some(2)))
    );
}
