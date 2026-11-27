use super::*;
use crate::sessions::iso::parse_iso_ms;
use crate::sessions::testutil::TempDir;
use crate::transcript::message::{is_image, Message};
use serde_json::json;
use std::path::Path;

fn stamp(second: u32) -> String {
    format!("2026-11-19T06:59:{second:02}.000Z")
}

fn since(second: u32) -> i64 {
    parse_iso_ms(&stamp(second)).unwrap()
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

fn enqueue(content: &str, second: u32) -> String {
    json!({"type": "queue-operation", "operation": "enqueue", "timestamp": stamp(second),
        "sessionId": "s", "content": content})
    .to_string()
}

fn dequeue(second: u32) -> String {
    json!({"type": "queue-operation", "operation": "dequeue", "timestamp": stamp(second),
        "sessionId": "s"})
    .to_string()
}

fn taken_as(stored: &str, message: &Message, images: usize) -> Option<Seen> {
    let dir = TempDir::new("sent-pictures");
    let lines = [enqueue(stored, 45), dequeue(52), user(stored, images, 52)];
    let path = dir.write("t.jsonl", &format!("{}\n", lines.join("\n")));
    count_since(&path.to_string_lossy(), message, since(42))
}

const KEPT_AS_TEXT: [&str; 3] = ["/tmp/Photo one.heic", "/tmp/Scan.tiff", "/tmp/old.bmp"];

#[test]
fn a_row_with_a_path_to_a_kind_claude_code_keeps_as_text_is_taken_by_the_line_with_the_path() {
    for path in KEPT_AS_TEXT {
        let sent = format!("look at this\n\n'{path}'");
        let message = Message::parse(&sent, |p| p == Path::new(path));
        assert_eq!(message.pictures, 0, "{path}");
        assert_eq!(
            taken_as(&sent, &message, 0),
            Some(Seen {
                enqueued: 1,
                taken: 1
            }),
            "{path}"
        );
    }
}

#[test]
fn the_picture_list_is_the_one_claude_code_reads() {
    for kept in [
        "a.png", "a.jpg", "a.jpeg", "a.gif", "a.webp", "A.PNG", "A.JpEg",
    ] {
        assert!(is_image(Path::new(&format!("/tmp/{kept}"))), "{kept}");
    }
    for text in ["a.heic", "a.tiff", "a.bmp", "a.svg", "a.pdf", "a.png.txt"] {
        assert!(!is_image(Path::new(&format!("/tmp/{text}"))), "{text}");
    }
}

#[test]
fn a_png_path_is_still_a_picture_that_needs_its_image_block() {
    let path = "/tmp/Shot one.png";
    let sent = format!("look at this\n\n'{path}'");
    let message = Message::parse(&sent, |p| p == Path::new(path));
    assert_eq!(message.pictures, 1);
    let stored = "[Image #1]look at this";
    let with_block = Some(Seen {
        enqueued: 1,
        taken: 1,
    });
    let without_block = Some(Seen {
        enqueued: 1,
        taken: 0,
    });
    assert_eq!(taken_as(stored, &message, 1), with_block);
    assert_eq!(taken_as(stored, &message, 0), without_block);
}
