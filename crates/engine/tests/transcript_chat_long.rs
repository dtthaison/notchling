mod common;

use common::*;
use notchling_engine::transcript::chat;

fn assistant_line(text: &str, uuid: &str) -> String {
    format!(
        r#"{{"type":"assistant","uuid":"{uuid}","message":{{"role":"assistant","content":[{{"type":"text","text":"{text}"}}]}}}}"#
    )
}

fn write_lines(scratch: &Scratch, lines: &[String], name: &str) -> String {
    s(&scratch.write(name, &(lines.join("\n") + "\n")))
}

#[test]
fn long_text_is_capped_only_when_it_runs_away() {
    let scratch = Scratch::new("chat");
    let cap = chat::MAX_TEXT_CHARACTERS;
    assert_eq!(cap, 100_000);
    let long = write_lines(
        &scratch,
        &[assistant_line(&"x".repeat(cap + 500), "long")],
        "long.jsonl",
    );
    let message = chat::read(&long, true, 200).messages.remove(0);
    assert_eq!(message.text.chars().count(), cap);
    assert!(message.text.ends_with('…'));
    let exact = write_lines(
        &scratch,
        &[assistant_line(&"y".repeat(cap), "exact")],
        "exact.jsonl",
    );
    let kept = chat::read(&exact, true, 200).messages.remove(0);
    assert_eq!(kept.text.chars().count(), cap);
    assert!(!kept.text.ends_with('…'));
}

#[test]
fn a_reply_of_50000_characters_arrives_whole() {
    let scratch = Scratch::new("chat");
    let reply = "r".repeat(50_000);
    let path = write_lines(&scratch, &[assistant_line(&reply, "r1")], "reply.jsonl");
    let messages = chat::read(&path, true, 200).messages;
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].text, reply);
}

#[test]
fn a_reply_line_over_100kb_arrives_whole() {
    let scratch = Scratch::new("chat");
    let report = "Đã kiểm tra và sửa lỗi hiển thị. ".repeat(2800);
    assert!(report.len() > 100_000);
    let path = write_lines(
        &scratch,
        &[assistant_line(report.trim(), "report")],
        "report.jsonl",
    );
    let messages = chat::read(&path, true, 200).messages;
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].text, report.trim());
}

#[test]
fn a_reply_line_of_2mb_is_cut_with_the_note_not_dropped() {
    let scratch = Scratch::new("chat");
    let path = write_lines(
        &scratch,
        &[
            assistant_line(&"z".repeat(2_000_000), "huge"),
            r#"{"type":"user","uuid":"u3","message":{"role":"user","content":"after"}}"#
                .to_string(),
        ],
        "huge-reply.jsonl",
    );
    let messages = chat::read(&path, true, 200).messages;
    let texts: Vec<usize> = messages.iter().map(|m| m.text.chars().count()).collect();
    assert_eq!(texts, [chat::MAX_TEXT_CHARACTERS, 5]);
    assert!(messages[0].text.ends_with('…'));
    assert_eq!(messages[1].text, "after");
}

#[test]
fn a_reply_line_of_5mb_is_cut_with_the_note_and_keeps_what_follows() {
    let scratch = Scratch::new("chat");
    let path = write_lines(
        &scratch,
        &[
            assistant_line(&"y".repeat(5_000_000), "huge"),
            r#"{"type":"user","uuid":"u3","message":{"role":"user","content":"after"}}"#
                .to_string(),
        ],
        "huge-reply-5mb.jsonl",
    );
    let result = chat::read(&path, true, 200);
    let texts: Vec<usize> = result
        .messages
        .iter()
        .map(|m| m.text.chars().count())
        .collect();
    assert_eq!(texts, [chat::MAX_TEXT_CHARACTERS, 5]);
    assert!(result.messages[0].text.ends_with('…'));
    assert!(result.messages[0].text.starts_with("yyyy"));
    assert_eq!(result.malformed_lines, 0);
}

#[test]
fn a_reply_line_of_many_small_blocks_over_1mb_still_arrives_with_its_text() {
    let scratch = Scratch::new("chat");
    let blocks = r#"{"type":"tool_use","id":"t","name":"Read","input":{}},"#.repeat(40_000);
    let line = format!(
        r#"{{"type":"assistant","uuid":"many","message":{{"role":"assistant","content":[{{"type":"text","text":"the answer"}},{blocks}{{"type":"text","text":"end"}}]}}}}"#
    );
    assert!(line.len() > 2_000_000);
    let path = write_lines(&scratch, &[line], "many-blocks.jsonl");
    let result = chat::read(&path, true, 200);
    assert_eq!(result.malformed_lines, 0);
    assert!(result
        .messages
        .iter()
        .any(|m| m.text.contains("the answer")));
}

#[test]
fn an_assistant_line_over_100kb_is_told_by_its_type_whatever_the_spacing() {
    let scratch = Scratch::new("chat");
    let text = "ý".repeat(60_000);
    let line = format!(
        r#"{{"message":{{"role":"assistant","content":[{{"type":"text","text":"{text}"}}]}},"uuid":"spaced", "type" : "assistant"}}"#
    );
    assert!(line.len() > 100_000);
    let path = write_lines(&scratch, &[line], "spaced.jsonl");
    let messages = chat::read(&path, true, 200).messages;
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].text, text);
}

#[test]
fn a_cut_user_line_over_100kb_that_quotes_an_assistant_line_is_skipped_without_a_parse() {
    let scratch = Scratch::new("chat");
    let quoted = r#"{"type":"assistant","n":1},"#.repeat(100_000);
    let cut = format!(
        r#"{{"type":"user","uuid":"q","message":{{"role":"user","content":"x"}},"toolUseResult":[{quoted}"#
    );
    assert!(cut.len() > 2_000_000);
    let path = write_lines(
        &scratch,
        &[cut, assistant_line("after", "u3")],
        "cut-user.jsonl",
    );
    let result = chat::read(&path, true, 200);
    let texts: Vec<&str> = result.messages.iter().map(|m| m.text.as_str()).collect();
    assert_eq!(texts, ["after"]);
    assert_eq!(result.malformed_lines, 0);
}

#[test]
fn a_user_tool_result_over_2mb_that_quotes_an_assistant_line_is_skipped() {
    let scratch = Scratch::new("chat");
    let quoted = r#"{"type":"assistant","n":1},"#.repeat(100_000);
    let result_line = format!(
        r#"{{"type":"user","uuid":"q","message":{{"role":"user","content":"x"}},"toolUseResult":[{quoted}{{"type":"assistant"}}]}}"#
    );
    assert!(result_line.len() > 2_000_000);
    let path = write_lines(
        &scratch,
        &[result_line, assistant_line("after", "u3")],
        "quoting-result.jsonl",
    );
    let result = chat::read(&path, true, 200);
    let texts: Vec<&str> = result.messages.iter().map(|m| m.text.as_str()).collect();
    assert_eq!(texts, ["after"]);
    assert_eq!(result.malformed_lines, 0);
}

fn thinking_blocks(count: usize) -> String {
    format!(
        r#"{{"type":"thinking","thinking":"{}"}},"#,
        "t".repeat(100_000)
    )
    .repeat(count)
}

fn user_line(text: &str) -> String {
    format!(r#"{{"type":"user","uuid":"u3","message":{{"role":"user","content":"{text}"}}}}"#)
}

#[test]
fn an_assistant_line_with_its_message_before_its_type_still_arrives() {
    let scratch = Scratch::new("chat");
    let thinking = thinking_blocks(60);
    let line = format!(
        r#"{{"message":{{"role":"assistant","content":[{{"type":"text","text":"the answer"}},{thinking}{{"type":"text","text":"the end"}}]}},"type":"assistant","uuid":"late"}}"#
    );
    assert!(line.len() > 5_000_000);
    let path = write_lines(&scratch, &[line, user_line("after")], "late-type.jsonl");
    let texts: Vec<String> = chat::read(&path, true, 200)
        .messages
        .into_iter()
        .map(|m| m.text)
        .collect();
    assert_eq!(texts, ["the answer\n\nthe end", "after"]);
}

#[test]
fn flags_after_a_huge_message_still_hide_a_sidechain_assistant_line() {
    let scratch = Scratch::new("chat");
    let thinking = thinking_blocks(60);
    let line = format!(
        r#"{{"type":"assistant","message":{{"role":"assistant","content":[{{"type":"text","text":"the answer"}},{thinking}{{"type":"text","text":"the end"}}]}},"isSidechain":true,"isMeta":false,"uuid":"side"}}"#
    );
    assert!(line.len() > 5_000_000);
    let path = write_lines(&scratch, &[line, user_line("after")], "late-flags.jsonl");
    let texts: Vec<String> = chat::read(&path, true, 200)
        .messages
        .into_iter()
        .map(|m| m.text)
        .collect();
    assert_eq!(texts, ["after"]);
}

#[test]
fn a_text_block_after_many_large_tool_blocks_is_kept() {
    let scratch = Scratch::new("chat");
    let tools = format!(
        r#"{{"type":"tool_use","id":"t","name":"Write","input":{{"file_path":"/a/b.txt","content":"{}"}}}},"#,
        "c".repeat(100_000)
    )
    .repeat(60);
    let line = format!(
        r#"{{"type":"assistant","uuid":"tools","message":{{"role":"assistant","content":[{tools}{{"type":"text","text":"last words"}}]}}}}"#
    );
    assert!(line.len() > 5_000_000);
    let path = write_lines(
        &scratch,
        &[line, user_line("after")],
        "tools-then-text.jsonl",
    );
    let result = chat::read(&path, true, 200);
    assert_eq!(result.malformed_lines, 0);
    assert!(result.messages.iter().any(|m| m.text == "last words"));
    assert!(result.messages.iter().any(|m| !m.steps.is_empty()));
}

#[test]
fn user_lines_over_100kb_are_still_skipped() {
    let scratch = Scratch::new("chat");
    let path = write_lines(
        &scratch,
        &[
            assistant_line("before", "u1"),
            format!(
                r#"{{"type":"user","uuid":"huge","message":{{"role":"user","content":"{}"}}}}"#,
                "z".repeat(150_000)
            ),
            r#"{"type":"user","uuid":"u3","message":{"role":"user","content":"after"}}"#
                .to_string(),
        ],
        "huge-user.jsonl",
    );
    let texts: Vec<String> = chat::read(&path, true, 200)
        .messages
        .into_iter()
        .map(|m| m.text)
        .collect();
    assert_eq!(texts, ["before", "after"]);
}

#[test]
fn a_line_of_many_tiny_tool_blocks_keeps_a_bounded_number_of_steps() {
    let scratch = Scratch::new("chat");
    let blocks = r#"{"type":"tool_use","id":"t","name":"Read","input":{"file_path":"/a/b.txt"}},"#
        .repeat(150_000);
    let line = format!(
        r#"{{"type":"assistant","uuid":"tiny","message":{{"role":"assistant","content":[{{"type":"text","text":"first words"}},{blocks}{{"type":"text","text":"last words"}}]}}}}"#
    );
    assert!(line.len() > 10_000_000);
    let path = write_lines(&scratch, &[line, user_line("after")], "tiny-blocks.jsonl");
    let result = chat::read(&path, true, 200);
    assert_eq!(result.malformed_lines, 0);
    assert!(result
        .messages
        .iter()
        .any(|m| m.text.contains("first words")));
    let most = result.messages.iter().map(|m| m.steps.len()).max();
    assert!(most.is_some_and(|n| n > 0 && n <= 2_000), "steps {most:?}");
}
