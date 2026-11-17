mod common;

use common::*;
use notchling_engine::transcript::subagents::{self, Status};
use notchling_engine::transcript::util::now_secs;
use notchling_engine::transcript::{conversation, finished};
use std::collections::HashMap;

const MAIN: &str = include_str!("fixtures/background-job/main.jsonl");
const HANDED_BACK: &str = include_str!("fixtures/background-job/agent-handed-back.jsonl");
const RESUMED: &str = include_str!("fixtures/background-job/agent-resumed.jsonl");
const TRAILING: &str = include_str!("fixtures/background-job/agent-handed-back-trailing.jsonl");
const AGENT: &str = "a0b1c2d3e4f5a6b7c";
const META: &str = r#"{"agentType":"general-purpose","description":"Research film pacing","toolUseId":"toolu_launch01","requestShape":"background","model":"sonnet"}"#;

fn world(tag: &str, agent_file: &str) -> (Scratch, String) {
    let scratch = Scratch::new(tag);
    let transcript = s(&scratch.write("sess.jsonl", MAIN));
    scratch.write(&format!("sess/subagents/agent-{AGENT}.meta.json"), META);
    scratch.write(&format!("sess/subagents/agent-{AGENT}.jsonl"), agent_file);
    (scratch, transcript)
}

fn status(transcript: &str, finished: &HashMap<String, String>) -> Status {
    let list = subagents::list(transcript, finished, now_secs());
    assert_eq!(list.len(), 1, "{list:?}");
    list[0].status
}

#[test]
fn a_background_agent_its_parent_resumed_is_running_again() {
    let (_scratch, transcript) = world("resumed-running", RESUMED);
    let read = conversation::read(&transcript);
    assert_eq!(
        status(&transcript, &read.finished_agents),
        Status::Running,
        "{:?}",
        read.finished_agents
    );
}

#[test]
fn the_scan_of_earlier_transcripts_reopens_a_resumed_agent_too() {
    let (_scratch, transcript) = world("resumed-scan", RESUMED);
    let read = conversation::read(&transcript);
    let scanned = finished::read(&transcript);
    assert_eq!(*scanned, read.finished_agents);
    assert_eq!(status(&transcript, &scanned), Status::Running);
}

#[test]
fn an_agent_that_reports_again_after_a_resume_is_completed_again() {
    let (scratch, transcript) = world("resumed-again", HANDED_BACK);
    let resumed = conversation::read(&transcript);
    assert_eq!(
        resumed.finished_agents.get(AGENT).map(String::as_str),
        Some("resumed")
    );
    assert_eq!(
        status(&transcript, &resumed.finished_agents),
        Status::Running
    );
    scratch.append(
        "sess.jsonl",
        concat!(
            r#"{"type":"queue-operation","operation":"enqueue","timestamp":"2026-11-17T02:42:27.709Z","content":"<task-notification>\n<task-id>a0b1c2d3e4f5a6b7c</task-id>\n<tool-use-id>toolu_resume01</tool-use-id>\n<status>completed</status>\n</task-notification>"}"#,
            "\n"
        ),
    );
    let read = conversation::read(&transcript);
    assert_eq!(
        read.finished_agents.get(AGENT).map(String::as_str),
        Some("completed")
    );
    assert_eq!(
        status(&transcript, &read.finished_agents),
        Status::Completed
    );
}

#[test]
fn an_agent_never_resumed_stays_completed() {
    let (_scratch, transcript) = world("never-resumed", HANDED_BACK);
    let without_resume: String = MAIN.lines().take(4).map(|l| format!("{l}\n")).collect();
    std::fs::write(&transcript, without_resume).expect("rewrite");
    let read = conversation::read(&transcript);
    assert_eq!(
        status(&transcript, &read.finished_agents),
        Status::Completed
    );
}

#[test]
fn work_after_the_report_means_the_agent_was_not_handed_back() {
    let scratch = Scratch::new("handed-back-then-work");
    let resumed = s(&scratch.write("resumed.jsonl", RESUMED));
    let done = s(&scratch.write("done.jsonl", HANDED_BACK));
    assert!(!subagents::handed_back(&resumed));
    assert!(subagents::handed_back(&done));
}

#[test]
fn a_resumed_agent_is_running_by_its_own_file_when_its_parent_has_no_notice_yet() {
    let (_scratch, transcript) = world("resumed-own-file", RESUMED);
    assert_eq!(status(&transcript, &HashMap::new()), Status::Running);
}

#[test]
fn a_hand_back_followed_only_by_housekeeping_records_still_counts() {
    let scratch = Scratch::new("handed-back-trailing");
    let path = s(&scratch.write("trailing.jsonl", TRAILING));
    assert!(subagents::handed_back(&path));
}

#[test]
fn a_line_being_written_after_the_report_means_the_agent_is_working() {
    let scratch = Scratch::new("handed-back-partial");
    let path = s(&scratch.write(
        "partial.jsonl",
        &format!("{HANDED_BACK}{{\"parentUuid\":\"u1\",\"isSidechain\":tr"),
    ));
    assert!(!subagents::handed_back(&path));
}

#[test]
fn a_resume_the_parent_recorded_beats_a_report_line_that_is_still_the_last() {
    let (_scratch, transcript) = world("resumed-not-yet-writing", HANDED_BACK);
    let read = conversation::read(&transcript);
    assert_eq!(status(&transcript, &read.finished_agents), Status::Running);
}

#[test]
fn a_resume_that_failed_does_not_reopen_the_agent() {
    let (_scratch, transcript) = world("resume-failed", HANDED_BACK);
    let failed = MAIN.replace(
        r#""success":true,"message":"Resuming agent a0b1c2d""#,
        r#""success":false,"message":"No such agent a0b1c2d""#,
    );
    assert_ne!(failed, MAIN);
    std::fs::write(&transcript, failed).expect("rewrite");
    let read = conversation::read(&transcript);
    assert_eq!(
        read.finished_agents.get(AGENT).map(String::as_str),
        Some("completed")
    );
    assert_eq!(
        status(&transcript, &read.finished_agents),
        Status::Completed
    );
}
