use super::*;
use notchling_engine::sessions::{AgentGroup, AgentTarget};
use serde_json::json;

fn job(group: AgentGroup, detail: Option<&str>) -> Agent {
    Agent {
        id: "sess-1".into(),
        name: "job".into(),
        project: "p".into(),
        cwd: "/w/p".into(),
        group,
        label: "Needs input".into(),
        detail: detail.map(str::to_owned),
        since_ms: 42,
        target: AgentTarget::Job("ab12".into()),
        suggested_reply: None,
        transcript_path: None,
        host: "Background".into(),
        headless: false,
        config_dir: None,
        command: None,
        resume_session_id: None,
        at_prompt: false,
        node: None,
        worktree_path: None,
    }
}

fn row(state: &str, why: Option<&str>) -> OutboxRow {
    let mut wire = json!({
        "id": "o1", "session": "ab12", "tool": "claude", "kind": "command",
        "text": "/model claude-opus-5-5", "state": state, "position": 1,
        "queuedMs": 1, "updatedMs": 1, "via": "chat",
    });
    if let Some(why) = why {
        wire["why"] = json!(why);
    }
    OutboxRow::from_wire(&wire).expect("a row")
}

fn reply() -> Value {
    json!({"dialog": {
        "title": "Switch model?",
        "body": ["Your next response will be slower and use more tokens"],
        "options": [
            {"number": 1, "label": "Yes, switch to Opus 5.5", "selected": true},
            {"number": 2, "label": "No, go back", "selected": false},
        ],
    }})
}

#[test]
fn a_reply_with_a_dialog_becomes_a_card() {
    let card = DialogCard::from_reply("k", &reply()).expect("a card");
    assert_eq!(card.title, "Switch model?");
    assert_eq!(card.body.len(), 1);
    assert_eq!(card.options.len(), 2);
    assert!(card.options[0].selected && !card.options[1].selected);
    assert_eq!(card.key, "k");
}

#[test]
fn a_reply_without_a_dialog_or_with_one_option_is_no_card() {
    assert_eq!(DialogCard::from_reply("k", &json!({"dialog": null})), None);
    assert_eq!(DialogCard::from_reply("k", &json!({"items": []})), None);
    let mut one = reply();
    one["dialog"]["options"] = json!([{"number": 1, "label": "Yes"}]);
    assert_eq!(DialogCard::from_reply("k", &one), None);
    let mut untitled = reply();
    untitled["dialog"]["title"] = json!("  ");
    assert_eq!(DialogCard::from_reply("k", &untitled), None);
}

#[test]
fn picking_an_option_asks_the_daemon_to_press_that_number_for_that_dialog() {
    let card = DialogCard::from_reply("k", &reply()).unwrap();
    let request = card.pick("sess-1", &card.options[1]);
    assert_eq!(request.op, crate::acting::ops::SESSION_DIALOG_PICK);
    assert_eq!(
        request.args,
        json!({"id": "sess-1", "title": "Switch model?", "number": 2, "label": "No, go back"})
    );
}

#[test]
fn a_waiting_claude_job_is_read_once_per_waiting_spell() {
    let waiting = job(AgentGroup::NeedsInput, None);
    assert_eq!(
        read_key(&waiting, Tool::Claude, false).as_deref(),
        Some("dialog:sess-1|42")
    );
    let mut later = waiting.clone();
    later.since_ms = 43;
    assert_ne!(
        read_key(&later, Tool::Claude, false),
        read_key(&waiting, Tool::Claude, false)
    );
}

#[test]
fn a_question_or_a_permission_keeps_its_own_read() {
    for detail in ["answer: Postgres or SQLite?", "approve Bash: rm -rf build"] {
        let agent = job(AgentGroup::NeedsInput, Some(detail));
        assert_eq!(read_key(&agent, Tool::Claude, false), None, "{detail}");
    }
}

#[test]
fn only_a_local_claude_job_is_read() {
    let working = job(AgentGroup::Working, None);
    assert_eq!(read_key(&working, Tool::Claude, false), None);
    assert!(read_key(&working, Tool::Claude, true).is_some());
    let waiting = job(AgentGroup::NeedsInput, None);
    assert_eq!(read_key(&waiting, Tool::Codex, false), None);
    let mut asked = waiting.clone();
    asked.command = Some("run it".into());
    assert_eq!(read_key(&asked, Tool::Claude, false), None);
    let mut process = waiting;
    process.target = AgentTarget::Process(4242);
    assert_eq!(read_key(&process, Tool::Claude, false), None);
}

#[test]
fn the_outbox_head_asks_when_it_waits_for_a_dialog_or_an_answer() {
    assert!(asked_by_outbox(&[row("waiting", Some("dialog"))]));
    assert!(asked_by_outbox(&[row("waiting", Some("answer"))]));
    assert!(!asked_by_outbox(&[row("waiting", Some("turn"))]));
    assert!(!asked_by_outbox(&[row("queued", None)]));
    assert!(!asked_by_outbox(&[]));
}

#[test]
fn a_read_is_tried_a_few_times_then_left_alone() {
    let mut reading = Reading::default();
    assert!(reading.due("a", 0));
    reading.begin("a");
    assert!(!reading.due("a", 10), "one read at a time");
    reading.finish("a", None, 1_000);
    assert!(!reading.due("a", 1_000 + READ_RETRY_MS - 1));
    assert!(reading.due("a", 1_000 + READ_RETRY_MS));
    reading.begin("a");
    reading.finish("a", None, 10_000);
    reading.begin("a");
    reading.finish("a", None, 20_000);
    assert!(!reading.due("a", 99_000), "three tries are enough");
    assert!(
        reading.due("b", 99_000),
        "another waiting spell starts again"
    );
}

#[test]
fn a_card_shows_only_for_the_key_it_was_read_for() {
    let mut reading = Reading::default();
    reading.begin("a");
    let card = DialogCard::from_reply("a", &reply());
    reading.finish("a", card, 0);
    assert!(reading.shown("a").is_some());
    assert!(reading.shown("b").is_none());
    assert!(!reading.due("a", i64::MAX), "read, so not read again");
    reading.begin("b");
    assert!(
        reading.shown("a").is_none(),
        "a new spell drops the old card"
    );
}
