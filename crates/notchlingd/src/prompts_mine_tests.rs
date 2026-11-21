use crate::acting_ops_tests::{app, setup};
use crate::consent_testkit::{shell, verified};
use notchling_local_api::{bind_identity, ops, OpSpec};
use serde_json::{json, Value};

const OWN: &str = "s-own";
const OTHER_COMMAND: &str = "planted-command-of-another-session";
const OTHER_QUESTION: &str = "planted-question-of-another-session";
const OTHER_FOLDER: &str = "/planted/folder/of/another/session";

fn permission(session: &str, command: &str, cwd: &str) -> Value {
    json!({"hook_event_name": "PermissionRequest", "session_id": session,
        "tool_name": "Bash", "cwd": cwd, "tool_input": {"command": command}})
}

fn question(session: &str, text: &str) -> Value {
    json!({"hook_event_name": "PreToolUse", "session_id": session,
        "tool_name": "AskUserQuestion", "cwd": OTHER_FOLDER,
        "tool_input": {"questions": [{"question": text, "header": "h",
            "options": [{"label": "a", "description": "b"}], "multiSelect": false}]}})
}

fn rig() -> crate::acting_ops_tests::Setup {
    let s = setup(true, &[], json!([]));
    let prompts = s.ops.prompts();
    let keep = [
        prompts.open(permission(OWN, "ls own", "/own"), None, 1),
        prompts.open(permission("s-other", OTHER_COMMAND, OTHER_FOLDER), None, 2),
        prompts.open(question("s-other", OTHER_QUESTION), None, 3),
    ];
    std::mem::forget(keep);
    s
}

#[test]
fn it_is_an_agent_read() {
    let s = rig();
    assert_eq!(s.ops.spec(ops::PROMPTS_MINE), Some(OpSpec::AGENT_READ));
}

#[test]
fn a_verified_session_reads_its_own_prompts_and_no_other() {
    let s = rig();
    let _who = bind_identity(Some(verified(OWN)));
    let reply = s.ops.call(&app(), ops::PROMPTS_MINE, &json!({})).unwrap();
    let rows = reply["rows"].as_array().unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["sessionId"], OWN);
    let text = reply.to_string();
    for planted in [OTHER_COMMAND, OTHER_QUESTION, OTHER_FOLDER, "s-other"] {
        assert!(!text.contains(planted), "{planted} leaked: {text}");
    }
}

#[test]
fn another_verified_session_with_no_prompt_gets_an_empty_list() {
    let s = rig();
    let _who = bind_identity(Some(verified("s-quiet")));
    let reply = s.ops.call(&app(), ops::PROMPTS_MINE, &json!({})).unwrap();
    assert_eq!(reply["rows"], json!([]));
    assert!(reply.get("reason").is_none());
}

#[test]
fn a_caller_with_no_verified_session_gets_nothing_and_the_reason() {
    let s = rig();
    let mut unverified = verified(OWN);
    unverified.verified = false;
    for who in [None, Some(shell("Terminal")), Some(unverified)] {
        let _bound = bind_identity(who);
        let reply = s.ops.call(&app(), ops::PROMPTS_MINE, &json!({})).unwrap();
        assert_eq!(reply["rows"], json!([]));
        assert_eq!(reply["reason"], "no_session");
        let text = reply.to_string();
        for planted in [OTHER_COMMAND, OTHER_QUESTION, OTHER_FOLDER, "ls own"] {
            assert!(!text.contains(planted), "{planted} leaked: {text}");
        }
    }
}
