use super::outbox::{hidden_queued, OutboxFeed, OutboxRow, Status};
use super::outgoing::{Outgoing, GRACE_MS};
use notchling_engine::sessions::{Agent, AgentGroup, AgentTarget};
use notchling_engine::transcript::model::{ChatItem, Conversation, ItemKind};
use notchling_local_api::Event;
use serde_json::{json, Value};

const SESSION: &str = "ab12cd34";
const QUEUED_MS: i64 = 10_000;
const SENT_MS: i64 = 12_000;
const TURN_AT: f64 = 12.5;

const SHOT: &str = "/Users/me/Library/Application Support/Notch/drops/17/Shot 1\u{202f}AM.png";

fn joined() -> String {
    "carry\u{200b}the\u{200b}blue\u{200b}lantern\u{200b}across\u{200b}the\u{200b}bridge".to_string()
}

fn with_shot() -> String {
    format!("why this lamp \n{}\n\n'{SHOT}'", joined())
}

fn event(name: &str, data: Value) -> Event {
    Event {
        event: name.into(),
        data,
    }
}

fn agent() -> Agent {
    Agent {
        id: "6f1c2a9e-3b7d-4e21-9a0c-5d8e7f6a1b2c".into(),
        name: "main".into(),
        project: "p".into(),
        cwd: "/w/p".into(),
        group: AgentGroup::Working,
        label: "Working".into(),
        detail: None,
        since_ms: 1,
        target: AgentTarget::Job(SESSION.into()),
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

fn wire(state: &str, text: &str) -> Value {
    json!({
        "id": "o1", "session": SESSION, "tool": "claude", "kind": "prompt", "text": text,
        "state": state, "why": "not_found", "position": 1, "queuedMs": QUEUED_MS,
        "updatedMs": QUEUED_MS, "attemptMs": QUEUED_MS + 500, "via": "chat",
    })
}

fn transcript(turns: &[(&str, usize, f64)]) -> Conversation {
    Conversation {
        items: turns
            .iter()
            .enumerate()
            .map(|(i, (text, images, at))| ChatItem {
                id: format!("u{i}"),
                kind: ItemKind::User {
                    text: (*text).to_owned(),
                    images: *images,
                    merged: false,
                },
                date: Some(*at),
            })
            .collect(),
        ..Conversation::default()
    }
}

fn settled(text: &str, turn: &str, images: usize) -> bool {
    let mut feed = OutboxFeed::default();
    feed.apply(
        &event("outbox.changed", json!({"row": wire("sending", text)})),
        QUEUED_MS,
    );
    feed.apply(
        &event(
            "outbox.removed",
            json!({"id": "o1", "session": SESSION, "why": "sent"}),
        ),
        SENT_MS,
    );
    assert_eq!(feed.bubbles(SESSION).len(), 1);
    feed.settle(
        SESSION,
        &transcript(&[(turn, images, TURN_AT)]),
        SENT_MS + 1_000,
    );
    feed.bubbles(SESSION).is_empty()
}

fn unsure_hidden(text: &str, turn: &str, images: usize) -> bool {
    let mut feed = OutboxFeed::default();
    feed.seed(&[wire("unsure", text)]);
    let shown = feed.bubbles_for_in(&agent(), &Conversation::default());
    assert_eq!(shown.len(), 1);
    assert_eq!(shown[0].status, Status::MayBeIn);
    feed.bubbles_for_in(&agent(), &transcript(&[(turn, images, TURN_AT)]))
        .is_empty()
}

fn arrived(text: &str, turn: &str) -> bool {
    let out = Outgoing::new(text, &Conversation::default(), 0);
    out.arrived(&transcript(&[(turn, 0, TURN_AT)]))
}

#[test]
fn a_sent_message_with_zero_width_spaces_is_gone_once_the_transcript_shows_it() {
    assert!(settled(&joined(), "carrythebluelanternacrossthebridge", 0));
}

#[test]
fn a_sent_picture_message_with_zero_width_spaces_is_gone_once_the_transcript_shows_its_chip() {
    assert!(settled(
        &with_shot(),
        "[Image #2]why this lamp \ncarrythebluelanternacrossthebridge",
        1
    ));
}

#[test]
fn a_maybe_sent_row_with_zero_width_spaces_has_no_bubble_once_the_transcript_shows_it() {
    assert!(unsure_hidden(
        &joined(),
        "carrythebluelanternacrossthebridge",
        0
    ));
    assert!(unsure_hidden(
        &with_shot(),
        "[Image #2]why this lamp \ncarrythebluelanternacrossthebridge",
        1
    ));
}

#[test]
fn a_typed_message_arrives_whatever_invisible_characters_claude_code_dropped() {
    assert!(arrived(&joined(), "carrythebluelanternacrossthebridge"));
    assert!(arrived("\u{feff}carry the\u{2060} lantern", "carry the lantern"));
}

#[test]
fn a_message_with_windows_line_breaks_arrives_with_unix_ones() {
    assert!(arrived("one\r\ntwo\r\nthree", "one\ntwo\nthree"));
    assert!(arrived("one\rtwo", "one\ntwo"));
}

#[test]
fn a_queued_copy_in_claude_codes_queue_hides_the_in_queue_row() {
    let row = OutboxRow::from_wire(&wire("in_queue", &joined())).expect("a row");
    let queued = vec![
        "something else".to_string(),
        "carrythebluelanternacrossthebridge".to_string(),
    ];
    assert_eq!(hidden_queued(&queued, &[row]), Some(1));
}

#[test]
fn text_that_differs_in_a_visible_character_still_does_not_arrive() {
    assert!(!arrived(&joined(), "carrythebluelampacrossthebridge"));
    assert!(!arrived(
        "carry the lantern",
        "carrythelantern"
    ));
    assert!(!arrived("carry\u{2800}lantern", "carrylantern"));
    assert!(!settled(&joined(), "carrythebluelampacrossthebridge", 0));
    assert!(!unsure_hidden(
        &with_shot(),
        "[Image #2]why this lamp \ncarrythebluelampacrossthebridge",
        1
    ));
    let mut feed = OutboxFeed::default();
    feed.seed(&[wire("unsure", &joined())]);
    let after = transcript(&[("another", 0, TURN_AT)]);
    assert_eq!(feed.bubbles_for_in(&agent(), &after).len(), 1);
}

#[test]
fn a_delivered_message_the_transcript_never_shows_still_lets_go_after_a_while() {
    let c = Conversation::default();
    let mut out = Outgoing::new(&joined(), &c, 0);
    out.delivered(1_000);
    assert!(out.keep(&c, 1_000 + GRACE_MS - 1));
    assert!(!out.keep(&c, 1_000 + GRACE_MS));
}
