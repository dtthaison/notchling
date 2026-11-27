use super::bounded::{is_assistant, parse_object};
use super::MAX_TEXT_CHARACTERS;
use serde_json::Value;

fn object(line: &str) -> Value {
    Value::Object(parse_object(line.as_bytes()).expect("a line that parses"))
}

fn nodes(value: &Value) -> usize {
    1 + match value {
        Value::Array(items) => items.iter().map(nodes).sum(),
        Value::Object(fields) => fields.values().map(nodes).sum(),
        _ => 0,
    }
}

#[test]
fn a_line_is_an_assistant_line_by_its_top_level_type_only() {
    assert!(is_assistant(r#"{"type":"assistant","uuid":"a"}"#));
    assert!(is_assistant(r#"{"uuid":"a", "type" : "assistant"}"#));
    assert!(is_assistant(
        r#"{"message":{"content":"x"},"uuid":"a","type":"assistant"}"#
    ));
    assert!(!is_assistant(
        r#"{"type":"user","message":{"content":"{\"type\":\"assistant\"}"}}"#
    ));
    assert!(!is_assistant(
        r#"{"type":"user","message":{"type":"assistant"}}"#
    ));
    assert!(!is_assistant(r#"{"type":7}"#));
    assert!(!is_assistant(r#"["type","assistant"]"#));
    assert!(!is_assistant(r#"{"type":"assistant""#));
    assert!(!is_assistant(""));
}

#[test]
fn an_escaped_or_long_type_is_not_an_assistant_line() {
    let escaped = format!(r#"{{"type":"assist{}u0061nt"}}"#, '\\');
    assert!(escaped.contains('\\'));
    assert!(!is_assistant(&escaped));
    let long = format!(r#"{{"type":"assistant{}"}}"#, " ".repeat(40));
    assert!(!is_assistant(&long));
    let escaped_and_huge = format!(r#"{{"type":"user\n{}"}}"#, "x".repeat(2_000_000));
    assert!(!is_assistant(&escaped_and_huge));
}

#[test]
fn a_small_assistant_line_reads_as_the_plain_parser_reads_it() {
    let line = r#"{"type":"assistant","uuid":"u1","timestamp":"2026-11-25T10:00:00.000Z","isSidechain":false,"message":{"role":"assistant","model":"m","content":[{"type":"text","text":"hi"},{"type":"tool_use","id":"t1","name":"Write","input":{"file_path":"/a","content":"x\ny","n":3,"f":1.5,"ok":true,"none":null}}]}}"#;
    let plain: Value = serde_json::from_str(line).expect("plain");
    let mut expected = plain.clone();
    expected["message"]
        .as_object_mut()
        .expect("message")
        .remove("role");
    assert_eq!(object(line), expected);
}

#[test]
fn only_the_fields_chat_reads_are_kept_and_order_does_not_matter() {
    let message = r#""message":{"content":[{"type":"thinking","thinking":"secret"},{"type":"text","text":"answer"}],"model":"m","usage":{"a":1}}"#;
    let flags =
        r#""isMeta":true,"isSidechain":true,"isCompactSummary":true,"uuid":"u","timestamp":"t""#;
    let forward = object(&format!(
        r#"{{"type":"assistant",{flags},{message},"cwd":"/x"}}"#
    ));
    let backward = object(&format!(
        r#"{{"cwd":"/x",{message},{flags},"type":"assistant"}}"#
    ));
    assert_eq!(forward, backward);
    assert_eq!(forward["isSidechain"], true);
    assert_eq!(forward["isMeta"], true);
    assert_eq!(forward["isCompactSummary"], true);
    assert_eq!(forward["uuid"], "u");
    assert_eq!(forward["message"]["model"], "m");
    assert_eq!(
        forward["message"]["content"].as_array().map(Vec::len),
        Some(1)
    );
    assert_eq!(forward["message"]["content"][0]["text"], "answer");
    assert!(forward.get("cwd").is_none());
    assert!(forward["message"].get("usage").is_none());
}

#[test]
fn a_line_that_is_not_one_object_is_refused() {
    assert!(parse_object(b"[1,2]").is_none());
    assert!(parse_object(b"{\"a\":1} trailing").is_none());
    assert!(parse_object(b"{\"type\":").is_none());
}

#[test]
fn a_reply_string_keeps_its_first_characters_and_one_more_than_the_cap() {
    let text = "日".repeat(MAX_TEXT_CHARACTERS * 3);
    let line = format!(
        r#"{{"type":"assistant","message":{{"content":[{{"type":"text","text":"{text}"}}]}}}}"#
    );
    let value = object(&line);
    let kept = value["message"]["content"][0]["text"]
        .as_str()
        .expect("text");
    assert_eq!(kept.chars().count(), MAX_TEXT_CHARACTERS + 1);
    assert!(kept.chars().all(|c| c == '日'));
}

#[test]
fn a_plain_string_reply_is_cut_the_same_way() {
    let text = "w".repeat(MAX_TEXT_CHARACTERS * 2);
    let value = object(&format!(
        r#"{{"type":"assistant","message":{{"content":"{text}"}}}}"#
    ));
    let kept = value["message"]["content"].as_str().expect("content");
    assert_eq!(kept.chars().count(), MAX_TEXT_CHARACTERS + 1);
}

#[test]
fn a_tool_input_keeps_its_keys_and_cuts_long_strings_and_wide_containers() {
    let long = "q".repeat(100_000);
    let items = "1,".repeat(5_000);
    let deep = "[".repeat(10) + &"]".repeat(10);
    let line = format!(
        r#"{{"type":"assistant","message":{{"content":[{{"type":"tool_use","id":"t","name":"X","input":{{"content":"{long}","list":[{items}0],"deep":{deep},"edits":[{{"old_string":"a","new_string":"b"}}]}}}}]}}}}"#
    );
    let value = object(&line);
    let input = &value["message"]["content"][0]["input"];
    assert_eq!(
        input["content"].as_str().map(|s| s.chars().count()),
        Some(20_000)
    );
    assert_eq!(input["list"].as_array().map(Vec::len), Some(1_000));
    assert!(input["deep"].is_array());
    assert_eq!(input["edits"][0]["new_string"], "b");
}

#[test]
fn big_unread_blocks_cost_nothing_and_a_text_block_after_many_tool_blocks_is_kept() {
    let thinking = format!(
        r#"{{"type":"thinking","thinking":"{}"}},"#,
        "t".repeat(100_000)
    )
    .repeat(60);
    let tools = format!(
        r#"{{"type":"tool_use","id":"t","name":"Write","input":{{"file_path":"/a","content":"{}"}}}},"#,
        "c".repeat(100_000)
    )
    .repeat(60);
    let line = format!(
        r#"{{"type":"assistant","message":{{"content":[{thinking}{tools}{{"type":"text","text":"last words"}}]}}}}"#
    );
    assert!(line.len() > 10_000_000);
    let value = object(&line);
    let blocks = value["message"]["content"].as_array().expect("blocks");
    assert_eq!(blocks.len(), 61);
    assert_eq!(blocks[60]["text"], "last words");
    assert!(nodes(&value) < 1_000);
}

#[test]
fn a_huge_key_is_cut() {
    let key = "k".repeat(1_000_000);
    let line = format!(
        r#"{{"type":"assistant","message":{{"content":[{{"type":"tool_use","name":"X","input":{{"{key}":"v"}}}}]}}}}"#
    );
    let value = object(&line);
    let input = value["message"]["content"][0]["input"]
        .as_object()
        .expect("input");
    assert!(input.keys().all(|k| k.chars().count() <= 256));
}

#[test]
fn many_tiny_blocks_keep_at_most_the_block_cap() {
    let blocks = r#"{"type":"tool_use","id":"t","name":"Read","input":{}},"#.repeat(300_000);
    let line = format!(
        r#"{{"type":"assistant","message":{{"content":[{{"type":"text","text":"first"}},{blocks}{{"type":"text","text":"last"}}]}}}}"#
    );
    assert!(line.len() > 10_000_000);
    let value = object(&line);
    let kept = value["message"]["content"].as_array().expect("blocks");
    assert_eq!(kept.len(), 2_000);
    assert_eq!(kept[0]["text"], "first");
}
