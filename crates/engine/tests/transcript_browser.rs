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
    assert!(shot.output.is_none());
    assert!(shot.input.is_empty());
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

fn noisy_png(width: u32, height: u32) -> Vec<u8> {
    use image::{codecs::png::PngEncoder, ImageEncoder, RgbImage};
    let mut seed = 0x1234_5678_u32;
    let mut picture = RgbImage::new(width, height);
    for pixel in picture.pixels_mut() {
        for channel in &mut pixel.0 {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            *channel = (seed >> 24) as u8;
        }
    }
    let mut out = Vec::new();
    PngEncoder::new(&mut out)
        .write_image(
            picture.as_raw(),
            width,
            height,
            image::ExtendedColorType::Rgb8,
        )
        .expect("encodes");
    out
}

fn use_line(n: usize, input: &str) -> String {
    format!(
        r#"{{"type":"assistant","message":{{"id":"m{n}","content":[{{"type":"tool_use","id":"s{n}","name":"mcp__claude-in-chrome__computer","input":{input}}}]}}}}"#
    )
}

fn big_result(n: usize, png: &[u8]) -> String {
    use base64::Engine as _;
    let data = base64::engine::general_purpose::STANDARD.encode(png);
    format!(
        r#"{{"type":"user","message":{{"role":"user","content":[{{"type":"tool_result","tool_use_id":"s{n}","content":[{{"type":"text","text":"Successfully captured screenshot (1200x800, png) - ID: ss_{n}"}},{{"type":"text","text":"Tab Context:\n- tabId 1"}},{{"type":"image","source":{{"type":"base64","media_type":"image/png","data":"{data}"}}}}]}}]}}}}"#
    )
}

#[test]
fn a_screenshot_line_over_the_cap_is_read_lazily_from_its_file() {
    use notchling_engine::transcript::browser_shot::thumbnail;
    let png = noisy_png(1200, 800);
    let lines = [
        r#"{"type":"user","message":{"role":"user","content":"go"}}"#.to_string(),
        use_line(1, r#"{"action":"screenshot","tabId":1}"#),
        big_result(1, &png),
        use_line(2, r#"{"action":"screenshot","tabId":1}"#),
        big_result(2, &png),
        r#"{"type":"assistant","message":{"id":"m9","content":[{"type":"text","text":"done"}]}}"#
            .to_string(),
    ];
    assert!(lines[2].len() > 400_000);
    let scratch = Scratch::new("chrome-big");
    let path = scratch.write("s.jsonl", &(lines.join("\n") + "\n"));
    let c = conversation::read(&s(&path));
    let calls = calls(&c);
    assert_eq!(calls.len(), 2);
    for (n, call) in calls.iter().enumerate() {
        assert!(call.finished);
        assert!(call.images().is_empty(), "no base64 held in memory");
        assert_eq!(call.shots(), 1);
        assert!(call.output.is_none());
        let [spot] = call.spots() else {
            panic!("one spot")
        };
        assert_eq!(spot.path, s(&path));
        let expected: usize = lines[..2 + 2 * n].iter().map(|l| l.len() + 1).sum();
        assert_eq!(spot.at, expected as u64);
        let thumb = thumbnail(spot, &call.id).expect("thumbnail");
        assert_eq!(&thumb[..2], [0xFF, 0xD8]);
        let decoded = image::load_from_memory(&thumb).expect("decodes");
        assert_eq!(decoded.width(), 640);
    }
}

#[test]
fn only_the_newest_oversized_screenshots_keep_their_spot() {
    let png = noisy_png(900, 500);
    let mut lines = vec![r#"{"type":"user","message":{"role":"user","content":"go"}}"#.to_string()];
    let total = KEEP_IMAGE_CALLS + 2;
    for n in 0..total {
        lines.push(use_line(n, r#"{"action":"screenshot","tabId":1}"#));
        lines.push(big_result(n, &png));
    }
    let scratch = Scratch::new("chrome-spots");
    let path = scratch.write("s.jsonl", &(lines.join("\n") + "\n"));
    let c = conversation::read(&s(&path));
    let calls = calls(&c);
    assert_eq!(calls.len(), total);
    let kept = calls.iter().filter(|call| !call.spots().is_empty()).count();
    assert_eq!(kept, KEEP_IMAGE_CALLS);
    assert!(calls.iter().all(|call| call.shots() == 1));
    assert!(calls[0].spots().is_empty());
}

#[test]
fn an_unknown_error_keeps_its_text_clipped_and_a_known_one_does_not() {
    let long = "x".repeat(2000);
    let lines = [
        r#"{"type":"user","message":{"role":"user","content":"go"}}"#.to_string(),
        use_line(1, r#"{"action":"left_click","tabId":1}"#),
        format!(
            r#"{{"type":"user","message":{{"role":"user","content":[{{"type":"tool_result","tool_use_id":"s1","is_error":true,"content":"No tab with id 1 {long}"}}]}}}}"#
        ),
        use_line(2, r#"{"action":"left_click","tabId":1}"#),
        r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"s2","is_error":true,"content":"Browser extension is not connected"}]}}"#.to_string(),
    ];
    let scratch = Scratch::new("chrome-errors");
    let path = scratch.write("s.jsonl", &(lines.join("\n") + "\n"));
    let c = conversation::read(&s(&path));
    let calls = calls(&c);
    let unknown = calls[0].output.as_deref().expect("kept");
    assert!(unknown.starts_with("No tab with id 1"));
    assert!(unknown.chars().count() <= 301, "{}", unknown.len());
    assert!(calls[1].output.is_none());
    assert_eq!(calls[1].fault(), Some(BrowserFault::NotConnected));
}

#[test]
fn console_errors_count_inside_a_batch_and_stop_at_the_tab_context() {
    let batch = r#"{"type":"assistant","message":{"id":"m1","content":[{"type":"tool_use","id":"b1","name":"mcp__claude-in-chrome__browser_batch","input":{"actions":[{"name":"navigate","input":{"url":"https://example.com"}},{"name":"read_console_messages","input":{"tabId":1}}]}}]}}"#;
    let result = r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"b1","content":[{"type":"text","text":"[navigate] Navigated"},{"type":"text","text":"[read_console_messages] [error] one\n[log] fine\n[error] two\n\nTab Context:\n- error tab"}]}]}}"#;
    let lines = [
        r#"{"type":"user","message":{"role":"user","content":"go"}}"#,
        batch,
        result,
    ];
    let scratch = Scratch::new("chrome-console");
    let path = scratch.write("s.jsonl", &(lines.join("\n") + "\n"));
    let c = conversation::read(&s(&path));
    let calls = calls(&c);
    assert_eq!(calls[0].steps()[1].count, 2);
}

#[test]
fn the_phone_path_keeps_no_chrome_inputs_output_or_pictures() {
    use base64::Engine as _;
    use notchling_engine::transcript::chat;
    let data = base64::engine::general_purpose::STANDARD.encode(noisy_png(40, 30));
    let lines = [
        r#"{"type":"user","message":{"role":"user","content":"go"}}"#.to_string(),
        use_line(
            1,
            r#"{"action":"type","text":"hunter2","tabId":1,"action_summary":"Types in the field"}"#,
        ),
        r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"s1","content":"Typed hunter2 into the field"}]}}"#.to_string(),
        r#"{"type":"assistant","message":{"id":"m2","content":[{"type":"tool_use","id":"s2","name":"mcp__claude-in-chrome__get_page_text","input":{"tabId":1}}]}}"#.to_string(),
        r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"s2","content":"SECRET PAGE TEXT"}]}}"#.to_string(),
        r#"{"type":"assistant","message":{"id":"m3","content":[{"type":"tool_use","id":"s3","name":"mcp__claude-in-chrome__javascript_tool","input":{"text":"document.cookie","tabId":1}}]}}"#.to_string(),
        r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"s3","is_error":true,"content":"Browser extension is not connected"}]}}"#
        .to_string(),
        r#"{"type":"assistant","message":{"id":"m5","content":[{"type":"tool_use","id":"s4","name":"mcp__claude-in-chrome__computer","input":{"action":"screenshot","tabId":1}}]}}"#.to_string(),
        format!(
            r#"{{"type":"user","message":{{"role":"user","content":[{{"type":"tool_result","tool_use_id":"s4","content":[{{"type":"image","source":{{"type":"base64","media_type":"image/png","data":"{data}"}}}}]}}]}}}}"#
        ),
        r#"{"type":"assistant","message":{"id":"m4","content":[{"type":"text","text":"ok"}]}}"#
            .to_string(),
    ];
    let scratch = Scratch::new("chrome-phone");
    let path = scratch.write("s.jsonl", &(lines.join("\n") + "\n"));
    let shown = chat::read_in(&s(&path), true, 50, scratch.dir.as_path());
    let steps: Vec<_> = shown.messages.iter().flat_map(|m| &m.steps).collect();
    assert_eq!(steps.len(), 4);
    let all = format!("{steps:?}");
    for secret in ["hunter2", "SECRET", "cookie"] {
        assert!(!all.contains(secret), "{secret} leaked: {all}");
    }
    assert_eq!(steps[0].summary, "Typed text: Types in the field");
    assert_eq!(steps[1].summary, "Read the page");
    assert_eq!(steps[2].summary, "Ran a script");
    assert_eq!(steps[3].summary, "Took a screenshot");
    assert_eq!(steps[0].output, None);
    assert_eq!(steps[1].output, None);
    assert_eq!(steps[2].output.as_deref(), Some("Chrome isn't connected"));
    assert!(shown.messages.iter().all(|m| m.images.is_empty()));
}
