use super::*;
use crate::sessions::iso::parse_iso_ms;
use crate::sessions::testutil::TempDir;
use crate::transcript::message::Message;
use crate::transcript::queue_state::QueueState;
use serde_json::json;
use std::path::Path;

const SHOT: &str = "/Users/dev/Library/Application Support/Notchling/drops/k3x9/Screenshot 2026-11-20 at 10.14.52.png";
const OTHER: &str = "/Users/dev/Library/Application Support/Notchling/drops/k3x9/Screenshot 2026-11-20 at 10.15.01.png";

fn stamp(second: u32) -> String {
    format!("2026-11-20T03:15:{second:02}.000Z")
}

fn since() -> i64 {
    parse_iso_ms(&stamp(11)).unwrap()
}

fn user(text: &str, images: usize, second: u32) -> String {
    let mut blocks = vec![json!({"type": "text", "text": text})];
    for _ in 0..images {
        blocks.push(json!({"type": "image", "source": {"type": "base64", "data": "AAAA"}}));
    }
    json!({"type": "user", "timestamp": stamp(second), "origin": {"kind": "human"},
        "message": {"role": "user", "content": blocks}})
    .to_string()
}

fn the_row() -> String {
    format!("is it right\n\n'{SHOT}'")
}

fn message(row: &str) -> Message {
    Message::parse(row, |path| {
        path == Path::new(SHOT) || path == Path::new(OTHER)
    })
}

fn counted(message: &Message, stored: &str, images: usize) -> Option<Seen> {
    let dir = TempDir::new("sent-kept");
    let path = dir.write("t.jsonl", &format!("{}\n", user(stored, images, 24)));
    count_since(&path.to_string_lossy(), message, since())
}

fn counted_queued(message: &Message, stored: &str, images: usize) -> Option<Seen> {
    let enqueue = json!({"type": "queue-operation", "operation": "enqueue",
        "timestamp": stamp(20), "sessionId": "s", "content": stored})
    .to_string();
    let dequeue = json!({"type": "queue-operation", "operation": "dequeue",
        "timestamp": stamp(24), "sessionId": "s"})
    .to_string();
    let dir = TempDir::new("sent-kept-queued");
    let body = [enqueue, dequeue, user(stored, images, 24)].join("\n");
    let path = dir.write("t.jsonl", &format!("{body}\n"));
    count_since(&path.to_string_lossy(), message, since())
}

const TAKEN: Option<Seen> = Some(Seen {
    enqueued: 0,
    taken: 1,
});

#[test]
fn the_row_is_read_as_one_picture_and_the_words() {
    let message = message(&the_row());
    assert_eq!(message.text, "is it right");
    assert_eq!(message.pictures, 1);
}

#[test]
fn a_line_that_kept_the_quoted_path_as_text_is_the_row() {
    let message = message(&the_row());
    assert_eq!(counted(&message, &the_row(), 0), TAKEN);
}

#[test]
fn a_kept_path_is_the_row_whatever_the_line_breaks_quotes_and_trailing_space() {
    let message = message(&the_row());
    let escaped = SHOT.replace(' ', "\\ ");
    let forms = [
        format!("is it right  '{SHOT}'"),
        format!("is it right\n\n'{SHOT}' "),
        format!("is it right\r\n\r\n'{SHOT}'\n"),
        format!("is it right\n\n{escaped}"),
        format!("is it right '{SHOT}'\u{a0}"),
    ];
    for stored in forms {
        assert_eq!(counted(&message, &stored, 0), TAKEN, "{stored:?}");
    }
}

#[test]
fn a_kept_path_with_the_picture_attached_as_well_is_the_row() {
    let message = message(&the_row());
    assert_eq!(counted(&message, &the_row(), 1), TAKEN);
}

#[test]
fn a_chip_with_its_picture_is_still_the_row() {
    let message = message(&the_row());
    assert_eq!(counted(&message, "[Image #7]is it right", 1), TAKEN);
    assert_eq!(counted(&message, "[Image #7] is it right ", 1), TAKEN);
}

#[test]
fn a_chip_without_its_picture_is_still_not_the_row() {
    let message = message(&the_row());
    assert_eq!(
        counted(&message, "[Image #7]is it right", 0),
        Some(Seen::default())
    );
}

#[test]
fn a_queued_line_with_the_kept_path_counts_as_enqueued_and_taken() {
    let message = message(&the_row());
    assert_eq!(
        counted_queued(&message, &the_row(), 0),
        Some(Seen {
            enqueued: 1,
            taken: 1
        })
    );
    let queue = QueueState {
        waiting: vec![the_row()],
        unknown: 0,
    };
    assert_eq!(queue.listed(&message), 1);
}

#[test]
fn two_pictures_one_read_and_one_kept_are_the_row() {
    let row = format!("look\n\n'{SHOT}' '{OTHER}'");
    let message = message(&row);
    assert_eq!(message.pictures, 2);
    let stored = format!("[Image #3]look '{OTHER}'");
    assert_eq!(counted(&message, &stored, 1), TAKEN);
    let both = format!("look '{SHOT}' '{OTHER}'");
    assert_eq!(counted(&message, &both, 0), TAKEN);
}

#[test]
fn other_words_beside_a_kept_path_are_not_the_row() {
    let message = message(&the_row());
    for stored in [
        format!("is it wrong\n\n'{SHOT}'"),
        format!("is it right and good\n\n'{SHOT}'"),
        format!("is it\n\n'{SHOT}'"),
    ] {
        assert_eq!(
            counted(&message, &stored, 0),
            Some(Seen::default()),
            "{stored:?}"
        );
    }
}

#[test]
fn words_alone_are_not_the_row_while_its_picture_is_missing() {
    let message = message(&the_row());
    assert_eq!(counted(&message, "is it right", 0), Some(Seen::default()));
}

#[test]
fn fewer_pictures_than_the_row_sent_are_not_the_row() {
    let row = format!("look\n\n'{SHOT}' '{OTHER}'");
    let message = message(&row);
    let stored = format!("look '{SHOT}'");
    assert_eq!(counted(&message, &stored, 0), Some(Seen::default()));
}

#[test]
fn a_row_with_no_picture_never_matches_a_line_that_has_a_path() {
    let message = Message::parse("is it right", |_| false);
    assert_eq!(counted(&message, &the_row(), 0), Some(Seen::default()));
}

#[test]
fn a_path_that_is_not_a_picture_is_not_cut_from_the_line() {
    let message = message(&the_row());
    let stored = format!("is it right\n\n'/tmp/notes.txt' '{SHOT}'");
    assert_eq!(counted(&message, &stored, 0), Some(Seen::default()));
}

#[test]
fn a_kept_path_of_another_picture_is_not_the_row() {
    let message = message(&the_row());
    let stored = format!("is it right\n\n'{OTHER}'");
    assert_eq!(counted(&message, &stored, 0), Some(Seen::default()));
    let stored = "is it right\n\n'/tmp/never-sent.png'";
    assert_eq!(counted(&message, stored, 0), Some(Seen::default()));
}

#[test]
fn a_queue_entry_with_fewer_pictures_than_the_row_is_not_listed() {
    let row = format!("look\n\n'{SHOT}' '{OTHER}'");
    let message = message(&row);
    let one = QueueState {
        waiting: vec![format!("look '{SHOT}'")],
        unknown: 0,
    };
    assert_eq!(one.listed(&message), 0);
    let chip_and_path = QueueState {
        waiting: vec![format!("[Image #3]look '{OTHER}'")],
        unknown: 0,
    };
    assert_eq!(chip_and_path.listed(&message), 1);
}

#[test]
fn a_repeated_path_does_not_stand_in_for_a_missing_one() {
    let row = format!("look\n\n'{SHOT}' '{OTHER}'");
    let message = message(&row);
    let twice = format!("look '{SHOT}' '{SHOT}'");
    assert_eq!(counted(&message, &twice, 0), Some(Seen::default()));
    let queue = QueueState {
        waiting: vec![twice],
        unknown: 0,
    };
    assert_eq!(queue.listed(&message), 0);
}

#[test]
fn a_queue_entry_with_the_words_but_no_picture_is_not_listed() {
    let message = message(&the_row());
    let bare = QueueState {
        waiting: vec!["is it right".to_string()],
        unknown: 0,
    };
    assert_eq!(bare.listed(&message), 0);
    let chipped = QueueState {
        waiting: vec!["[Image #4]is it right".to_string()],
        unknown: 0,
    };
    assert_eq!(chipped.listed(&message), 1);
}
