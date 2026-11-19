use super::*;
use serde_json::json;

fn obj(v: Value) -> Map<String, Value> {
    v.as_object().cloned().unwrap_or_default()
}

#[test]
fn only_the_extensions_tools_are_browser_tools() {
    assert!(is_browser("mcp__claude-in-chrome__computer"));
    assert!(!is_browser("mcp__muse__muse_chat"));
    assert!(!is_browser("WebFetch"));
}

#[test]
fn computer_actions_map_to_verbs() {
    let step = |v: Value| steps("mcp__claude-in-chrome__computer", &obj(v)).remove(0);
    assert_eq!(
        step(json!({"action": "right_click"})).verb,
        BrowserVerb::Click
    );
    assert_eq!(
        step(json!({"action": "key", "text": "Escape"})).subject,
        "Escape"
    );
    assert_eq!(
        step(json!({"action": "scroll", "scroll_direction": "UP"})).subject,
        "up"
    );
    assert_eq!(
        step(json!({"action": "left_click_drag"})).verb,
        BrowserVerb::Drag
    );
    assert_eq!(step(json!({"action": "nope"})).verb, BrowserVerb::Other);
}

#[test]
fn typed_text_is_never_the_subject() {
    let s = steps(
        "mcp__claude-in-chrome__computer",
        &obj(json!({"action": "type", "text": "hunter2"})),
    );
    assert_eq!(s[0].verb, BrowserVerb::Type);
    assert_eq!(s[0].subject, "");
}

#[test]
fn a_batch_without_actions_has_no_steps_and_a_long_one_is_cut() {
    assert!(steps("mcp__claude-in-chrome__browser_batch", &obj(json!({}))).is_empty());
    let actions: Vec<Value> = (0..100)
        .map(|_| json!({"name": "computer", "input": {"action": "wait"}}))
        .collect();
    let s = steps(
        "mcp__claude-in-chrome__browser_batch",
        &obj(json!({"actions": actions})),
    );
    assert_eq!(s.len(), BATCH_MAX);
}

#[test]
fn faults_are_told_apart() {
    assert_eq!(
        fault_of("Browser extension is not connected. Please ensure ..."),
        Some(BrowserFault::NotConnected)
    );
    assert_eq!(
        fault_of("\"navigate\" call was never delivered to the Chrome extension: the computer ..."),
        Some(BrowserFault::NotConnected)
    );
    assert_eq!(
        fault_of("No response to the Claude in Chrome permission prompt"),
        Some(BrowserFault::NeedsPermission)
    );
    assert_eq!(
        fault_of("Navigation to this domain is not allowed"),
        Some(BrowserFault::NeedsPermission)
    );
    assert_eq!(
        fault_of("Permission denied by user"),
        Some(BrowserFault::Denied)
    );
    assert_eq!(fault_of("Tab 5 does not exist"), None);
}

#[test]
fn console_errors_count_error_lines_only() {
    assert_eq!(console_errors("[error] a\n[log] b\n[ERROR] c\nwarn d"), 2);
    assert_eq!(console_errors(""), 0);
}

#[test]
fn images_come_out_of_a_result_in_order() {
    let content = json!([
        {"type": "text", "text": "x"},
        {"type": "image", "source": {"type": "base64", "media_type": "image/jpeg", "data": "AAAA"}},
        {"type": "image", "source": {"type": "base64", "data": "BBBB"}},
        {"type": "image", "source": {"type": "url"}}
    ]);
    let images = images_of(Some(&content));
    assert_eq!(images.len(), 2);
    assert_eq!(images[0].media_type, "image/jpeg");
    assert_eq!(images[1].media_type, "image/png");
    assert!(images_of(Some(&json!("text"))).is_empty());
    assert!(images_of(None).is_empty());
}

#[test]
fn a_bad_picture_does_not_decode() {
    let image = ToolImage {
        media_type: "image/png".into(),
        data: "!!not base64!!".into(),
    };
    assert!(image.bytes().is_none());
}
