use super::agent::{AgentGroup, AgentTarget};
use super::jobs::*;
use serde_json::{json, Value};

fn none(_: &str) -> Option<f64> {
    None
}

fn in_flight(tasks: u64) -> Value {
    json!({"tasks": tasks, "queued": 0, "kinds": ["local_bash", "local_agent"], "drainableMonitors": 0})
}

fn job(state: &str, tempo: &str, tasks: u64) -> Value {
    json!({
        "state": state, "tempo": tempo, "detail": "installed and running",
        "inFlight": in_flight(tasks), "template": "bg", "name": "main",
        "sessionId": "be50d36a-4348-4ba6-ab58-96959399a893",
        "resumeSessionId": "fee7356b-b891-4f6f-b4e7-35f7093b251c",
        "daemonShort": "be50d36a", "cwd": "/Users/me/app", "backend": "daemon",
        "updatedAt": "2026-11-14T14:56:40.293Z"
    })
}

fn live(status: &str) -> Value {
    json!({
        "pid": 79896, "sessionId": "fee7356b-b891-4f6f-b4e7-35f7093b251c", "kind": "bg",
        "jobId": "be50d36a", "status": status, "statusUpdatedAt": 1_794_668_142_000.0,
        "entrypoint": "cli"
    })
}

fn row(job: &Value, live: Option<&Value>) -> super::agent::Agent {
    agent_from_job(job, "be50d36a", live, &none)
}

#[test]
fn a_finished_turn_with_background_tasks_still_shows_working_but_is_at_its_prompt() {
    let a = row(&job("done", "idle", 3), Some(&live("busy")));
    assert_eq!(
        (a.group, a.label.as_str()),
        (AgentGroup::Working, "Working")
    );
    assert!(a.at_prompt);
    assert!(!a.turn_running());
    assert!(a.takes_held_text());
    assert!(matches!(a.target, AgentTarget::Job(_)));
}

#[test]
fn a_turn_that_ended_asking_a_question_while_tasks_run_is_at_its_prompt() {
    let a = row(&job("blocked", "active", 5), Some(&live("busy")));
    assert_eq!(a.group, AgentGroup::Working);
    assert!(a.at_prompt);
    assert!(a.takes_held_text());
}

#[test]
fn a_running_turn_is_not_at_its_prompt() {
    let a = row(&job("working", "active", 3), Some(&live("busy")));
    assert_eq!(a.group, AgentGroup::Working);
    assert!(!a.at_prompt);
    assert!(a.turn_running());
    assert!(!a.takes_held_text());
}

#[test]
fn a_turn_starting_before_the_job_file_says_so_is_not_called_over() {
    let a = row(&job("done", "idle", 0), Some(&live("busy")));
    assert_eq!(a.group, AgentGroup::Working);
    assert!(!a.at_prompt, "busy with nothing in flight is the turn");
    let other = row(&job("running", "idle", 2), Some(&live("busy")));
    assert!(!other.at_prompt);
    let shell = row(&job("done", "idle", 2), Some(&live("shell")));
    assert!(!shell.at_prompt);
}

#[test]
fn a_dialog_is_never_the_prompt() {
    let a = row(&job("blocked", "blocked", 2), Some(&live("waiting")));
    assert_eq!(a.group, AgentGroup::NeedsInput);
    assert!(!a.at_prompt);
    assert!(!a.takes_held_text());
}

#[test]
fn a_question_in_text_with_the_session_idle_takes_text() {
    let a = row(&job("blocked", "blocked", 0), Some(&live("idle")));
    assert_eq!(a.group, AgentGroup::NeedsInput);
    assert!(a.at_prompt);
    assert!(a.takes_held_text());
}

#[test]
fn a_job_with_no_live_session_is_never_called_at_its_prompt() {
    for state in ["working", "blocked", "done"] {
        let a = row(&job(state, "active", 2), None);
        assert!(!a.at_prompt, "state {state}");
    }
}

#[test]
fn an_older_job_file_without_in_flight_counts_keeps_the_old_reading() {
    let mut old = job("done", "idle", 0);
    old.as_object_mut().unwrap().remove("inFlight");
    let a = row(&old, Some(&live("busy")));
    assert_eq!(a.group, AgentGroup::Working);
    assert!(!a.at_prompt);
}

#[test]
fn the_flag_stays_off_the_wire_unless_set() {
    let off = row(&job("working", "active", 3), Some(&live("busy")));
    assert!(serde_json::to_value(&off)
        .unwrap()
        .get("atPrompt")
        .is_none());
    let on = row(&job("done", "idle", 3), Some(&live("busy")));
    assert_eq!(serde_json::to_value(&on).unwrap()["atPrompt"], json!(true));
    let back: super::agent::Agent =
        serde_json::from_value(serde_json::to_value(&on).unwrap()).unwrap();
    assert!(back.at_prompt);
}
