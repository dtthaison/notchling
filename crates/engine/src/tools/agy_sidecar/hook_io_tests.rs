use super::hook_io::{parse_conversation, parse_tool, prompt_event, stop_output, tool_output};
use serde_json::{json, Value};

const ID: &str = "457369a9-35a1-40f8-92b9-26b6398b78a1";

fn event(tool: &str, args: Value) -> Vec<u8> {
    json!({
        "conversationId": ID,
        "cwd": "/Users/me/w",
        "hookEventName": "PreToolUse",
        "toolCall": {"id": "t1", "name": tool, "args": args},
        "stepIdx": 4,
    })
    .to_string()
    .into_bytes()
}

#[test]
fn a_tool_event_is_read_and_its_ids_checked() {
    let ask = parse_tool(&event("run_command", json!({"CommandLine": "npm test"}))).unwrap();
    assert_eq!(ask.conversation, ID);
    assert_eq!(ask.tool, "run_command");
    assert_eq!(ask.cwd, "/Users/me/w");
    assert!(parse_tool(b"not json").is_none());
    assert!(parse_tool(b"{}").is_none());
    let mut bad = serde_json::from_slice::<Value>(&event("run_command", json!({}))).unwrap();
    bad["conversationId"] = json!("../x");
    assert!(parse_tool(bad.to_string().as_bytes()).is_none());
    for tool in ["", "a b", "x;y", "../x"] {
        assert!(parse_tool(&event(tool, json!({}))).is_none(), "{tool}");
    }
    assert_eq!(parse_conversation(&event("x", json!({}))), Some(ID.into()));
    assert_eq!(parse_conversation(b"{\"conversationId\":\"nope\"}"), None);
}

#[test]
fn a_huge_tool_input_is_cut_before_it_goes_to_the_daemon() {
    let big = "x".repeat(50_000);
    let ask = parse_tool(&event(
        "write_to_file",
        json!({"TargetFile": "/a", "CodeContent": big}),
    ))
    .unwrap();
    assert!(ask.args.to_string().len() < 10_000);
    assert_eq!(ask.args["TargetFile"], "/a");
    let many: Vec<String> = (0..4000).map(|n| format!("k{n}")).collect();
    let ask = parse_tool(&event("x", json!({"list": many}))).unwrap();
    assert!(ask.args["note"].is_string());
}

#[test]
fn the_permission_card_gets_claude_shaped_fields_for_the_notch_and_phone() {
    let ask = parse_tool(&event(
        "run_command",
        json!({"CommandLine": "npm test", "Cwd": "/w"}),
    ))
    .unwrap();
    let e = prompt_event(&ask, &format!("agy:{ID}"));
    assert_eq!(e["hook_event_name"], "PermissionRequest");
    assert_eq!(e["session_id"], format!("agy:{ID}"));
    assert_eq!(e["tool_name"], "run_command");
    assert_eq!(e["tool_input"]["command"], "npm test");
    assert_eq!(e["tool_input"]["CommandLine"], "npm test");
    let ask = parse_tool(&event("write_to_file", json!({"TargetFile": "/a/b.txt"}))).unwrap();
    assert_eq!(
        prompt_event(&ask, "agy:x")["tool_input"]["file_path"],
        "/a/b.txt"
    );
}

#[test]
fn only_a_clear_yes_or_no_answers_for_the_user() {
    assert_eq!(
        tool_output("allow"),
        Some(json!({"decision": "auto_approve"}))
    );
    assert_eq!(tool_output("deny").unwrap()["decision"], "deny");
    for other in ["pass", "", "always", "ask"] {
        assert_eq!(tool_output(other), None, "{other}");
    }
}

#[test]
fn a_stop_ends_the_turn_at_the_next_model_call() {
    let out = stop_output();
    assert_eq!(out["terminationBehavior"], "terminate");
    assert!(out["injectSteps"][0]["errorMessage"]["userMessage"].is_string());
}
