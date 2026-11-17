use super::outbox::{OutboxFeed, Status};
use notchling_engine::sessions::{Agent, AgentGroup, AgentTarget};
use notchling_engine::transcript::model::{ChatItem, Conversation, ItemKind};
use serde_json::{json, Value};

const JOB: &str = "a2a33818";
const QUEUED_MS: i64 = 1_794_887_371_000;
const UNSURE_MS: i64 = 1_794_887_406_000;
const WRITTEN: f64 = 1_794_887_373.116;

fn wire(id: &str, state: &str, text: &str) -> Value {
    json!({
        "id": id, "session": JOB, "tool": "claude", "kind": "prompt", "text": text,
        "state": state, "why": "not_found", "position": 1, "queuedMs": QUEUED_MS,
        "updatedMs": UNSURE_MS, "via": "chat",
    })
}

fn job() -> Agent {
    Agent {
        id: "a2a33818-3136-4475-a7b4-d4ce6afec721".into(),
        name: "main".into(),
        project: "p".into(),
        cwd: "/w/p".into(),
        group: AgentGroup::Working,
        label: "Working".into(),
        detail: None,
        since_ms: 1,
        target: AgentTarget::Job(JOB.into()),
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

fn transcript(copies: &[(&str, f64)]) -> Conversation {
    Conversation {
        items: copies
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

fn feed(rows: &[Value]) -> OutboxFeed {
    let mut feed = OutboxFeed::default();
    feed.seed(rows);
    feed
}

fn texts(feed: &OutboxFeed, conversation: &Conversation) -> Vec<String> {
    feed.bubbles_for_in(&job(), conversation)
        .into_iter()
        .map(|b| b.text)
        .collect()
}

#[test]
fn a_maybe_sent_row_has_no_second_bubble_once_the_transcript_shows_it() {
    let feed = feed(&[wire("o1", "unsure", "a long message")]);
    let before = Conversation::default();
    let shown = feed.bubbles_for_in(&job(), &before);
    assert_eq!(shown.len(), 1);
    assert_eq!(shown[0].status, Status::MayBeIn);
    let after = transcript(&[("a long message", WRITTEN)]);
    assert!(
        feed.bubbles_for_in(&job(), &after).is_empty(),
        "the transcript's copy is this message, written before the daemon gave up looking"
    );
}

#[test]
fn a_maybe_sent_row_stays_when_the_copy_is_older_than_the_row() {
    let feed = feed(&[wire("o1", "unsure", "a long message")]);
    let earlier = transcript(&[("a long message", (QUEUED_MS - 1_000) as f64 / 1000.0)]);
    assert_eq!(texts(&feed, &earlier), ["a long message"]);
}

#[test]
fn a_maybe_sent_row_stays_when_the_transcript_has_other_words() {
    let feed = feed(&[wire("o1", "unsure", "a long message")]);
    let other = transcript(&[("another message", WRITTEN)]);
    assert_eq!(texts(&feed, &other), ["a long message"]);
}

#[test]
fn the_row_behind_a_maybe_sent_one_keeps_its_bubble() {
    let feed = feed(&[
        wire("o1", "unsure", "a long message"),
        wire("o2", "queued", "and then this"),
    ]);
    let after = transcript(&[("a long message", WRITTEN)]);
    assert_eq!(texts(&feed, &after), ["and then this"]);
}

#[test]
fn spacing_does_not_keep_the_second_bubble_of_a_maybe_sent_row() {
    let feed = feed(&[wire("o1", "unsure", "a long message\n")]);
    let after = transcript(&[("a long message", WRITTEN)]);
    assert!(texts(&feed, &after).is_empty());
}

fn attempted(id: &str, text: &str, attempt_ms: i64) -> Value {
    let mut row = wire(id, "unsure", text);
    row["attemptMs"] = json!(attempt_ms);
    row
}

#[test]
fn a_copy_written_while_the_row_waited_does_not_hide_a_maybe_sent_row() {
    let attempt = QUEUED_MS + 20_000;
    let feed = feed(&[attempted("o1", "a long message", attempt)]);
    let waiting = transcript(&[("a long message", (QUEUED_MS + 10_000) as f64 / 1000.0)]);
    assert_eq!(texts(&feed, &waiting), ["a long message"]);
    let after = transcript(&[("a long message", (attempt + 2_000) as f64 / 1000.0)]);
    assert!(texts(&feed, &after).is_empty());
}
