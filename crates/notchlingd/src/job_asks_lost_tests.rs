use super::failure_tests::{failing_with, Failing};
use super::*;
use crate::consent_testkit::agent_caller;
use crate::job_book::Record;
use std::process::Command;

const DAY_MS: i64 = 24 * 60 * 60 * 1000;

fn delegate_record(started: i64) -> Record {
    Record {
        session: "parent".to_string(),
        organization: "Personal".to_string(),
        kind: "delegate".to_string(),
        tool: "agy".to_string(),
        folder: "/work/gone".to_string(),
        started_ms: started,
        ended_ms: None,
        exit: None,
    }
}

fn state_of(listed: &Value, id: &str) -> String {
    listed["jobs"]
        .as_array()
        .unwrap()
        .iter()
        .find(|j| j["id"] == id)
        .map(|j| j["state"].as_str().unwrap_or_default().to_string())
        .unwrap_or_default()
}

fn begin_as(asks: &Arc<JobAsks>, pid: u32) -> String {
    let mut caller = agent_caller();
    caller.pid = Some(pid);
    let reply = asks
        .call(
            &caller,
            ops::DELEGATE_BEGIN,
            &json!({"tool": "agy", "folder": "/work/a"}),
        )
        .unwrap();
    reply["id"].as_str().unwrap().to_string()
}

fn mine(asks: &Arc<JobAsks>) -> Value {
    asks.call(&agent_caller(), ops::JOBS_MINE, &json!({}))
        .unwrap()
}

fn asks_over(tag: &str, book: Arc<JobBook>) -> Failing {
    failing_with(tag, ApiError::internal("unused"), book)
}

#[test]
fn an_old_delegate_job_that_never_ended_reads_as_lost() {
    let f = asks_over("old", Arc::new(JobBook::in_memory()));
    let asks = &f.asks;
    let now = now_ms();
    asks.book()
        .record("d-old", delegate_record(now - 3 * DAY_MS));
    asks.book().record("d-new", delegate_record(now - 60_000));
    let listed = mine(asks);
    assert_eq!(state_of(&listed, "d-old"), "lost", "{listed}");
    assert_eq!(state_of(&listed, "d-new"), "running", "{listed}");
    let got = asks
        .call(&agent_caller(), ops::JOBS_GET, &json!({"id": "d-old"}))
        .unwrap();
    assert_eq!(got["state"], "lost");
    assert!(got["endedMs"].is_number());
}

#[cfg(unix)]
#[test]
fn a_delegate_whose_process_is_gone_ends_as_lost_and_a_live_one_keeps_running() {
    let f = asks_over("pid", Arc::new(JobBook::in_memory()));
    let asks = &f.asks;
    let mut child = Command::new("sleep").arg("30").spawn().unwrap();
    let dead = begin_as(asks, child.id());
    let alive = begin_as(asks, std::process::id());
    assert_eq!(state_of(&mine(asks), &dead), "running");
    child.kill().unwrap();
    child.wait().unwrap();
    let listed = mine(asks);
    assert_eq!(state_of(&listed, &dead), "lost", "{listed}");
    assert_eq!(state_of(&listed, &alive), "running", "{listed}");
}

#[test]
fn starting_the_daemon_ends_the_jobs_whose_process_is_gone() {
    let book = Arc::new(JobBook::in_memory());
    book.record("d-stale", delegate_record(now_ms() - 3 * DAY_MS));
    let _f = asks_over("start", book.clone());
    let record = book.get("d-stale").unwrap();
    assert!(record.ended_ms.is_some(), "{record:?}");
}

#[test]
fn a_job_that_ended_by_itself_stays_done() {
    let f = asks_over("done", Arc::new(JobBook::in_memory()));
    let asks = &f.asks;
    let id = begin_as(asks, std::process::id());
    asks.call(
        &agent_caller(),
        ops::DELEGATE_END,
        &json!({"id": id, "exit": 0}),
    )
    .unwrap();
    assert_eq!(state_of(&mine(asks), &id), "done");
}
