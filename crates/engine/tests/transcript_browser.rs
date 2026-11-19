mod common;

use common::*;
use notchling_engine::transcript::browser::KEEP_IMAGE_CALLS;
use notchling_engine::transcript::conversation;
use notchling_engine::transcript::model::*;

fn calls(c: &Conversation) -> Vec<&ToolCall> {
    c.items
        .iter()
        .filter_map(|i| match &i.kind {
            ItemKind::Tools(calls) => Some(calls.iter()),
            _ => None,
        })
        .flatten()
        .collect()
}

fn fixture() -> Conversation {
    conversation::read(&s(&notchling_fixture("chrome.jsonl")))
}

#[test]
fn chrome_calls_are_browser_calls_with_steps() {
    let c = fixture();
    let calls = calls(&c);
    assert_eq!(calls.len(), 17);
    assert!(calls.iter().all(|call| call.kind == ToolKind::Browser));
    assert!(calls.iter().all(|call| call.finished));
    let verbs: Vec<BrowserVerb> = calls.iter().map(|call| call.steps()[0].verb).collect();
    assert_eq!(
        &verbs[..8],
        [
            BrowserVerb::ListTabs,
            BrowserVerb::NewTab,
            BrowserVerb::Open,
            BrowserVerb::Screenshot,
            BrowserVerb::Find,
            BrowserVerb::Click,
            BrowserVerb::Type,
            BrowserVerb::Key,
        ]
    );
    assert_eq!(calls[2].steps()[0].subject, "example.com");
    assert_eq!(calls[4].steps()[0].subject, "Sign Up button");
    assert_eq!(calls[5].steps()[0].subject, "Clicks the Sign Up button");
    assert_eq!(calls[7].steps()[0].subject, "Enter");
}

#[test]
fn a_batch_lists_its_steps() {
    let c = fixture();
    let calls = calls(&c);
    let batch = calls[8];
    let verbs: Vec<BrowserVerb> = batch.steps().iter().map(|s| s.verb).collect();
    assert_eq!(
        verbs,
        [
            BrowserVerb::Open,
            BrowserVerb::Wait,
            BrowserVerb::Screenshot,
            BrowserVerb::Find,
            BrowserVerb::Scroll,
        ]
    );
    assert_eq!(batch.steps()[0].subject, "example.com");
    assert_eq!(batch.steps()[4].subject, "down");
    assert_eq!(batch.shots(), 1);
}

#[test]
fn the_console_step_counts_errors() {
    let c = fixture();
    let calls = calls(&c);
    assert_eq!(calls[9].steps()[0].verb, BrowserVerb::Console);
    assert_eq!(calls[9].steps()[0].count, 3);
}

#[test]
fn faults_come_from_the_result_text() {
    let c = fixture();
    let calls = calls(&c);
    assert_eq!(calls[14].fault(), Some(BrowserFault::NotConnected));
    assert!(calls[14].is_error);
    assert_eq!(calls[15].fault(), Some(BrowserFault::NeedsPermission));
    assert_eq!(calls[0].fault(), None);
}

#[test]
fn screenshots_keep_their_pictures_and_decode() {
    let c = fixture();
    let calls = calls(&c);
    let shot = calls[3];
    assert_eq!(shot.shots(), 1);
    assert_eq!(shot.images().len(), 1);
    assert_eq!(shot.images()[0].media_type, "image/png");
    let bytes = shot.images()[0].bytes().expect("decodes");
    assert_eq!(&bytes[..4], [0x89, b'P', b'N', b'G']);
    assert!(shot
        .output
        .as_deref()
        .is_some_and(|o| o.contains("[Image]")));
}

#[test]
fn only_the_newest_screenshots_stay_in_memory() {
    let mut lines = vec![r#"{"type":"user","message":{"role":"user","content":"go"}}"#.to_string()];
    let total = KEEP_IMAGE_CALLS + 3;
    for n in 0..total {
        lines.push(format!(
            r#"{{"type":"assistant","message":{{"id":"m{n}","content":[{{"type":"tool_use","id":"s{n}","name":"mcp__claude-in-chrome__computer","input":{{"action":"screenshot","tabId":1}}}}]}}}}"#
        ));
        lines.push(format!(
            r#"{{"type":"user","message":{{"role":"user","content":[{{"type":"tool_result","tool_use_id":"s{n}","content":[{{"type":"text","text":"ok"}},{{"type":"image","source":{{"type":"base64","media_type":"image/png","data":"AAAA"}}}}]}}]}}}}"#
        ));
    }
    let scratch = Scratch::new("chrome-shots");
    let path = scratch.write("s.jsonl", &(lines.join("\n") + "\n"));
    let c = conversation::read(&s(&path));
    let calls = calls(&c);
    assert_eq!(calls.len(), total);
    let kept = calls
        .iter()
        .filter(|call| !call.images().is_empty())
        .count();
    assert_eq!(kept, KEEP_IMAGE_CALLS);
    assert!(calls.iter().all(|call| call.shots() == 1));
    assert!(calls[0].images().is_empty());
    assert!(!calls[total - 1].images().is_empty());
}

#[test]
fn a_screenshot_line_over_the_cap_is_a_picture_not_kept() {
    let big = "A".repeat(450_000);
    let lines = [
        r#"{"type":"user","message":{"role":"user","content":"go"}}"#.to_string(),
        r#"{"type":"assistant","message":{"id":"m1","content":[{"type":"tool_use","id":"s1","name":"mcp__claude-in-chrome__computer","input":{"action":"screenshot","tabId":1}}]}}"#.to_string(),
        format!(
            r#"{{"type":"user","message":{{"role":"user","content":[{{"type":"tool_result","tool_use_id":"s1","content":[{{"type":"image","source":{{"type":"base64","media_type":"image/jpeg","data":"{big}"}}}}]}}]}}}}"#
        ),
    ];
    let scratch = Scratch::new("chrome-big");
    let path = scratch.write("s.jsonl", &(lines.join("\n") + "\n"));
    let c = conversation::read(&s(&path));
    let calls = calls(&c);
    assert_eq!(calls.len(), 1);
    assert!(calls[0].finished);
    assert!(calls[0].images().is_empty());
    assert_eq!(calls[0].shots(), 1);
}
