use super::*;
use crate::sessions::iso::parse_iso_ms;
use crate::sessions::testutil::TempDir;
use crate::transcript::message::Message;
use crate::transcript::queue_state::queue_lists;
use serde_json::json;
use std::path::Path;

const SHOT: &str = "/tmp/Shot one.png";
const ZWSP: char = '\u{200b}';

fn stamp(second: u32) -> String {
    format!("2026-11-19T06:59:{second:02}.000Z")
}

fn since(second: u32) -> i64 {
    parse_iso_ms(&stamp(second)).unwrap()
}

fn seen(enqueued: usize, taken: usize) -> Option<Seen> {
    Some(Seen { enqueued, taken })
}

fn with_picture(text: &str) -> Message {
    Message::parse(text, |path| path == Path::new(SHOT))
}

fn plain(text: &str) -> Message {
    Message::parse(text, |_| false)
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

fn user(text: &str, images: usize, second: u32) -> String {
    let mut blocks = vec![json!({"type": "text", "text": text})];
    for _ in 0..images {
        blocks.push(json!({"type": "image", "source": {"type": "base64", "data": "AAAA"}}));
    }
    json!({"type": "user", "timestamp": stamp(second), "origin": {"kind": "human"},
        "message": {"role": "user", "content": blocks}})
    .to_string()
}

fn write(lines: &[String]) -> (TempDir, String) {
    let dir = TempDir::new("sent-invisible");
    let path = dir.write("t.jsonl", &format!("{}\n", lines.join("\n")));
    let path = path.to_string_lossy().into_owned();
    (dir, path)
}

fn taken_as(stored: &str, message: &Message, images: usize) -> Option<Seen> {
    let (_dir, path) = write(&[
        enqueue(stored, 45),
        dequeue(52),
        user(stored, images, 52),
    ]);
    count_since(&path, message, since(42))
}

fn words() -> [&'static str; 8] {
    [
        "carry", "the", "blue", "lantern", "across", "the", "quiet", "bridge",
    ]
}

#[test]
fn a_picture_message_whose_words_are_joined_by_zero_width_spaces_is_taken() {
    let sentence = words().join(&ZWSP.to_string());
    let sent = format!("why this lamp \n{sentence}\n\n'{SHOT}'");
    let stored = format!("[Image #2]why this lamp \n{}", words().concat());
    assert_eq!(
        taken_as(&stored, &with_picture(&sent), 1),
        seen(1, 1),
        "Claude Code stores the prompt without the U+200B"
    );
}

#[test]
fn a_zero_width_space_beside_the_spaces_does_not_stop_the_match() {
    let sentence = words().join(&format!("{ZWSP} "));
    let sent = format!("{sentence}\n\n'{SHOT}'");
    let stored = format!("[Image #1]{}", words().join(" "));
    assert_eq!(taken_as(&stored, &with_picture(&sent), 1), seen(1, 1));
}

#[test]
fn a_message_with_no_picture_and_zero_width_spaces_is_taken() {
    let sent = words().join(&ZWSP.to_string());
    assert_eq!(taken_as(&words().concat(), &plain(&sent), 0), seen(1, 1));
}

#[test]
fn a_byte_order_mark_and_a_word_joiner_do_not_stop_the_match() {
    let sent = format!("\u{feff}carry the\u{2060} blue lantern");
    assert_eq!(
        taken_as("carry the blue lantern", &plain(&sent), 0),
        seen(1, 1)
    );
}

#[test]
fn every_character_claude_code_drops_is_ignored() {
    let dropped = [
        '\u{00ad}', '\u{034f}', '\u{061c}', '\u{115f}', '\u{1160}', '\u{17b4}', '\u{180b}',
        '\u{180e}', '\u{200b}', '\u{200c}', '\u{200d}', '\u{200e}', '\u{200f}', '\u{202a}',
        '\u{202e}', '\u{2060}', '\u{2062}', '\u{2066}', '\u{2069}', '\u{206f}', '\u{3164}',
        '\u{fe00}', '\u{fe0f}', '\u{feff}', '\u{ffa0}', '\u{fff9}', '\u{e0001}', '\u{e0041}',
        '\u{e0100}', '\u{07}', '\u{1b}', '\u{7f}', '\u{9f}',
    ];
    let missed: Vec<String> = dropped
        .iter()
        .filter(|c| {
            let sent = format!("carry the {c}lan{c}tern");
            taken_as("carry the lantern", &plain(&sent), 0) != seen(1, 1)
        })
        .map(|c| format!("U+{:04X}", *c as u32))
        .collect();
    assert!(missed.is_empty(), "not ignored: {missed:?}");
}

#[test]
fn a_line_break_of_any_kind_is_one_line_break() {
    let sent = "one\r\ntwo\rthree\u{b}four\u{c}five\u{85}six\u{2028}seven\u{2029}eight";
    let stored = "one\ntwo\nthree\nfour\nfive\nsix\nseven\neight";
    assert_eq!(taken_as(stored, &plain(sent), 0), seen(1, 1));
}

#[test]
fn text_that_differs_in_a_visible_character_is_still_not_taken() {
    let sent = words().join(&ZWSP.to_string());
    let other = "carry the blue lamp across the quiet bridge";
    assert_eq!(taken_as(other, &plain(&sent), 0), seen(0, 0));
    assert_eq!(
        taken_as(&words().concat(), &plain(&words().join(" ")), 0),
        seen(0, 0),
        "a space is not invisible: the words with spaces are another text"
    );
    assert_eq!(
        taken_as(&words().join(" "), &plain(&words().concat()), 0),
        seen(0, 0)
    );
}

#[test]
fn characters_claude_code_keeps_are_not_ignored() {
    for kept in ['\u{2800}', '\u{00b7}', '\u{2022}', '\u{fffd}', '\u{1d159}'] {
        let sent = format!("carry the {kept}lantern");
        assert_eq!(
            taken_as("carry the lantern", &plain(&sent), 0),
            seen(0, 0),
            "U+{:04X}",
            kept as u32
        );
    }
}

#[test]
fn a_picture_message_still_needs_its_image_blocks() {
    let sentence = words().join(&ZWSP.to_string());
    let sent = format!("{sentence}\n\n'{SHOT}'");
    let stored = format!("[Image #1]{}", words().concat());
    assert_eq!(taken_as(&stored, &with_picture(&sent), 0), seen(1, 0));
}

#[test]
fn a_waiting_entry_is_listed_for_its_message_through_zero_width_spaces() {
    let sent = words().join(&ZWSP.to_string());
    let (_dir, path) = write(&[enqueue(&words().concat(), 45)]);
    assert_eq!(queue_lists(&path, &plain(&sent)), Some(true));
    assert_eq!(queue_lists(&path, &plain("carry the blue lamp")), Some(false));
}

#[test]
fn sent_since_ignores_the_characters_claude_code_drops() {
    let (_dir, path) = write(&[user(&words().concat(), 0, 52)]);
    assert_eq!(
        sent_since(&path, &words().join(&ZWSP.to_string()), since(42)),
        Some(true)
    );
}
