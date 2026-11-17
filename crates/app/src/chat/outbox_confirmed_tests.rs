use super::outbox::{OutboxFeed, Status};
use notchling_engine::sessions::{Agent, AgentGroup, AgentTarget};
use notchling_engine::transcript::model::{ChatItem, Conversation, ItemKind};
use serde_json::{json, Value};

const JOB: &str = "52afae32";
const QUEUED_MS: i64 = 1_794_878_653_417;
const ATTEMPT_MS: i64 = 1_794_878_653_500;

fn wire(id: &str, state: &str, text: &str) -> Value {
    json!({
        "id": id, "session": JOB, "tool": "claude", "kind": "prompt", "text": text,
        "state": state, "position": 1, "queuedMs": QUEUED_MS, "updatedMs": ATTEMPT_MS,
        "via": "chat",
    })
}

fn job() -> Agent {
    Agent {
        id: "6f1c2a9e-3b7d-4e21-9a0c-5d8e7f6a1b2c".into(),
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
fn a_message_being_sent_has_no_second_bubble_once_the_transcript_shows_it() {
    let feed = feed(&[wire("o1", "sending", "hi")]);
    let before = Conversation::default();
    let shown = feed.bubbles_for_in(&job(), &before);
    assert_eq!(shown.len(), 1);
    assert_eq!(shown[0].status, Status::Sending);

    let after = transcript(&[("hi", 1_794_878_654.783)]);
    assert!(
        feed.bubbles_for_in(&job(), &after).is_empty(),
        "the transcript's hi is this one"
    );
}

#[test]
fn a_copy_from_before_the_attempt_is_not_this_message() {
    let feed = feed(&[wire("o1", "sending", "hi")]);
    let earlier = transcript(&[("hi", 1_794_878_000.0)]);
    assert_eq!(texts(&feed, &earlier), ["hi"]);
}

#[test]
fn other_words_in_the_transcript_do_not_hide_the_bubble() {
    let feed = feed(&[wire("o1", "sending", "hi")]);
    let other = transcript(&[("hello", 1_794_878_654.783)]);
    assert_eq!(texts(&feed, &other), ["hi"]);
}

#[test]
fn spacing_does_not_keep_the_second_bubble() {
    let feed = feed(&[wire("o1", "sending", "hi\n")]);
    let after = transcript(&[("hi", 1_794_878_654.783)]);
    assert!(texts(&feed, &after).is_empty());
}

#[test]
fn only_a_row_being_sent_gives_way_to_the_transcript() {
    let after = transcript(&[("hi", 1_794_878_654.783)]);
    for state in [
        "queued", "waiting", "in_queue", "unsure", "box", "stopped", "gone",
    ] {
        let feed = feed(&[wire("o1", state, "hi")]);
        assert_eq!(texts(&feed, &after), ["hi"], "{state} keeps its bubble");
    }
}

#[test]
fn a_confirmed_row_does_not_hide_the_ones_waiting_behind_it() {
    let feed = feed(&[
        wire("o1", "sending", "hi"),
        wire("o2", "queued", "and then this"),
    ]);
    let after = transcript(&[("hi", 1_794_878_654.783)]);
    assert_eq!(texts(&feed, &after), ["and then this"]);
}

#[test]
fn a_copy_just_before_the_attempt_is_the_message_before_not_this_one() {
    let feed = feed(&[wire("o2", "sending", "hi")]);
    let first = transcript(&[("hi", (ATTEMPT_MS - 1_000) as f64 / 1000.0)]);
    assert_eq!(texts(&feed, &first), ["hi"]);
    let both = transcript(&[
        ("hi", (ATTEMPT_MS - 1_000) as f64 / 1000.0),
        ("hi", (ATTEMPT_MS + 1_200) as f64 / 1000.0),
    ]);
    assert!(texts(&feed, &both).is_empty());
}

#[test]
fn a_shell_command_is_matched_as_the_transcript_writes_it() {
    let feed = feed(&[wire("o1", "sending", "!ls -la")]);
    let after = transcript(&[("! ls -la", 1_794_878_654.783)]);
    assert!(texts(&feed, &after).is_empty());
}

#[test]
fn the_bubble_comes_back_when_the_row_leaves_sending() {
    let mut feed = feed(&[wire("o1", "sending", "hi")]);
    let after = transcript(&[("hi", 1_794_878_654.783)]);
    assert!(texts(&feed, &after).is_empty());
    let mut unsure = wire("o1", "unsure", "hi");
    unsure["why"] = json!("not_found");
    feed.apply(
        &notchling_local_api::Event {
            event: "outbox.changed".into(),
            data: json!({"row": unsure}),
        },
        ATTEMPT_MS + 20_000,
    );
    let shown = feed.bubbles_for_in(&job(), &after);
    assert_eq!(shown.len(), 1);
    assert_eq!(shown[0].status, Status::MayBeIn);
}
