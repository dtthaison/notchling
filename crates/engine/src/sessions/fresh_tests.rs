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

fn wall_ms() -> i64 {
    let since = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap();
    i64::try_from(since.as_millis()).unwrap()
}

fn began_a_minute_ago(pid: u32) -> Option<i64> {
    (pid == std::process::id()).then(|| wall_ms() - 60_000)
}

fn look_at(now_ms: i64) -> Look<'static> {
    Look {
        alive: Probes::from_proc().is_alive,
        started_ms: &began_a_minute_ago,
        now_ms,
    }
}

fn ended_in(roots: &Roots) -> Ended {
    job_ended(roots, &look_at(wall_ms()), "be50d36a")
}

fn ended(dir: &TempDir) -> Ended {
    ended_in(&roots(dir))
}

#[test]
fn a_stopped_job_with_no_live_process_has_really_ended() {
    let dir = TempDir::new("fresh-ended-stopped");
    dir.write("claude/jobs/be50d36a/state.json", &job("stopped", 0, "x"));
    assert_eq!(ended(&dir), Ended::Stopped);
    let dead = live("idle").replace(&std::process::id().to_string(), "99999999");
    dir.write("claude/sessions/1.json", &dead);
    assert_eq!(ended(&dir), Ended::Stopped);
}

#[test]
fn a_stopped_job_whose_process_is_alive_is_unknown_not_ended() {
    let dir = TempDir::new("fresh-ended-alive");
    dir.write("claude/jobs/be50d36a/state.json", &job("stopped", 0, "x"));
    dir.write("claude/sessions/1.json", &live("busy"));
    assert_eq!(ended(&dir), Ended::Unknown);
}

#[test]
fn a_stopped_job_is_unknown_while_a_session_file_is_caught_mid_write() {
    let dir = TempDir::new("fresh-ended-torn-session");
    dir.write("claude/jobs/be50d36a/state.json", &job("stopped", 0, "x"));
    let dead = live("idle").replace(&std::process::id().to_string(), "99999999");
    dir.write("claude/sessions/1.json", &dead);
    assert_eq!(ended(&dir), Ended::Stopped);
    let mine = format!("claude/sessions/{}.json", std::process::id());
    dir.write(&mine, "{\"pid\": 12");
    assert_eq!(ended(&dir), Ended::Unknown);
    dir.write(&mine, &dead);
    assert_eq!(ended(&dir), Ended::Stopped);
}

#[test]
fn a_session_file_of_a_dead_process_that_cannot_be_read_does_not_hold_a_stop_back() {
    let dir = TempDir::new("fresh-ended-torn-dead");
    dir.write("claude/jobs/be50d36a/state.json", &job("stopped", 0, "x"));
    dir.write("claude/sessions/99999999.json", "{\"pid\": 99999");
    assert_eq!(ended(&dir), Ended::Stopped);
    dir.write("claude/sessions/odd-name.json", "{\"pid\": 12");
    assert_eq!(ended(&dir), Ended::Unknown);
}

#[test]
fn a_done_job_with_a_session_file_caught_mid_write_is_unknown_not_done() {
    let dir = TempDir::new("fresh-ended-done-torn");
    dir.write(
        "claude/jobs/be50d36a/state.json",
        &job("done", 0, "finished"),
    );
    assert_eq!(ended(&dir), Ended::Done);
    let mine = format!("claude/sessions/{}.json", std::process::id());
    dir.write(&mine, "{\"pid\": 12");
    assert_eq!(ended(&dir), Ended::Unknown);
    dir.write("claude/sessions/99999999.json", "{\"pid\": 99999");
    dir.write(&mine, &live("idle"));
    assert_eq!(ended(&dir), Ended::No);
}

fn two_lanes(dir: &TempDir) -> Roots {
    let lane = dir.path("lane-b");
    let lanes = json!([{
        "name": "b", "path": lane.to_string_lossy(), "accountID": "a2", "email": "b@example.com"
    }]);
    dir.write("lanes.json", &lanes.to_string());
    Roots {
        main_dir: dir.path("claude"),
        lanes_file: dir.path("lanes.json"),
    }
}

fn aged(dir: &TempDir, relative: &str, secs: u64) {
    let file = std::fs::OpenOptions::new()
        .write(true)
        .open(dir.path(relative))
        .unwrap();
    file.set_modified(std::time::UNIX_EPOCH + std::time::Duration::from_secs(secs))
        .unwrap();
}

#[test]
fn a_done_job_whose_newest_state_copy_cannot_be_read_is_unknown_not_done() {
    let dir = TempDir::new("fresh-ended-done-copy");
    let roots = two_lanes(&dir);
    dir.write("claude/jobs/be50d36a/state.json", &job("done", 0, "old"));
    assert_eq!(ended_in(&roots), Ended::Done);
    dir.write("lane-b/jobs/be50d36a/state.json", "{ half a wri");
    assert_eq!(ended_in(&roots), Ended::Unknown);
}

#[test]
fn a_state_copy_older_than_the_newest_one_that_cannot_be_read_is_ignored() {
    let dir = TempDir::new("fresh-ended-old-copy");
    let roots = two_lanes(&dir);
    dir.write("claude/jobs/be50d36a/state.json", &job("stopped", 0, "new"));
    dir.write("lane-b/jobs/be50d36a/state.json", "{ half a wri");
    assert_eq!(ended_in(&roots), Ended::Unknown);
    aged(&dir, "lane-b/jobs/be50d36a/state.json", 1_000);
    assert_eq!(ended_in(&roots), Ended::Stopped);
}

#[test]
fn a_state_file_caught_mid_write_is_unknown_and_the_finished_one_decides() {
    let dir = TempDir::new("fresh-ended-torn");
    dir.write("claude/jobs/be50d36a/state.json", "{ half a wri");
    assert_eq!(ended(&dir), Ended::Unknown);
    dir.write("claude/jobs/be50d36a/state.json", "");
    assert_eq!(ended(&dir), Ended::Unknown);
    dir.write("claude/jobs/be50d36a/state.json", &job("stopped", 0, "x"));
    assert_eq!(ended(&dir), Ended::Stopped);
    dir.write("claude/jobs/be50d36a/state.json", &job("working", 0, "x"));
    assert_eq!(ended(&dir), Ended::No);
}

#[test]
fn a_stale_stopped_copy_is_not_the_answer_while_the_live_copy_cannot_be_read() {
    let dir = TempDir::new("fresh-ended-copies");
    let lane = dir.path("lane-b");
    let lanes = json!([{
        "name": "b", "path": lane.to_string_lossy(), "accountID": "a2", "email": "b@example.com"
    }]);
    dir.write("lanes.json", &lanes.to_string());
    let roots = Roots {
        main_dir: dir.path("claude"),
        lanes_file: dir.path("lanes.json"),
    };
    dir.write("claude/jobs/be50d36a/state.json", &job("stopped", 0, "old"));
    dir.write("lane-b/jobs/be50d36a/state.json", "{ half a wri");
    assert_eq!(ended_in(&roots), Ended::Unknown);
    dir.write("lane-b/jobs/be50d36a/state.json", &job("working", 0, "now"));
    assert_eq!(ended_in(&roots), Ended::No);
}

#[test]
fn a_done_job_with_no_process_is_done_and_with_a_process_is_not_ended() {
    let dir = TempDir::new("fresh-ended-done");
    dir.write(
        "claude/jobs/be50d36a/state.json",
        &job("done", 0, "finished"),
    );
    assert_eq!(ended(&dir), Ended::Done);
    dir.write("claude/sessions/1.json", &live("idle"));
    assert_eq!(ended(&dir), Ended::No);
}

#[test]
fn a_live_job_or_no_job_has_not_ended() {
    let dir = TempDir::new("fresh-ended-none");
    assert_eq!(ended(&dir), Ended::No);
    dir.write("claude/jobs/be50d36a/state.json", &job("working", 2, "x"));
    assert_eq!(ended(&dir), Ended::No);
    for odd in ["", "..", ".hidden", "a/b"] {
        assert_eq!(
            job_ended(&roots(&dir), &look_at(wall_ms()), odd),
            Ended::No,
            "{odd:?}"
        );
    }
}

fn aged_dir(dir: &TempDir, relative: &str, secs: u64) {
    let folder = std::fs::File::open(dir.path(relative)).unwrap();
    folder
        .set_modified(std::time::UNIX_EPOCH + std::time::Duration::from_secs(secs))
        .unwrap();
}

const DAYS_AFTER_THE_UPDATE: u64 = 1_794_800_000;

#[test]
fn a_job_folder_left_without_a_state_file_does_not_hold_a_stop_back_for_good() {
    let dir = TempDir::new("fresh-ended-no-state");
    let roots = two_lanes(&dir);
    dir.write("claude/jobs/be50d36a/state.json", &job("stopped", 0, "x"));
    dir.write("lane-b/jobs/be50d36a/timeline.jsonl", "");
    aged_dir(&dir, "lane-b/jobs/be50d36a", 1_000);
    assert_eq!(ended_in(&roots), Ended::Stopped);
}

#[test]
fn a_broken_state_copy_written_long_ago_does_not_hold_a_stop_back_for_good() {
    let dir = TempDir::new("fresh-ended-old-broken");
    let roots = two_lanes(&dir);
    dir.write("claude/jobs/be50d36a/state.json", &job("stopped", 0, "x"));
    dir.write("lane-b/jobs/be50d36a/state.json", "{ half a wri");
    aged(
        &dir,
        "lane-b/jobs/be50d36a/state.json",
        DAYS_AFTER_THE_UPDATE,
    );
    aged_dir(&dir, "lane-b/jobs/be50d36a", DAYS_AFTER_THE_UPDATE);
    assert_eq!(ended_in(&roots), Ended::Stopped);
}

#[test]
fn a_lone_state_file_broken_long_ago_with_no_process_is_stopped() {
    let dir = TempDir::new("fresh-ended-lone-broken");
    dir.write("claude/jobs/be50d36a/state.json", "{ half a wri");
    aged(&dir, "claude/jobs/be50d36a/state.json", 1_000);
    aged_dir(&dir, "claude/jobs/be50d36a", 1_000);
    assert_eq!(ended(&dir), Ended::Stopped);
    dir.write("claude/sessions/1.json", &live("idle"));
    assert_eq!(ended(&dir), Ended::No);
}

#[test]
fn a_broken_session_file_written_long_ago_does_not_hold_a_stop_back_for_good() {
    let dir = TempDir::new("fresh-ended-old-torn");
    dir.write("claude/jobs/be50d36a/state.json", &job("stopped", 0, "x"));
    dir.write("claude/sessions/odd-name.json", "{\"pid\": 12");
    assert_eq!(ended(&dir), Ended::Unknown);
    aged(&dir, "claude/sessions/odd-name.json", 1_000);
    assert_eq!(ended(&dir), Ended::Stopped);
    let reused = format!("claude/sessions/{}.json", std::process::id());
    dir.write(&reused, "{\"pid\": 12");
    aged(&dir, &reused, 1_000);
    assert_eq!(ended(&dir), Ended::Stopped);
}

#[test]
fn a_session_file_whose_pid_now_runs_a_newer_process_is_no_live_process() {
    let dir = TempDir::new("fresh-ended-reused-pid");
    dir.write("claude/jobs/be50d36a/state.json", &job("stopped", 0, "x"));
    let mut file: serde_json::Value = serde_json::from_str(&live("idle")).unwrap();
    file["startedAt"] = json!(1_000_000_i64);
    dir.write("claude/sessions/1.json", &file.to_string());
    assert_eq!(ended(&dir), Ended::Stopped);
    file["startedAt"] = json!(wall_ms() - 59_000);
    dir.write("claude/sessions/1.json", &file.to_string());
    assert_eq!(
        ended(&dir),
        Ended::Unknown,
        "its own process, started first"
    );
    file["startedAt"] = json!("2026-11-14T22:54:49.559Z");
    dir.write("claude/sessions/1.json", &file.to_string());
    assert_eq!(ended(&dir), Ended::Stopped, "an ISO start is read too");
    let unknown_start = |_pid: u32| -> Option<i64> { None };
    let look = Look {
        started_ms: &unknown_start,
        ..look_at(wall_ms())
    };
    assert_eq!(
        job_ended(&roots(&dir), &look, "be50d36a"),
        Ended::Unknown,
        "a process whose start cannot be read is taken as alive"
    );
}

const AT: i64 = 1_800_000_000_000;

#[test]
fn a_file_that_cannot_be_read_holds_a_stop_back_only_while_it_was_written_just_now() {
    let dir = TempDir::new("fresh-ended-window");
    dir.write("claude/jobs/be50d36a/state.json", "{ half a wri");
    let secs = u64::try_from(AT / 1_000).unwrap();
    aged(&dir, "claude/jobs/be50d36a/state.json", secs);
    let at = |now_ms: i64| job_ended(&roots(&dir), &look_at(now_ms), "be50d36a");
    assert_eq!(at(AT), Ended::Unknown);
    assert_eq!(at(AT + MID_WRITE_MS - 1), Ended::Unknown);
    assert_eq!(at(AT + MID_WRITE_MS), Ended::Stopped);
    assert_eq!(at(AT - MID_WRITE_MS), Ended::Stopped, "a time far ahead");
    dir.write("claude/jobs/be50d36a/state.json", &job("done", 0, "x"));
    dir.write("claude/sessions/odd.json", "{\"pid\": 12");
    aged(&dir, "claude/sessions/odd.json", secs);
    assert_eq!(at(AT + 1_000), Ended::Unknown);
    assert_eq!(at(AT + MID_WRITE_MS), Ended::Done);
}

#[test]
fn a_job_folder_made_just_now_without_its_state_file_is_unknown_for_a_moment() {
    let dir = TempDir::new("fresh-ended-new-folder");
    dir.write("claude/jobs/be50d36a/timeline.jsonl", "");
    let secs = u64::try_from(AT / 1_000).unwrap();
    aged_dir(&dir, "claude/jobs/be50d36a", secs);
    let at = |now_ms: i64| job_ended(&roots(&dir), &look_at(now_ms), "be50d36a");
    assert_eq!(at(AT + 1_000), Ended::Unknown);
    assert_eq!(at(AT + MID_WRITE_MS), Ended::Stopped);
}
