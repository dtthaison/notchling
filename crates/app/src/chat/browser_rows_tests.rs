use super::*;
use crate::chat::rows::{self, Row};
use crate::chat::steps;
use notchling_engine::transcript::browser;
use notchling_engine::transcript::conversation;
use notchling_engine::transcript::model::{ChatItem, ItemKind, ToolKind};
use serde_json::json;
use std::path::Path;

fn step(verb: BrowserVerb, subject: &str) -> BrowserStep {
    BrowserStep {
        verb,
        subject: subject.into(),
        count: 0,
    }
}

fn call(steps: Vec<BrowserStep>) -> ToolCall {
    let mut call = ToolCall::new(
        "t",
        "mcp__claude-in-chrome__computer",
        ToolKind::Browser,
        "x".into(),
        String::new(),
    );
    call.browser_mut().steps = steps;
    call.finished = true;
    call
}

#[test]
fn each_step_reads_in_plain_words() {
    let said = |verb, subject: &str| step_title(&step(verb, subject));
    assert_eq!(said(BrowserVerb::Open, "example.com"), "Opened example.com");
    assert_eq!(said(BrowserVerb::Open, ""), "Opened a page");
    assert_eq!(said(BrowserVerb::NewTab, ""), "New tab");
    assert_eq!(said(BrowserVerb::CloseTab, ""), "Closed tab");
    assert_eq!(said(BrowserVerb::Screenshot, ""), "Took a screenshot");
    assert_eq!(said(BrowserVerb::ReadText, ""), "Read the page");
    assert_eq!(said(BrowserVerb::Script, ""), "Ran a script");
    assert_eq!(said(BrowserVerb::Switch, ""), "Switched browser");
    assert_eq!(said(BrowserVerb::Key, "Enter"), "Pressed Enter");
    assert_eq!(said(BrowserVerb::Scroll, "down"), "Scrolled down");
    assert_eq!(said(BrowserVerb::Scroll, ""), "Scrolled the page");
    assert_eq!(
        said(BrowserVerb::Find, "Sign Up"),
        "Looked for \u{201c}Sign Up\u{201d}"
    );
    assert_eq!(
        said(BrowserVerb::Click, "Opens the form"),
        "Clicked: Opens the form"
    );
    assert_eq!(said(BrowserVerb::Click, ""), "Clicked");
}

#[test]
fn the_console_row_counts_errors() {
    let mut console = step(BrowserVerb::Console, "");
    assert_eq!(step_title(&console), "Read console");
    console.count = 1;
    assert_eq!(step_title(&console), "Read console (1 error)");
    console.count = 3;
    assert_eq!(step_title(&console), "Read console (3 errors)");
}

#[test]
fn a_batch_reads_as_a_count_of_steps() {
    let batch = call(vec![
        step(BrowserVerb::Open, "example.com"),
        step(BrowserVerb::Wait, ""),
        step(BrowserVerb::Screenshot, ""),
        step(BrowserVerb::Find, "x"),
        step(BrowserVerb::Scroll, "down"),
    ]);
    let row = row(&batch);
    assert_eq!(title(&batch, row.fault.as_ref()), "5 browser steps");
    assert_eq!(row.steps.len(), 5);
    assert_eq!(row.steps[0], "Opened example.com");
    let one = call(vec![step(BrowserVerb::Wait, "")]);
    assert_eq!(title(&one, None), "Waited");
    assert!(self::row(&one).steps.is_empty());
}

#[test]
fn faults_say_what_to_do() {
    let mut c = call(vec![step(BrowserVerb::Open, "example.com")]);
    c.browser_mut().fault = Some(BrowserFault::NotConnected);
    let r = row(&c);
    assert_eq!(title(&c, r.fault.as_ref()), "Chrome isn't connected");
    assert!(r
        .fault
        .as_ref()
        .is_some_and(|f| f.hint.contains("extension")));
    c.browser_mut().fault = Some(BrowserFault::NeedsPermission);
    let r = row(&c);
    assert_eq!(
        title(&c, r.fault.as_ref()),
        "Waiting for permission in the Chrome extension"
    );
    c.browser_mut().fault = Some(BrowserFault::Denied);
    assert_eq!(
        title(&c, row(&c).fault.as_ref()),
        "Permission was refused in Chrome"
    );
}

#[test]
fn pictures_missing_from_memory_are_counted() {
    let mut c = call(vec![step(BrowserVerb::Screenshot, "")]);
    c.browser_mut().shots = 2;
    c.browser_mut().images = vec![ToolImage {
        media_type: "image/png".into(),
        data: "AAAA".into(),
    }];
    let r = row(&c);
    assert_eq!(r.shots.len(), 1);
    assert_eq!(r.missing, 1);
}

fn fixture_rows() -> Vec<rows::ToolRow> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("fixtures/chat-chrome/claude/projects/-Users-me-work-demo/chrome-demo.jsonl");
    let c = conversation::read(&path.to_string_lossy());
    c.items
        .iter()
        .filter_map(|i: &ChatItem| match rows::row_of(i) {
            Row::Tools(tools) => Some(tools),
            _ => None,
        })
        .flatten()
        .collect()
}

#[test]
fn the_fixture_conversation_becomes_chrome_rows() {
    let tools = fixture_rows();
    let titles: Vec<&str> = tools.iter().map(|t| t.title.as_str()).collect();
    assert_eq!(titles[0], "Checked open tabs");
    assert_eq!(titles[2], "Opened example.com");
    assert_eq!(titles[3], "Took a screenshot");
    assert_eq!(titles[8], "5 browser steps");
    assert_eq!(titles[9], "Read console (3 errors)");
    assert_eq!(titles[14], "Chrome isn't connected");
    assert_eq!(titles[15], "Waiting for permission in the Chrome extension");
    assert!(tools.iter().all(|t| t.kind == ToolKind::Browser));
    assert_eq!(steps::detail(&tools[8]), steps::Detail::Browser);
    assert_eq!(steps::detail(&tools[14]), steps::Detail::Browser);
    assert_eq!(steps::detail(&tools[3]), steps::Detail::None);
    assert_eq!(tools[3].browser.as_ref().map(|b| b.shots.len()), Some(1));
    assert_eq!(steps::summary(&tools[3..4]), "Took a screenshot");
    assert_eq!(steps::summary(&tools[0..3]), "Used Chrome 3 times");
}

#[test]
fn a_chrome_call_made_by_hand_is_a_browser_row() {
    let input = json!({"action": "screenshot", "tabId": 1});
    let made = browser::call(
        "c",
        "mcp__claude-in-chrome__computer",
        input.as_object().unwrap_or(&Default::default()),
    );
    let row = rows::row_of(&ChatItem {
        id: "i".into(),
        kind: ItemKind::Tools(vec![made]),
        date: None,
    });
    let Row::Tools(tools) = row else {
        panic!("not tools");
    };
    assert_eq!(tools[0].title, "Took a screenshot");
}
