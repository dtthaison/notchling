//! Chat's transcript window is a budget of parsed bytes, not of file bytes: base64 images (a Read of
//! a .jpg writes one line of 0.5 to 1.8 MB) must not push the conversation before them out of the
//! read. A session with 50 turns, three huge image results, then 5 more turns shows all 55 prompts.

mod common;

use common::*;
use notchling_engine::transcript::conversation::{self, Parser};
use notchling_engine::transcript::model::*;
use notchling_engine::transcript::tail::{Cursors, LineParser};
use std::sync::Arc;

const IMAGE_BYTES: usize = 1_500_000;

fn prompt(n: usize) -> String {
    format!(
        r#"{{"type":"user","timestamp":"2026-11-08T08:{:02}:00.000Z","message":{{"role":"user","content":"prompt {n}"}}}}"#,
        n % 60
    )
}

fn answer(n: usize) -> String {
    format!(
        r#"{{"type":"assistant","timestamp":"2026-11-08T08:{:02}:01.000Z","message":{{"id":"msg_{n}","role":"assistant","model":"claude-opus-5-5","content":[{{"type":"text","text":"answer {n}"}}]}}}}"#,
        n % 60
    )
}

/// An assistant line that reads a picture.
fn read_call(id: &str) -> String {
    format!(
        r#"{{"type":"assistant","timestamp":"2026-11-08T08:30:00.000Z","message":{{"id":"msg_{id}","role":"assistant","model":"claude-opus-5-5","content":[{{"type":"tool_use","id":"{id}","name":"Read","input":{{"file_path":"/tmp/{id}.jpg"}}}}]}}}}"#
    )
}

/// What Read writes for a picture: the id first, then megabytes of base64, then the rest of the line.
fn image_result(id: &str, bytes: usize) -> String {
    let data = "A".repeat(bytes);
    format!(
        r#"{{"type":"user","message":{{"role":"user","content":[{{"tool_use_id":"{id}","type":"tool_result","content":[{{"type":"image","source":{{"type":"base64","media_type":"image/jpeg","data":"{data}"}}}}]}}]}},"timestamp":"2026-11-08T08:30:01.000Z"}}"#
    )
}

/// `before` small turns, one Read call per image with its huge result, then `after` small turns.
fn session(before: usize, images: usize, bytes: usize, after: usize) -> String {
    let mut lines = Vec::new();
    for n in 0..before {
        lines.push(prompt(n));
        lines.push(answer(n));
    }
    for i in 0..images {
        let id = format!("toolu_img{i}");
        lines.push(read_call(&id));
        lines.push(image_result(&id, bytes));
    }
    for n in before..before + after {
        lines.push(prompt(n));
        lines.push(answer(n));
    }
    lines.join("\n") + "\n"
}

fn prompts(c: &Conversation) -> Vec<String> {
    c.items
        .iter()
        .filter_map(|i| match &i.kind {
            ItemKind::User { text, .. } => Some(text.clone()),
            _ => None,
        })
        .collect()
}

fn expected(range: std::ops::Range<usize>) -> Vec<String> {
    range.map(|n| format!("prompt {n}")).collect()
}

#[test]
fn image_results_do_not_push_earlier_turns_out_of_chat() {
    let scratch = Scratch::new("longlines");
    let text = session(50, 3, IMAGE_BYTES, 5);
    assert!(
        text.len() > 4_500_000,
        "the images alone outweigh the 4 MB window"
    );
    let path = s(&scratch.write("s.jsonl", &text));
    let c = conversation::read(&path);
    assert_eq!(prompts(&c), expected(0..55), "every prompt is in the chat");
}

#[test]
fn a_skipped_image_result_still_finishes_its_step() {
    let scratch = Scratch::new("longlines-step");
    let path = s(&scratch.write("s.jsonl", &session(2, 3, IMAGE_BYTES, 1)));
    let c = conversation::read(&path);
    let calls: Vec<&ToolCall> = c
        .items
        .iter()
        .filter_map(|i| match &i.kind {
            ItemKind::Tools(calls) => Some(calls),
            _ => None,
        })
        .flatten()
        .collect();
    assert_eq!(calls.len(), 3, "the three Read steps show");
    for call in calls {
        assert!(call.finished, "{} is not running forever", call.id);
        assert!(!call.is_error);
        assert_eq!(call.output.as_deref(), Some("[Image]"));
    }
    assert_eq!(c.activity, None, "nothing is running");
}

#[test]
fn an_append_after_the_images_is_read_on_top_of_what_was_read() {
    let scratch = Scratch::new("longlines-append");
    let path = s(&scratch.write("s.jsonl", &session(50, 3, IMAGE_BYTES, 5)));
    let first = conversation::read_shared(&path);
    assert_eq!(prompts(&first), expected(0..55));
    assert!(Arc::ptr_eq(&first, &conversation::read_shared(&path)));

    scratch.append("s.jsonl", &format!("{}\n{}\n", prompt(55), answer(55)));
    let second = conversation::read_shared(&path);
    assert_eq!(prompts(&second), expected(0..56));

    // Another image result, bigger than the whole window, then a prompt.
    let id = "toolu_late";
    scratch.append(
        "s.jsonl",
        &format!(
            "{}\n{}\n{}\n",
            read_call(id),
            image_result(id, 4_500_000),
            prompt(56)
        ),
    );
    let third = conversation::read_shared(&path);
    assert_eq!(prompts(&third), expected(0..57));
}

/// Counts the lines it is given, and the bytes.
#[derive(Default)]
struct Count {
    lines: Vec<usize>,
}

impl LineParser for Count {
    fn consume(&mut self, line: &[u8]) {
        self.lines.push(line.len());
    }
}

#[test]
fn only_the_lines_that_fit_are_parsed_however_many_images_sit_between() {
    let scratch = Scratch::new("longlines-count");
    let path = s(&scratch.write("s.jsonl", &session(50, 3, IMAGE_BYTES, 5)));
    let cursors = Cursors::new(4_000_000, 4, |c: &Count| c.lines.len())
        .skipping_lines_over(400_000, 64_000_000);
    assert_eq!(cursors.read(&path), Some(50 * 2 + 3 + 5 * 2));
    let small: u64 = (50 * 2 + 3 + 5 * 2) as u64;
    assert!(
        cursors.parsed_bytes() < small * 400,
        "the images were not parsed: {} bytes",
        cursors.parsed_bytes()
    );
}

#[test]
fn the_look_back_stops_at_the_scan_limit() {
    let scratch = Scratch::new("longlines-cap");
    // 50 turns, 4.5 MB of images, 5 turns. A 3 MB scan reaches only into the images.
    let path = s(&scratch.write("s.jsonl", &session(50, 3, IMAGE_BYTES, 5)));
    let capped = Cursors::new(4_000_000, 4, |p: &Parser| Arc::new(p.conversation()))
        .skipping_lines_over(400_000, 3_000_000);
    let c = capped.read(&path).expect("read");
    assert_eq!(
        prompts(&c),
        expected(50..55),
        "it reads from the line it reached"
    );

    // A file of nothing but images and a few lines at its end: the scan gives up at the limit.
    let mut text = String::new();
    for i in 0..70 {
        text.push_str(&image_result(&format!("toolu_{i}"), 1_000_000));
        text.push('\n');
    }
    text.push_str(&format!("{}\n{}\n", prompt(0), answer(0)));
    let path = s(&scratch.write("images.jsonl", &text));
    let wide = Cursors::new(4_000_000, 4, |p: &Parser| Arc::new(p.conversation()))
        .skipping_lines_over(400_000, 64_000_000);
    let c = wide.read(&path).expect("read");
    assert_eq!(prompts(&c), expected(0..1));
}

#[test]
fn a_picture_bigger_than_the_window_appended_costs_only_its_neighbours() {
    let scratch = Scratch::new("longlines-grow");
    let path = s(&scratch.write("s.jsonl", &session(50, 3, IMAGE_BYTES, 5)));
    let cursors = Cursors::new(4_000_000, 4, |p: &Parser| Arc::new(p.conversation()))
        .skipping_lines_over(400_000, 64_000_000);
    let first = cursors.read(&path).expect("read");
    let before = cursors.parsed_bytes();

    let id = "toolu_late";
    scratch.append(
        "s.jsonl",
        &format!(
            "{}\n{}\n{}\n",
            read_call(id),
            image_result(id, 4_500_000),
            prompt(55)
        ),
    );
    let second = cursors.read(&path).expect("read");
    assert_eq!(prompts(&second), expected(0..56));
    assert!(!Arc::ptr_eq(&first, &second));
    let added = cursors.parsed_bytes() - before;
    assert!(
        added < 1_000,
        "the 4.5 MB picture was skipped and the earlier turns kept, not read again: {added} bytes"
    );
}

#[test]
fn a_transcript_without_pictures_reads_the_same_either_way() {
    let path = s(&notchling_fixture("conversation.jsonl"));
    let plain = Cursors::new(4_000_000, 4, |p: &Parser| Arc::new(p.conversation()));
    let skipping = Cursors::new(4_000_000, 4, |p: &Parser| Arc::new(p.conversation()))
        .skipping_lines_over(400_000, 64_000_000);
    assert_eq!(plain.read(&path), skipping.read(&path));
    assert_eq!(plain.parsed_bytes(), skipping.parsed_bytes());
}

#[test]
fn the_notch_keeps_the_last_prompt_behind_a_run_of_pictures() {
    use notchling_engine::transcript::snapshot;
    let scratch = Scratch::new("longlines-notch");
    // The last prompt, then the agent reads four pictures: the last 2 MB of the file is all base64.
    let mut text = session(5, 0, 0, 0);
    text.push_str(&format!("{}\n", prompt(5)));
    for i in 0..4 {
        let id = format!("toolu_n{i}");
        text.push_str(&format!(
            "{}\n{}\n",
            read_call(&id),
            image_result(&id, 1_200_000)
        ));
    }
    let path = s(&scratch.write("s.jsonl", &text));
    let shot = snapshot::read(&path);
    assert_eq!(shot.last_prompt.as_deref(), Some("prompt 5"));
    assert!(shot.lines.iter().any(|l| l.text == "answer 4"));
}
