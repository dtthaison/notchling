//! Ports of ACPTranscriptTests. `acp-agy.jsonl` is a real `agy acp` exchange (Antigravity CLI 1.5.1):
//! `>` lines Notchling sent, `<` lines it got.

mod common;

use common::*;
use notchling_engine::transcript::acp::{self, Transcript};
use notchling_engine::transcript::model::*;
use serde_json::{json, Value};

fn replay() -> (Transcript, Option<Value>) {
    let mut t = Transcript::default();
    let mut permission = None;
    for line in lines_of(&notchling_fixture("acp-agy.jsonl")) {
        let sent = line.starts_with('>');
        let json: Value = serde_json::from_str(&line[2..]).expect("json");
        let method = json["method"].as_str();
        if sent {
            if method == Some("session/prompt") {
                t.begin_turn(100.0);
            }
            continue;
        }
        let params = &json["params"];
        if method == Some("session/update") && params["update"].is_object() {
            t.apply(&params["update"], None);
        } else if method == Some("session/request_permission") && params["toolCall"].is_object() {
            permission = Some(params.clone());
            let mut call = params["toolCall"].clone();
            call["sessionUpdate"] = json!("tool_call_update");
            t.apply(&call, None);
        } else if json["result"]["stopReason"].is_string() {
            let usage = json["result"].get("usage");
            t.end_turn(json["result"]["stopReason"].as_str(), usage, 110.0);
        }
    }
    (t, permission)
}

#[test]
fn replay_and_live_turn() {
    let (t, _) = replay();
    let kinds: Vec<String> = t
        .items()
        .iter()
        .map(|item| match &item.kind {
            ItemKind::User { .. } => "user".to_string(),
            ItemKind::Assistant { .. } => "assistant".to_string(),
            ItemKind::Thinking { .. } => "thinking".to_string(),
            ItemKind::Tools(calls) => format!("tools({})", calls.len()),
            ItemKind::TurnEnd { .. } => "end".to_string(),
            _ => "other".to_string(),
        })
        .collect();
    assert_eq!(
        kinds,
        [
            "user",
            "tools(2)",
            "assistant",
            "user",
            "tools(1)",
            "assistant",
            "end"
        ]
    );
    assert_eq!(t.title(), Some("Execute Command And Read File"));

    let ItemKind::Tools(first) = &t.items()[1].kind else {
        panic!("tools")
    };
    assert_eq!(first[0].kind, ToolKind::Shell);
    assert_eq!(first[0].input, "ls");
    assert!(first[0].finished);
    assert!(first[0]
        .output
        .as_deref()
        .is_some_and(|o| o.contains("hello.txt")));
    assert_eq!(first[1].kind, ToolKind::Read);

    let ItemKind::Tools(edit) = &t.items()[4].kind else {
        panic!("tools")
    };
    assert_eq!(edit[0].kind, ToolKind::Edit);
    assert!(edit[0].finished);
    let texts = |kind: DiffKind| -> Vec<String> {
        edit[0]
            .diff
            .iter()
            .filter(|d| d.kind == kind)
            .map(|d| d.text.clone())
            .collect()
    };
    assert_eq!(texts(DiffKind::Remove), ["hello from the scratch folder"]);
    assert_eq!(texts(DiffKind::Add), ["hello from Notchling"]);

    let ItemKind::Assistant { text } = &t.items()[5].kind else {
        panic!("assistant")
    };
    assert_eq!(text, "ok");

    let c = t.conversation();
    assert_eq!(c.turn_start, None);
    assert_eq!(c.facts.turns, 1);
    assert_eq!(c.facts.files.len(), 1);
    assert!(c.facts.context_tokens > 0);
}

#[test]
fn permission_request() {
    let (_, params) = replay();
    let p = params.expect("permission");
    let call = acp::tool_call(&p["toolCall"], None);
    assert_eq!(call.kind, ToolKind::Edit);
    assert_eq!(call.title, "File hello.txt");
    assert!(!call.diff.is_empty());
    let kinds: Vec<&str> = p["options"]
        .as_array()
        .expect("options")
        .iter()
        .filter_map(|o| o["kind"].as_str())
        .collect();
    assert_eq!(kinds, ["allow_once", "reject_once"]);
}

fn chunk(kind: &str, text: &str) -> Value {
    json!({"sessionUpdate": kind, "content": {"type": "text", "text": text}})
}

#[test]
fn chunks_join_until_another_kind_or_a_new_turn() {
    let mut t = Transcript::default();
    t.apply(&chunk("agent_message_chunk", "Hel"), None);
    t.apply(&chunk("agent_message_chunk", "lo"), None);
    t.apply(&chunk("agent_thought_chunk", "hmm"), None);
    t.apply(&chunk("agent_message_chunk", "Bye"), None);
    t.begin_turn(1.0);
    t.apply(&chunk("user_message_chunk", "one"), None);
    t.begin_turn(2.0);
    t.apply(&chunk("user_message_chunk", "two"), None);
    assert_eq!(t.items().len(), 5);
    let ItemKind::Assistant { text } = &t.items()[0].kind else {
        panic!("assistant")
    };
    assert_eq!(text, "Hello");
    assert!(t.conversation().turn_start.is_some());
    t.end_turn(Some("cancelled"), None, 3.0);
    let ItemKind::Notice { text, .. } = &t.items()[5].kind else {
        panic!("notice")
    };
    assert_eq!(text, "Stopped");
}

#[test]
fn diff_trims_shared_lines() {
    let d = acp::diff(Some("a\nb\nc\nd\ne\nf\ng\nh\n"), "a\nb\nc\nd\nX\nf\ng\nh\n");
    use DiffKind::*;
    assert_eq!(
        d.iter().map(|l| l.kind).collect::<Vec<_>>(),
        [Context, Context, Context, Remove, Add, Context, Context, Context]
    );
    assert_eq!(
        acp::diff(None, "new\n")
            .iter()
            .map(|l| l.kind)
            .collect::<Vec<_>>(),
        [Add]
    );
}

#[test]
fn failed_turn_adds_a_notice_and_clears_the_turn() {
    let mut t = Transcript::default();
    t.begin_turn(1.0);
    t.fail_turn("agy exited", 2.0);
    assert_eq!(t.conversation().turn_start, None);
    assert_eq!(
        t.items().last().map(|i| i.kind.clone()),
        Some(ItemKind::Notice {
            text: "agy exited".into(),
            symbol: "exclamationmark.triangle".into()
        })
    );
}

#[test]
fn mode_updates_are_not_messages() {
    let mut t = Transcript::default();
    t.apply(&chunk("agent_message_chunk", "[MODE_UPDATE] plan"), None);
    assert!(t.items().is_empty());
}
