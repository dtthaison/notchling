use super::outbox::{hidden_queued, OutboxFeed, OutboxRow, Status};
use super::outgoing::Outgoing;
use notchling_engine::sessions::{Agent, AgentGroup, AgentTarget};
use notchling_engine::transcript::model::{ChatItem, Conversation, ItemKind};
use notchling_local_api::Event;
use serde_json::{json, Value};

const SESSION: &str = "ab12cd34";
const QUEUED_MS: i64 = 10_000;
const SENT_MS: i64 = 12_000;
const TURN_AT: f64 = 12.5;

const ONE_PICTURE: &str = "where add assets\n\n'/Users/me/Library/Application Support/Notch/drops/1795062208712/Shot 2026-11-19 at 11.23.40\u{202f}AM.png'";
const TWO_PICTURES: &str = "compare these\n\n/tmp/drops/a.png '/tmp/drops/b c.jpg'";
const WRAPPED: &str =
    "the first line of a longer note\nand its second line, which wraps\n\n/tmp/drops/a.png";

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
        "state": state, "position": 1, "queuedMs": QUEUED_MS, "updatedMs": QUEUED_MS,
        "via": "chat",
    })
}

fn row(state: &str, text: &str) -> OutboxRow {
    OutboxRow::from_wire(&wire(state, text)).expect("a row")
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

fn sent_feed(text: &str) -> OutboxFeed {
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
    feed
}

fn settled(text: &str, turn: &str, images: usize) -> bool {
    let mut feed = sent_feed(text);
    assert_eq!(feed.bubbles(SESSION).len(), 1);
    feed.settle(
        SESSION,
        &transcript(&[(turn, images, TURN_AT)]),
        SENT_MS + 1_000,
    );
    feed.bubbles(SESSION).is_empty()
}

#[test]
fn a_sent_message_with_a_picture_is_gone_once_the_transcript_shows_its_chip() {
    assert!(settled(ONE_PICTURE, "[Image #3]where add assets", 1));
}

#[test]
fn the_chip_number_is_any_number() {
    for n in [1, 3, 48, 1024] {
        assert!(
            settled(ONE_PICTURE, &format!("[Image #{n}]where add assets"), 1),
            "{n}"
        );
    }
}

#[test]
fn several_pictures_become_several_chips() {
    assert!(settled(
        TWO_PICTURES,
        "[Image #4] [Image #5]compare these",
        2
    ));
    assert!(settled(
        TWO_PICTURES,
        "[Image #4]compare these /tmp/drops/a.png",
        1
    ));
}

#[test]
fn a_message_that_wraps_with_a_picture_still_matches() {
    assert!(settled(
        WRAPPED,
        "[Image #7]the first line of a longer note\nand its second line, which wraps",
        1
    ));
    assert!(settled(
        WRAPPED,
        "[Image #7]the first line of a longer note and its second line, which wraps",
        1
    ));
}

#[test]
fn a_picture_alone_is_its_chip() {
    assert!(settled("/tmp/drops/a.png", "[Image #2]", 1));
}

#[test]
fn text_only_matches_as_before() {
    assert!(settled("ship it", "ship it\n", 0));
    assert!(!settled("ship it", "ship it, please", 0));
    assert!(!settled("ship it", "[Image #3]ship it", 1));
}

#[test]
fn near_misses_do_not_clear() {
    let near = [
        ("[Image #3]where add asset", 1),
        ("[Image #3]where add assets too", 1),
        ("[Image #3]add assets where", 1),
        ("where add assets", 0),
        ("[Image #3][Image #4]where add assets", 2),
        ("[Image #3]", 1),
        ("[Image #3] where add assets [Image #4]", 2),
    ];
    for (turn, images) in near {
        assert!(!settled(ONE_PICTURE, turn, images), "{turn}");
    }
}

#[test]
fn a_chip_the_person_wrote_is_not_a_picture_of_ours() {
    assert!(!settled("see [Image #1] now", "see [Image #2] now", 0));
    assert!(settled("see [Image #1] now", "see [Image #1] now", 0));
    assert!(!settled("see now", "[Image #5]see now", 1));
    assert!(!settled(
        "see [Image #1] now\n\n/tmp/drops/a.png",
        "[Image #2]see [Image #1] now",
        1
    ));
}

#[test]
fn an_older_copy_with_a_chip_is_not_this_message() {
    let mut feed = sent_feed(ONE_PICTURE);
    let older = transcript(&[("[Image #1]where add assets", 1, 1.0)]);
    feed.settle(SESSION, &older, SENT_MS + 1_000);
    assert_eq!(feed.bubbles(SESSION).len(), 1);
}

#[test]
fn the_echo_of_a_message_with_a_picture_arrives_with_its_chip() {
    let before = transcript(&[]);
    let out = Outgoing::new(ONE_PICTURE, &before, 10_000);
    assert!(!out.arrived(&before));
    let after = transcript(&[("[Image #3]where add assets", 1, TURN_AT)]);
    assert!(out.arrived(&after));
    assert!(!out.keep(&after, 11_000));
}

#[test]
fn the_echo_counts_copies_with_any_chip_number() {
    let before = transcript(&[("[Image #1]where add assets", 1, 1.0)]);
    let out = Outgoing::new(ONE_PICTURE, &before, 10_000);
    assert!(!out.arrived(&before), "the earlier copy is not this one");
    let after = transcript(&[
        ("[Image #1]where add assets", 1, 1.0),
        ("[Image #2]where add assets", 1, 1.5),
    ]);
    assert!(out.arrived(&after));
}

#[test]
fn a_message_being_sent_gives_way_to_its_chip_turn() {
    let mut feed = OutboxFeed::default();
    feed.seed(&[wire("sending", ONE_PICTURE)]);
    let shown = feed.bubbles_for_in(&agent(), &transcript(&[]));
    assert_eq!(shown[0].status, Status::Sending);
    let after = transcript(&[("[Image #3]where add assets", 1, 13.0)]);
    assert!(feed.bubbles_for_in(&agent(), &after).is_empty());
    let other = transcript(&[("[Image #3]where add asset", 1, 13.0)]);
    assert_eq!(feed.bubbles_for_in(&agent(), &other).len(), 1);
}

#[test]
fn a_message_in_claude_codes_queue_hides_the_queued_entry_with_its_chip() {
    let queued = vec!["first".to_owned(), "[Image #3]where add assets".to_owned()];
    let held = [row("in_queue", ONE_PICTURE)];
    assert_eq!(hidden_queued(&queued, &held), Some(1));
    let other = vec!["[Image #3]where add asset".to_owned()];
    assert_eq!(hidden_queued(&other, &held), None);
}
