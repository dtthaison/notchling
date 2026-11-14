use notchling_engine::sessions::{Agent, AgentGroup, AgentTarget};

use super::prompt_flow::{already_answered, is_gone};
use super::prompts::permission_from_needs;

fn waiting(needs: &str) -> Agent {
    Agent {
        id: "job1".into(),
        name: "job".into(),
        project: "p".into(),
        cwd: "/w/p".into(),
        group: AgentGroup::NeedsInput,
        label: "Needs input".into(),
        detail: Some(needs.into()),
        since_ms: 1,
        target: AgentTarget::Job("job1".into()),
        suggested_reply: None,
        transcript_path: None,
        host: "Background".into(),
        headless: false,
        config_dir: None,
        command: None,
        resume_session_id: None,
        at_prompt: false,
    }
}

#[test]
fn a_file_edit_is_not_offered_always_since_its_dialog_has_no_such_option() {
    for tool in ["Write", "Edit", "MultiEdit", "NotebookEdit"] {
        let r = permission_from_needs(&waiting(&format!("approve {tool}: /w/p/memory/notes.md")))
            .expect("a card");
        assert!(!r.can_always_allow, "{tool}");
    }
    for needs in [
        "approve Bash: ls -la",
        "approve WebFetch: https://example.com",
        "approve mcp__srv__do: {}",
    ] {
        let r = permission_from_needs(&waiting(needs)).expect("a card");
        assert!(r.can_always_allow, "{needs}");
    }
}

#[test]
fn the_tool_is_the_first_word_with_or_without_a_colon() {
    for needs in [
        "approve Write: /Users/me/a b/notes.md",
        "approve Write /Users/me/a b/notes.md",
    ] {
        let r = permission_from_needs(&waiting(needs)).expect("a card");
        assert_eq!(r.tool_name, "Write", "{needs}");
        assert_eq!(r.summary, "/Users/me/a b/notes.md", "{needs}");
    }
    let r = permission_from_needs(&waiting("approve Bash: echo a: b")).expect("a card");
    assert_eq!(
        (r.tool_name.as_str(), r.summary.as_str()),
        ("Bash", "echo a: b")
    );
    let bare = permission_from_needs(&waiting("approve Edit")).expect("a card");
    assert_eq!(
        (bare.tool_name.as_str(), bare.summary.as_str()),
        ("Edit", "")
    );
}

#[test]
fn a_refusal_that_the_permission_is_gone_means_it_was_answered_elsewhere() {
    use notchling_engine::reply::exec::Failure;
    assert!(is_gone(&Failure::NotPermission.to_string()));
    assert!(!is_gone(&Failure::PermissionMismatch.to_string()));
    assert!(!is_gone(&Failure::NoAlways.to_string()));
    assert!(!is_gone(&Failure::PermissionStillOpen.to_string()));
    assert!(!is_gone("the daemon didn't answer in time"));
    assert_eq!(
        already_answered("Write", "/w/memory/notes.md"),
        "Already answered in the terminal: Write notes.md"
    );
}

#[test]
fn a_held_prompt_carries_the_bash_description_from_the_daemon() {
    use crate::notch::requests::HookRequest;
    use serde_json::json;
    let with = HookRequest::from_prompt(&json!({
        "id": "p1-a", "sessionId": "s", "kind": "permission", "toolName": "Bash",
        "summary": "make handoff", "canAlwaysAllow": true,
        "input": {"command": "make handoff", "description": "Commit the handoff doc"},
    }))
    .expect("a request");
    assert_eq!(with.description, "Commit the handoff doc");
    assert_eq!(with.summary, "make handoff");
    let without = HookRequest::from_prompt(&json!({
        "id": "p2-a", "sessionId": "s", "kind": "permission", "toolName": "Bash",
        "summary": "ls", "input": {"command": "ls"},
    }))
    .expect("a request");
    assert_eq!(without.description, "");
    let old = HookRequest::from_prompt(&json!({
        "id": "p3-a", "sessionId": "s", "kind": "permission", "toolName": "Bash", "summary": "ls",
    }))
    .expect("a request");
    assert_eq!(old.description, "");
}

#[test]
fn a_hook_file_carries_the_bash_description() {
    use crate::notch::requests::HookRequest;
    use serde_json::json;
    let r = HookRequest::parse(&json!({
        "id": "r1", "session_id": "s", "pid": 4, "hook_event_name": "PermissionRequest",
        "tool_name": "Bash",
        "tool_input": {"command": "ls", "description": "List the files"},
    }))
    .expect("a request");
    assert_eq!(r.description, "List the files");
}
