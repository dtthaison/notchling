use super::{key_for, name_for, wire};
use notchling_engine::ask::{AskState, Asker, By, Mode, Row};
use notchling_engine::sessions::Agent;
use serde_json::json;

fn row() -> Row {
    Row {
        n: 1,
        id: "q01".into(),
        key: "k".into(),
        session: "abcd1234".into(),
        session_id: Some("sid-1".into()),
        asker: Asker::Subagent,
        agent_id: Some("agent-7".into()),
        label: Some("Review Task 3".into()),
        agent_type: Some("reviewer".into()),
        model: Some("sonnet".into()),
        tool_use_id: Some("toolu_1".into()),
        folder: Some("/tmp/work".into()),
        branch: Some("agent/ask".into()),
        question: "Which one?".into(),
        choices: Some(vec!["a".into(), "b".into()]),
        main_may_answer: true,
        mode: Mode::Background,
        deadline_ms: Some(9),
        pid: 77,
        pid_start: Some(5),
        state: AskState::Answered,
        answer: Some("b".into()),
        choice: Some(2),
        by: Some(By::Phone),
        inbox_id: Some("inbox-1".into()),
        created_ms: 100,
        updated_ms: 200,
        closed_ms: Some(300),
    }
}

fn bare() -> Row {
    Row {
        agent_id: None,
        label: None,
        agent_type: None,
        model: None,
        tool_use_id: None,
        folder: None,
        branch: None,
        choices: None,
        answer: None,
        choice: None,
        by: None,
        closed_ms: None,
        session_id: None,
        state: AskState::Open,
        asker: Asker::Unknown,
        ..row()
    }
}

#[test]
fn a_row_on_the_wire_is_camel_case_with_every_field() {
    let wire = wire(&row(), true, true);
    assert_eq!(
        wire,
        json!({
            "id": "q01",
            "session": "abcd1234",
            "sessionId": "sid-1",
            "asker": "subagent",
            "agentId": "agent-7",
            "label": "Review Task 3",
            "agentType": "reviewer",
            "model": "sonnet",
            "folder": "/tmp/work",
            "branch": "agent/ask",
            "question": "Which one?",
            "choices": ["a", "b"],
            "mainMayAnswer": true,
            "mode": "background",
            "state": "answered",
            "waiting": true,
            "answer": "b",
            "choice": 2,
            "by": "phone",
            "createdMs": 100,
            "updatedMs": 200,
            "closedMs": 300,
        })
    );
}

#[test]
fn empty_optionals_are_left_out() {
    let wire = wire(&bare(), false, true);
    let keys: Vec<&str> = wire
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    for key in [
        "id",
        "session",
        "asker",
        "question",
        "mainMayAnswer",
        "mode",
        "state",
        "waiting",
        "createdMs",
        "updatedMs",
    ] {
        assert!(keys.contains(&key), "{key}");
    }
    for key in [
        "sessionId",
        "agentId",
        "label",
        "agentType",
        "model",
        "folder",
        "branch",
        "choices",
        "answer",
        "choice",
        "by",
        "closedMs",
    ] {
        assert!(!keys.contains(&key), "{key}");
    }
    assert_eq!(wire["waiting"], json!(false));
}

#[test]
fn the_answer_is_only_there_when_asked_for() {
    assert!(wire(&row(), false, true).get("answer").is_some());
    let without = wire(&row(), false, false);
    assert!(without.get("answer").is_none());
    assert_eq!(without["choice"], json!(2));
}

fn agent(id: &str, job: &str, transcript: Option<&str>) -> Agent {
    serde_json::from_value(json!({
        "id": id, "name": "n", "project": "p", "cwd": "/tmp", "group": "working", "label": "",
        "detail": null, "sinceMs": 0, "target": {"job": job}, "suggestedReply": null,
        "transcriptPath": transcript, "host": "Background", "headless": false,
        "configDir": null, "command": null, "resumeSessionId": null
    }))
    .unwrap()
}

#[test]
fn the_key_is_the_listed_agents_outbox_key_else_the_session_id() {
    let agents = vec![
        agent("row-1", "abcd1234", Some("/tmp/none/sid-1.jsonl")),
        agent("row-2", "wxyz9999", None),
    ];
    assert_eq!(key_for(&agents, "sid-1"), "abcd1234");
    assert_eq!(key_for(&agents, "sid-unknown"), "sid-unknown");
    assert_eq!(key_for(&[], "sid-1"), "sid-1");
}

fn process(id: &str, name: &str) -> Agent {
    serde_json::from_value(json!({
        "id": id, "name": name, "project": "p", "cwd": "/tmp", "group": "working", "label": "",
        "detail": null, "sinceMs": 0, "target": {"process": 4242}, "suggestedReply": null,
        "transcriptPath": null, "host": "Terminal", "headless": false,
        "configDir": null, "command": null, "resumeSessionId": null
    }))
    .unwrap()
}

#[test]
fn a_session_with_no_transcript_path_is_found_by_its_row_id() {
    let agents = vec![
        process("sid-t", "build site"),
        agent("sid-j", "abcd1234", None),
    ];
    assert_eq!(name_for(&agents, "sid-t"), "build site");
    assert_eq!(key_for(&agents, "sid-t"), "sid-t");
    assert_eq!(
        key_for(&agents, "sid-j"),
        "abcd1234",
        "a job keys on its job id"
    );
    assert_eq!(name_for(&agents, "sid-j"), "n");
    assert_eq!(name_for(&agents, "nobody"), "");
}
