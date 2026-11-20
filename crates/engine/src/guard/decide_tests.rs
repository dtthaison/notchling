use super::decide::{decide, record, Env};
use super::event::{parse, Event, Kind};
use super::policy;
use super::rules::{Action, Limits, Rule};
use crate::sessions::testutil::TempDir;
use serde_json::{json, Value};
use std::path::Path;

const NOW: f64 = 1_800_000_000.0;

fn big(_: &Path) -> Option<u64> {
    Some(1_000_000)
}

fn event(kind: Kind, tool: &str, agent: Option<&str>, id: Option<&str>, input: Value) -> Event {
    Event {
        kind,
        session: "s1".into(),
        agent: agent.map(str::to_string),
        tool: tool.into(),
        tool_use_id: id.map(str::to_string),
        cwd: Some("/work".into()),
        input,
    }
}

fn spawn(id: &str) -> Event {
    event(
        Kind::Pre,
        "Agent",
        None,
        Some(id),
        json!({"description": "[code] x", "model": "sonnet"}),
    )
}

fn run(dir: &TempDir, event: &Event) -> Vec<(Rule, Action)> {
    let env = Env {
        now: NOW,
        dir: dir.root(),
        limits: Limits::default(),
        size_of: &big,
    };
    decide(event, &env)
        .into_iter()
        .map(|w| (w.rule, w.action))
        .collect()
}

#[test]
fn the_sixth_concurrent_builder_would_be_blocked() {
    let dir = TempDir::new("guard-decide");
    for i in 0..5 {
        assert!(
            run(&dir, &spawn(&format!("t{i}"))).is_empty(),
            "builder {i}"
        );
    }
    assert_eq!(run(&dir, &spawn("t5")), vec![(Rule::Cap, Action::Block)]);
}

#[test]
fn a_stopped_builder_makes_room() {
    let dir = TempDir::new("guard-decide");
    for i in 0..5 {
        run(&dir, &spawn(&format!("t{i}")));
        run(
            &dir,
            &event(
                Kind::SubagentStart,
                "",
                Some(&format!("a{i}")),
                None,
                Value::Null,
            ),
        );
    }
    assert_eq!(run(&dir, &spawn("t5")), vec![(Rule::Cap, Action::Block)]);
    run(
        &dir,
        &event(Kind::SubagentStop, "", Some("a0"), None, Value::Null),
    );
    run(
        &dir,
        &event(Kind::SubagentStop, "", Some("a1"), None, Value::Null),
    );
    assert!(run(&dir, &spawn("t6")).is_empty());
}

#[test]
fn a_spawn_without_an_id_is_not_counted() {
    let dir = TempDir::new("guard-decide");
    for _ in 0..8 {
        let e = event(
            Kind::Pre,
            "Agent",
            None,
            None,
            json!({"description": "[code] x"}),
        );
        assert!(run(&dir, &e).is_empty());
    }
}

#[test]
fn a_helper_past_eighty_calls_is_noted_once() {
    let dir = TempDir::new("guard-decide");
    let call = |n: u32| {
        event(
            Kind::Pre,
            "Bash",
            Some("agent-a"),
            Some(&format!("t{n}")),
            json!({"command": "cargo build"}),
        )
    };
    for n in 1..=80 {
        assert!(run(&dir, &call(n)).is_empty(), "call {n}");
    }
    assert_eq!(run(&dir, &call(81)), vec![(Rule::Size, Action::Note)]);
    assert!(run(&dir, &call(82)).is_empty());
}

#[test]
fn the_main_chat_is_never_a_long_helper() {
    let dir = TempDir::new("guard-decide");
    for n in 0..100 {
        let e = event(
            Kind::Pre,
            "Bash",
            None,
            Some(&format!("t{n}")),
            json!({"command": "ls"}),
        );
        assert!(run(&dir, &e).is_empty());
    }
}

#[test]
fn two_helpers_count_apart() {
    let dir = TempDir::new("guard-decide");
    let call = |agent: &str| event(Kind::Pre, "Read", Some(agent), None, json!({"limit": 5}));
    for _ in 0..60 {
        run(&dir, &call("a"));
        run(&dir, &call("b"));
    }
    assert!(run(&dir, &call("a")).is_empty());
}

#[test]
fn rules_and_state_add_up_on_one_call() {
    let dir = TempDir::new("guard-decide");
    for i in 0..5 {
        run(&dir, &spawn(&format!("t{i}")));
    }
    let mut over = spawn("t5");
    over.input = json!({"description": "[review] x", "model": "sonnet"});
    assert_eq!(
        run(&dir, &over),
        vec![(Rule::Review, Action::Correct), (Rule::Cap, Action::Block)]
    );
}

fn payload(command: &str) -> Vec<u8> {
    serde_json::to_vec(&json!({
        "hook_event_name": "PreToolUse",
        "session_id": "sess-1",
        "tool_name": "Bash",
        "tool_use_id": "toolu_1",
        "cwd": "/work",
        "tool_input": {"command": command, "description": "SECRET-DESCRIPTION"}
    }))
    .expect("json")
}

#[test]
fn without_a_policy_nothing_is_recorded() {
    let dir = TempDir::new("guard-record");
    assert!(record(&payload("sleep 60"), dir.root(), NOW).is_empty());
    assert!(!dir.path("events.log").exists());
}

#[test]
fn with_a_policy_a_would_is_queued_without_the_command() {
    let dir = TempDir::new("guard-record");
    policy::write(dir.root(), 5).expect("policy");
    let woulds = record(&payload("ls -R SECRET-COMMAND"), dir.root(), NOW);
    assert_eq!(woulds.len(), 1);
    assert_eq!(woulds[0].rule, Rule::Tree);
    let queued = std::fs::read_to_string(dir.path("events.log")).expect("queue");
    for secret in ["SECRET", "ls -R", "/work", "command", "description"] {
        assert!(!queued.contains(secret), "{secret} in {queued}");
    }
    let row: Value = serde_json::from_str(queued.trim()).expect("row");
    assert_eq!(row["session"], "sess-1");
    assert_eq!(row["tool"], "Bash");
    assert_eq!(row["rule"], "tree");
    assert_eq!(row["action"], "note");
    assert_eq!(row["agent"], "");
}

#[test]
fn a_call_that_would_do_nothing_leaves_no_file() {
    let dir = TempDir::new("guard-record");
    policy::write(dir.root(), 5).expect("policy");
    assert!(record(&payload("cargo test"), dir.root(), NOW).is_empty());
    assert!(!dir.path("events.log").exists());
}

#[test]
fn trouble_in_the_payload_records_nothing() {
    let dir = TempDir::new("guard-record");
    policy::write(dir.root(), 5).expect("policy");
    for bytes in [
        b"".to_vec(),
        b"not json".to_vec(),
        b"{}".to_vec(),
        br#"{"hook_event_name":"PreToolUse"}"#.to_vec(),
        br#"{"hook_event_name":"PreToolUse","session_id":"../../etc","tool_name":"Bash","tool_input":{"command":"sleep 60"}}"#.to_vec(),
        br#"{"hook_event_name":"Stop","session_id":"s1"}"#.to_vec(),
    ] {
        assert!(record(&bytes, dir.root(), NOW).is_empty());
    }
    assert!(!dir.path("events.log").exists());
}

#[test]
fn a_policy_that_is_off_or_damaged_stops_the_guard() {
    let dir = TempDir::new("guard-record");
    for text in ["", "nope", r#"{"level":"off"}"#, r#"{"v":1}"#] {
        dir.write("policy.json", text);
        assert!(
            record(&payload("sleep 60"), dir.root(), NOW).is_empty(),
            "{text}"
        );
    }
    dir.write("policy.json", r#"{"level":"log","cap":0}"#);
    assert_eq!(policy::read(dir.root()).map(|l| l.cap), Some(5));
    dir.write("policy.json", r#"{"level":"log","cap":3}"#);
    assert_eq!(policy::read(dir.root()).map(|l| l.cap), Some(3));
    policy::clear(dir.root());
    assert!(policy::read(dir.root()).is_none());
}

#[test]
fn the_payload_is_parsed_into_ids_only_where_they_are_plain() {
    let e = parse(
        br#"{"hook_event_name":"SubagentStop","session_id":"s-1","agent_id":"agent_9","agent_type":"x"}"#,
    )
    .expect("event");
    assert_eq!(e.kind, Kind::SubagentStop);
    assert_eq!(e.agent.as_deref(), Some("agent_9"));
    let odd = parse(
        br#"{"hook_event_name":"PreToolUse","session_id":"s-1","agent_id":"bad id!","tool_name":"Bash"}"#,
    )
    .expect("event");
    assert_eq!(odd.agent, None);
}
