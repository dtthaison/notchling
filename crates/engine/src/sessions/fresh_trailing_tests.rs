use super::*;
use crate::sessions::testutil::TempDir;
use serde_json::json;

const JOB: &str = "3dc1c4be";
const PID: u32 = 91_648;
const BEGAN_MS: i64 = 1_795_168_333_000;
const BEGAN: &str = "Fri Nov 20 09:52:13 2026";
const STATE: &str = "claude/jobs/3dc1c4be/state.json";
const SESSION: &str = "claude/sessions/91648.json";
const RUNS: Process = Process::Ours {
    started_ms: Some(BEGAN_MS),
    ticks: None,
};

fn roots(dir: &TempDir) -> Roots {
    Roots {
        main_dir: dir.path("claude"),
        lanes_file: dir.path("lanes.json"),
    }
}

fn session_text() -> String {
    json!({
        "pid": PID,
        "sessionId": "0b0f4d4e-6d3e-4f0a-9d3a-5a1f0c2b7e11",
        "jobId": JOB,
        "status": "busy",
        "procStart": BEGAN,
        "startedAt": BEGAN_MS + 1_600,
        "cwd": "/tmp/work",
        "formerNames": [{"name": "old", "at": 1}]
    })
    .to_string()
}

fn job_text(state: &str) -> String {
    json!({"state": state, "updatedAt": "2026-11-14T22:54:49.559Z"}).to_string()
}

fn ended(dir: &TempDir) -> Ended {
    let process = |pid: u32| if pid == PID { RUNS } else { Process::Gone };
    let settled = |_: &Mark| false;
    let look = Look {
        process: &process,
        settled: &settled,
    };
    job_ended(&roots(dir), &look, JOB)
}

fn alive(pid: u32) -> bool {
    pid == PID
}

fn nobody(_: u32) -> Option<String> {
    None
}

#[test]
fn a_done_job_whose_live_session_file_has_a_stray_closing_brace_is_not_unknown() {
    let dir = TempDir::new("trailing-live");
    dir.write(STATE, &job_text("done"));
    dir.write(SESSION, &format!("{}}}", session_text()));
    assert_eq!(ended(&dir), Ended::No);
}

#[test]
fn a_session_file_with_a_longer_earlier_tail_still_names_its_job() {
    let dir = TempDir::new("trailing-tail");
    dir.write(STATE, &job_text("done"));
    dir.write(SESSION, &format!("{}}}\n]\n ", session_text()));
    assert_eq!(ended(&dir), Ended::No);
}

#[test]
fn a_truncated_live_session_file_keeps_the_verdict_unknown() {
    let dir = TempDir::new("trailing-torn");
    dir.write(STATE, &job_text("done"));
    let whole = session_text();
    dir.write(SESSION, &whole[..whole.len() - 12]);
    assert_eq!(ended(&dir), Ended::Unknown);
}

#[test]
fn a_session_file_with_a_second_object_after_the_first_is_unreadable() {
    let dir = TempDir::new("trailing-two");
    dir.write(STATE, &job_text("done"));
    dir.write(SESSION, &format!("{}{}", session_text(), session_text()));
    assert_eq!(ended(&dir), Ended::Unknown);
}

#[test]
fn a_state_file_with_a_stray_closing_brace_is_read_for_its_state() {
    let dir = TempDir::new("trailing-state");
    dir.write(STATE, &format!("{}}}", job_text("done")));
    dir.write(SESSION, &format!("{}}}", session_text()));
    assert_eq!(ended(&dir), Ended::No);
    assert_eq!(job_state(&roots(&dir), JOB).as_deref(), Some("done"));
    let probes = Probes {
        is_alive: &alive,
        host_app: &nobody,
    };
    assert!(job_row(&roots(&dir), &probes, JOB).is_some());
}

#[test]
fn the_live_status_comes_from_a_session_file_with_a_stray_closing_brace() {
    let dir = TempDir::new("trailing-status");
    dir.write(STATE, &job_text("working"));
    dir.write(SESSION, &format!("{}}}", session_text()));
    let probes = Probes {
        is_alive: &alive,
        host_app: &nobody,
    };
    assert_eq!(
        job_status(&roots(&dir), &probes, JOB).as_deref(),
        Some("busy")
    );
    assert_eq!(
        job_process(&roots(&dir), &probes, JOB).map(|(pid, _)| pid),
        Some(PID)
    );
}

#[test]
fn only_closing_brackets_braces_and_blanks_may_follow_the_first_object() {
    let dir = TempDir::new("trailing-shapes");
    let good = [
        "{\"a\":1}",
        "{\"a\":1}}",
        "{\"a\":1}\n",
        "{\"a\":1}}]\n }",
        "{\"a\":1} \t\r\n",
    ];
    let bad = [
        "{\"a\":1",
        "{\"a\":1}{\"b\":2}",
        "{\"a\":1}x",
        "{\"a\":1},",
        "{\"a\":1}\"}",
        "[1]]",
        "}{\"a\":1}",
        "",
        "   ",
    ];
    for (n, text) in good.iter().enumerate() {
        let path = dir.write(&format!("good{n}.json"), text);
        assert_eq!(read_json(&path), Some(json!({"a": 1})), "{text:?}");
    }
    for (n, text) in bad.iter().enumerate() {
        let path = dir.write(&format!("bad{n}.json"), text);
        assert_eq!(read_json(&path), None, "{text:?}");
    }
}
