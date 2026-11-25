use super::*;
use notchling_engine::sessions::{Agent, AgentGroup, AgentTarget, NodeRef};
use serde_json::{json, Value};

const T0: i64 = 1_000_000;

fn wire(id: &str, session: &str, created: i64) -> Value {
    json!({
        "id": id, "session": session, "asker": "subagent", "label": "Review Task 3",
        "agentType": "reviewer", "model": "sonnet", "branch": "agent/ask",
        "question": "Which one?", "choices": ["Keep it", "Drop it"],
        "mainMayAnswer": false, "mode": "background", "state": "open", "waiting": true,
        "createdMs": created, "updatedMs": created,
    })
}

fn closed(id: &str, state: &str, by: Option<&str>, choice: Option<u8>, closed_ms: i64) -> Value {
    let mut row = wire(id, "ab12cd34", T0);
    row["state"] = json!(state);
    row["waiting"] = json!(false);
    row["closedMs"] = json!(closed_ms);
    if let Some(by) = by {
        row["by"] = json!(by);
    }
    if let Some(choice) = choice {
        row["choice"] = json!(choice);
        row["answer"] = json!(["Keep it", "Drop it"][choice as usize - 1]);
    }
    row
}

fn row(value: Value) -> AskRow {
    AskRow::from_wire(&value).expect("a row")
}

fn changed(value: Value) -> (&'static str, Value) {
    ("ask.changed", json!({"row": value}))
}

fn removed(id: &str, session: &str) -> (&'static str, Value) {
    ("ask.removed", json!({"id": id, "session": session}))
}

fn job_agent(id: &str, job: &str) -> Agent {
    Agent {
        worktree_path: None,
        id: id.into(),
        name: "job".into(),
        project: "p".into(),
        cwd: "/w/p".into(),
        group: AgentGroup::Working,
        label: "Working".into(),
        detail: None,
        since_ms: 1,
        target: AgentTarget::Job(job.into()),
        suggested_reply: None,
        transcript_path: None,
        host: "Background".into(),
        headless: false,
        config_dir: None,
        command: None,
        resume_session_id: None,
        at_prompt: false,
        node: None,
    }
}

#[test]
fn feed_applies_changed_and_removed_in_order() {
    let mut feed = AskFeed::default();
    let agent = job_agent("6f1c2a9e", "ab12cd34");
    let (event, data) = changed(wire("q1", "ab12cd34", T0));
    assert!(feed.apply(event, &data));
    assert!(!feed.apply(event, &data));
    let mut again = wire("q1", "ab12cd34", T0);
    again["waiting"] = json!(false);
    let (event, data) = changed(again);
    assert!(feed.apply(event, &data));
    assert!(!feed.rows_for(&agent)[0].waiting);
    let (event, data) = removed("q1", "ab12cd34");
    assert!(feed.apply(event, &data));
    assert!(feed.rows_for(&agent).is_empty());
    assert!(!feed.apply(event, &data));
    assert!(!feed.apply("ask.unknown", &json!({})));
    assert!(!feed.apply("ask.changed", &json!({"row": {"id": "x"}})));
    assert!(!feed.apply("ask.removed", &json!({})));
}

#[test]
fn reseed_replaces() {
    let mut feed = AskFeed::default();
    let agent = job_agent("6f1c2a9e", "ab12cd34");
    let (event, data) = changed(wire("q1", "ab12cd34", T0));
    feed.apply(event, &data);
    feed.reseed(vec![row(wire("q2", "ab12cd34", T0 + 5))]);
    let ids: Vec<&str> = feed
        .rows_for(&agent)
        .iter()
        .map(|r| r.id.as_str())
        .collect();
    assert_eq!(ids, ["q2"]);
    feed.reseed(Vec::new());
    assert!(feed.rows_for(&agent).is_empty());
    assert_eq!(feed, AskFeed::default());
}

#[test]
fn rows_for_matches_outbox_key_oldest_first() {
    let mut feed = AskFeed::default();
    feed.reseed(vec![
        row(wire("late", "ab12cd34", T0 + 20)),
        row(wire("other", "ffff0000", T0 + 1)),
        row(wire("early", "ab12cd34", T0 + 10)),
        row(wire("remote", "n1/ab12cd34", T0 + 2)),
    ]);
    let job = job_agent("6f1c2a9e-3b7d-4e21-9a0c-5d8e7f6a1b2c", "ab12cd34");
    let ids: Vec<&str> = feed.rows_for(&job).iter().map(|r| r.id.as_str()).collect();
    assert_eq!(ids, ["early", "late"]);
    let mut remote = job_agent("x", "ab12cd34");
    remote.node = Some(NodeRef {
        id: "n1".into(),
        name: "m".into(),
        online: true,
    });
    let ids: Vec<&str> = feed
        .rows_for(&remote)
        .iter()
        .map(|r| r.id.as_str())
        .collect();
    assert_eq!(ids, ["remote"]);
}

#[test]
fn needs_you_only_while_open() {
    let mut feed = AskFeed::default();
    let agent = job_agent("6f1c2a9e", "ab12cd34");
    assert!(!feed.needs_you(&agent));
    feed.reseed(vec![row(closed(
        "q1",
        "answered",
        Some("mac"),
        Some(1),
        T0 + 9,
    ))]);
    assert!(!feed.needs_you(&agent));
    let (event, data) = changed(wire("q2", "ab12cd34", T0));
    feed.apply(event, &data);
    assert!(feed.needs_you(&agent));
    assert!(!feed.needs_you(&job_agent("zz", "ffff0000")));
    let (event, data) = changed(closed("q2", "dismissed", None, None, T0 + 30));
    feed.apply(event, &data);
    assert!(!feed.needs_you(&agent));
}

fn header_of(value: Value, now: i64) -> String {
    card(&row(value), now).expect("a card").header
}

#[test]
fn card_words_for_every_state_and_asker() {
    let open = card(&row(wire("q1", "s", T0)), T0 + 4 * 60_000).expect("a card");
    assert!(open.open);
    assert_eq!(open.closed_line, None);
    assert_eq!(open.question, "Which one?");
    assert_eq!(open.choices, ["Keep it", "Drop it"]);
    assert_eq!(
        open.header,
        "Review Task 3 · reviewer · sonnet · agent/ask · waited 4 min"
    );

    let mut main = wire("q2", "s", T0);
    main["asker"] = json!("main");
    for key in ["label", "agentType", "model"] {
        main.as_object_mut().expect("object").remove(key);
    }
    assert_eq!(
        header_of(main, T0 + 30_000),
        "This session asks · agent/ask · just now"
    );

    let mut unknown = wire("q3", "s", T0);
    unknown["asker"] = json!("unknown");
    for key in ["label", "agentType", "model", "branch"] {
        unknown.as_object_mut().expect("object").remove(key);
    }
    unknown["choices"] = Value::Null;
    let card_unknown = card(&row(unknown), T0 + 2 * 3_600_000).expect("a card");
    assert_eq!(
        card_unknown.header,
        "This session or one of its subagents · waited 2 h"
    );
    assert!(card_unknown.choices.is_empty());

    let mut codex = wire("q4", "s", T0);
    codex["asker"] = json!("codex");
    for key in ["label", "agentType", "model", "branch"] {
        codex.as_object_mut().expect("object").remove(key);
    }
    assert_eq!(header_of(codex.clone(), T0), "Codex job · just now");
    codex["label"] = json!("Fix the parser");
    assert_eq!(header_of(codex, T0), "Fix the parser · just now");

    let mut agy = wire("q5", "s", T0);
    agy["asker"] = json!("antigravity");
    for key in ["label", "agentType", "model", "branch"] {
        agy.as_object_mut().expect("object").remove(key);
    }
    assert_eq!(header_of(agy, T0), "Antigravity job · just now");

    let mut bare = wire("q6", "s", T0);
    for key in ["label", "agentType", "model", "branch"] {
        bare.as_object_mut().expect("object").remove(key);
    }
    assert_eq!(header_of(bare, T0), "A subagent · just now");

    let cases = [
        (
            closed("c1", "answered", Some("mac"), Some(1), T0 + 1),
            "Answered · Mac: Keep it",
        ),
        (
            closed("c2", "delivered", Some("phone"), Some(2), T0 + 1),
            "Answered · Phone: Drop it",
        ),
        (
            closed("c3", "answered", Some("main"), None, T0 + 1),
            "Answered · main session",
        ),
        (closed("c4", "dismissed", None, None, T0 + 1), "Dismissed"),
        (
            closed("c5", "expired", None, None, T0 + 1),
            "No answer in time",
        ),
        (closed("c6", "gone", None, None, T0 + 1), "Stopped waiting"),
    ];
    for (value, line) in cases {
        let shown = card(&row(value), T0 + 2).expect("a card");
        assert!(!shown.open);
        assert!(shown.choices.is_empty());
        assert_eq!(shown.closed_line.as_deref(), Some(line));
    }

    let mut typed = closed("c7", "answered", Some("mac"), None, T0 + 1);
    typed["answer"] = json!("  use\nthe  second\tone  ");
    let shown = card(&row(typed), T0 + 2).expect("a card");
    assert_eq!(
        shown.closed_line.as_deref(),
        Some("Answered · Mac: use the second one")
    );
    let mut long = closed("c8", "answered", Some("phone"), None, T0 + 1);
    long["answer"] = json!("é".repeat(200));
    let line = card(&row(long), T0 + 2)
        .and_then(|c| c.closed_line)
        .expect("a line");
    assert!(line.starts_with("Answered · Phone: éé"));
    assert!(line.chars().count() < 100);
}

#[test]
fn closed_card_goes_after_ten_minutes() {
    let value = closed("c1", "answered", Some("mac"), Some(1), T0);
    let gone = row(value);
    assert!(card(&gone, T0).is_some());
    assert!(card(&gone, T0 + 599_999).is_some());
    assert_eq!(card(&gone, T0 + 600_000), None);
    assert_eq!(card(&gone, T0 + 9_000_000), None);
    let mut open = wire("q1", "s", T0);
    open["updatedMs"] = json!(T0);
    assert!(card(&row(open), T0 + 9_000_000).is_some());
    let mut no_close = closed("c2", "dismissed", None, None, T0);
    no_close.as_object_mut().expect("object").remove("closedMs");
    no_close["updatedMs"] = json!(T0 + 100);
    let no_close = row(no_close);
    assert!(card(&no_close, T0 + 600_099).is_some());
    assert_eq!(card(&no_close, T0 + 600_100), None);
}

#[test]
fn waited_words() {
    assert_eq!(waited(-5), "just now");
    assert_eq!(waited(0), "just now");
    assert_eq!(waited(59_999), "just now");
    assert_eq!(waited(60_000), "waited 1 min");
    assert_eq!(waited(4 * 60_000 + 59_000), "waited 4 min");
    assert_eq!(waited(59 * 60_000 + 59_999), "waited 59 min");
    assert_eq!(waited(3_600_000), "waited 1 h");
    assert_eq!(waited(2 * 3_600_000 + 3_000_000), "waited 2 h");
}

#[test]
fn a_lost_answer_reads_by_who_won() {
    assert_eq!(lost_line(&json!({"answered": true})), None);
    assert_eq!(
        lost_line(&json!({"answered": false, "state": "answered", "by": "phone"})),
        Some("Already answered · Phone".into())
    );
    assert_eq!(
        lost_line(&json!({"answered": false, "state": "delivered", "by": "mac"})),
        Some("Already answered · Mac".into())
    );
    assert_eq!(
        lost_line(&json!({"answered": false, "state": "answered", "by": "main"})),
        Some("Already answered · main session".into())
    );
    assert_eq!(
        lost_line(&json!({"answered": false, "state": "dismissed"})),
        Some("Dismissed".into())
    );
    assert_eq!(
        lost_line(&json!({"answered": false, "state": "expired"})),
        Some("No answer in time".into())
    );
    assert_eq!(
        lost_line(&json!({"answered": false, "state": "gone"})),
        Some("Stopped waiting".into())
    );
    assert_eq!(
        lost_line(&json!({"answered": false})),
        Some("Already answered".into())
    );
}
