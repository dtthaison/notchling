use notchling_engine::sessions::iso::parse_iso_ms;
use notchling_engine::terminal::input_box::holds_message;
use notchling_engine::transcript::message::Message;
use notchling_engine::transcript::queue_state::QueueState;
use notchling_engine::transcript::sent::{count_since, Seen};
use serde_json::json;
use std::path::{Path, PathBuf};

const SHOT: &str = "/tmp/Shot one.png";
const BOXED: &str = "\u{2764}\u{FE0F} \u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F467} \u{1F3F4}\u{E0067}\u{E0062}\u{E0065}\u{E006E}\u{E0067}\u{E007F}";

struct Folder(PathBuf);

impl Drop for Folder {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn stamp(second: u32) -> String {
    format!("2026-11-19T06:59:{second:02}.000Z")
}

fn since() -> i64 {
    parse_iso_ms(&stamp(42)).unwrap()
}

fn transcript(tag: usize, stored: &str, images: usize) -> (Folder, String) {
    let dir = std::env::temp_dir().join(format!("nl-parity-{}-{tag}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let mut blocks = vec![json!({"type": "text", "text": stored})];
    for _ in 0..images {
        blocks.push(json!({"type": "image", "source": {"type": "base64", "data": "AAAA"}}));
    }
    let lines = [
        json!({"type": "queue-operation", "operation": "enqueue", "timestamp": stamp(45),
            "sessionId": "s", "content": stored}),
        json!({"type": "queue-operation", "operation": "dequeue", "timestamp": stamp(52),
            "sessionId": "s"}),
        json!({"type": "user", "timestamp": stamp(52), "origin": {"kind": "human"},
            "message": {"role": "user", "content": blocks}}),
    ];
    let text: Vec<String> = lines.iter().map(ToString::to_string).collect();
    let path = dir.join("t.jsonl");
    std::fs::write(&path, format!("{}\n", text.join("\n"))).unwrap();
    (Folder(dir), path.to_string_lossy().into_owned())
}

struct Case {
    name: &'static str,
    row: String,
    stored: String,
    images: usize,
    in_box: String,
}

fn cases() -> Vec<Case> {
    let words = ["carry", "the", "blue", "lantern", "across", "the", "bridge"];
    vec![
        Case {
            name: "plain",
            row: "carry the blue lantern".into(),
            stored: "carry the blue lantern".into(),
            images: 0,
            in_box: "carry the blue lantern".into(),
        },
        Case {
            name: "zero width spaces",
            row: words.join("\u{200B}"),
            stored: words.concat(),
            images: 0,
            in_box: words.concat(),
        },
        Case {
            name: "windows line breaks",
            row: "one\r\ntwo\r\nthree".into(),
            stored: "one\ntwo\nthree".into(),
            images: 0,
            in_box: "one two three".into(),
        },
        Case {
            name: "picture",
            row: format!("why this lamp\n\n'{SHOT}'"),
            stored: "[Image #2]why this lamp".into(),
            images: 1,
            in_box: "[Image #2]why this lamp".into(),
        },
        Case {
            name: "picture with zero width spaces and windows breaks",
            row: format!("why\u{200B} this lamp\r\n\r\n'{SHOT}'"),
            stored: "[Image #2]why this lamp".into(),
            images: 1,
            in_box: "[Image #2]why this lamp".into(),
        },
        Case {
            name: "shell line",
            row: "!ls -la".into(),
            stored: "!ls -la".into(),
            images: 0,
            in_box: "!ls -la".into(),
        },
        Case {
            name: "emoji kept in context",
            row: format!("so happy {BOXED}"),
            stored: format!("so happy {BOXED}"),
            images: 0,
            in_box: format!("so happy {BOXED}"),
        },
        Case {
            name: "emoji dropped by Claude Code",
            row: format!("so happy {BOXED}"),
            stored: "so happy \u{2764} \u{1F468}\u{1F469}\u{1F467} \u{1F3F4}".into(),
            images: 0,
            in_box: "so happy \u{2764} \u{1F468}\u{1F469}\u{1F467} \u{1F3F4}".into(),
        },
        Case {
            name: "heic kept as text",
            row: "look\n\n'/tmp/Photo one.heic'".into(),
            stored: "look\n\n'/tmp/Photo one.heic'".into(),
            images: 0,
            in_box: "look '/tmp/Photo one.heic'".into(),
        },
    ]
}

#[test]
fn the_row_the_transcript_the_queue_and_the_box_give_one_answer() {
    for (tag, case) in cases().iter().enumerate() {
        let message = Message::parse(&case.row, |path| path == Path::new(SHOT));
        let (_dir, path) = transcript(tag, &case.stored, case.images);
        assert_eq!(
            count_since(&path, &message, since()),
            Some(Seen {
                enqueued: 1,
                taken: 1
            }),
            "{}: the transcript",
            case.name
        );
        let queue = QueueState {
            waiting: vec![case.stored.clone()],
            unknown: 0,
        };
        assert_eq!(queue.listed(&message), 1, "{}: the queue", case.name);
        assert!(
            holds_message(&case.in_box, &case.row),
            "{}: the box",
            case.name
        );
    }
}

#[test]
fn texts_that_differ_in_a_visible_character_give_different_answers() {
    for (tag, case) in cases().iter().enumerate() {
        let message = Message::parse(&case.row, |path| path == Path::new(SHOT));
        let other = format!("{} extra", case.stored);
        let (_dir, path) = transcript(100 + tag, &other, case.images);
        let seen = count_since(&path, &message, since());
        assert_ne!(
            seen,
            Some(Seen {
                enqueued: 1,
                taken: 1
            }),
            "{}: the transcript",
            case.name
        );
        let queue = QueueState {
            waiting: vec![other.clone()],
            unknown: 0,
        };
        assert_eq!(queue.listed(&message), 0, "{}: the queue", case.name);
        assert!(!holds_message(&other, &case.row), "{}: the box", case.name);
    }
}
