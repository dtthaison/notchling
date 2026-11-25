use super::outbox::{OutboxFeed, Status};
use super::outgoing::{Turns, Wanted};
use notchling_engine::sessions::{Agent, AgentGroup, AgentTarget};
use notchling_engine::transcript::model::{ChatItem, Conversation, ItemKind};
use notchling_local_api::Event;
use serde_json::{json, Value};

const SESSION: &str = "ab12cd34";
const HEART: &str = "\u{2764}";
const HEART_RED: &str = "\u{2764}\u{FE0F}";

fn conversation(turns: &[(&str, f64)]) -> Conversation {
    Conversation {
        items: turns
            .iter()
            .enumerate()
            .map(|(i, (text, at))| ChatItem {
                id: format!("u{i}"),
                kind: ItemKind::User {
                    text: (*text).to_owned(),
                    images: 0,
                    merged: false,
                },
                date: Some(*at),
            })
            .collect(),
        ..Conversation::default()
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

fn event(name: &str, data: Value) -> Event {
    Event {
        event: name.into(),
        data,
    }
}

fn sending(id: &str, text: &str, queued_ms: i64, position: usize) -> Value {
    json!({
        "id": id, "session": SESSION, "tool": "claude", "kind": "prompt", "text": text,
        "state": "sending", "position": position, "queuedMs": queued_ms,
        "updatedMs": queued_ms, "via": "chat",
    })
}

#[test]
fn one_turn_clears_one_row_even_when_two_rows_read_alike() {
    let both = conversation(&[(HEART_RED, 12.5)]);
    let mut turns = Turns::new(&both);
    assert!(turns.take(&Wanted::of(HEART), 10_000.0));
    assert!(
        !turns.take(&Wanted::of(HEART_RED), 10_000.0),
        "the one turn is already spoken for"
    );
}

#[test]
fn a_second_turn_clears_the_second_row() {
    let both = conversation(&[(HEART_RED, 12.5), (HEART, 13.5)]);
    let mut turns = Turns::new(&both);
    assert!(turns.take(&Wanted::of(HEART), 10_000.0));
    assert!(turns.take(&Wanted::of(HEART_RED), 10_000.0));
}

#[test]
fn a_heart_and_a_heart_with_its_selector_are_both_kept_until_each_has_its_own_turn() {
    let mut feed = OutboxFeed::default();
    for (id, text, position) in [("o1", HEART, 1), ("o2", HEART_RED, 2)] {
        feed.apply(
            &event(
                "outbox.changed",
                json!({"row": sending(id, text, 10_000 + position as i64, position)}),
            ),
            10_000,
        );
    }
    let none = feed.bubbles_for_in(&agent(), &Conversation::default());
    assert_eq!(none.len(), 2);
    let one_turn = conversation(&[(HEART_RED, 12.5)]);
    let shown = feed.bubbles_for_in(&agent(), &one_turn);
    assert_eq!(
        shown.len(),
        1,
        "one turn hides one row, not both: {shown:?}"
    );
    assert_eq!(shown[0].status, Status::Sending);
    let two_turns = conversation(&[(HEART_RED, 12.5), (HEART, 13.5)]);
    assert!(feed.bubbles_for_in(&agent(), &two_turns).is_empty());
}
