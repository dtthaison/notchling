use super::content;
use super::library::PageLibrary;
use super::owner::Outcome;
use super::record::{flatten, RecordContext};
use crate::search::{artifact_id_in, PageEntry};
use serde_json::{json, Value};

pub fn handles(event: &Value) -> bool {
    event.get("tool_name").and_then(Value::as_str) == Some("Artifact")
        && matches!(
            event.get("hook_event_name").and_then(Value::as_str),
            Some("PreToolUse" | "PostToolUse" | "PostToolUseFailure")
        )
}

pub fn may_publish(event: &Value) -> bool {
    event.get("hook_event_name").and_then(Value::as_str) == Some("PostToolUse")
        && matches!(
            event
                .get("tool_input")
                .and_then(|i| i.get("action"))
                .and_then(Value::as_str),
            None | Some("publish")
        )
}

pub fn answer(lib: &PageLibrary, event: &Value, ctx: &RecordContext<'_>) -> Option<Value> {
    if !handles(event) {
        return None;
    }
    match event.get("hook_event_name").and_then(Value::as_str)? {
        "PreToolUse" => pre_tool_use(lib, event, None),
        _ => post_tool_use(lib, event, ctx),
    }
}

pub fn saved_page(lib: &PageLibrary, event: &Value) -> Option<PageEntry> {
    let input = event.get("tool_input")?.as_object()?;
    if input.get("action").and_then(Value::as_str) != Some("read")
        || input.contains_key("path")
        || input.contains_key("paths")
    {
        return None;
    }
    let id = artifact_id_in(input.get("url")?.as_str()?)?;
    lib.entries()
        .into_iter()
        .find(|e| e.artifact_id() == Some(id.as_str()))
}

pub fn local_read(lib: &PageLibrary, event: &Value) -> Option<String> {
    let page = saved_page(lib, event)?;
    if !lib.local_path(&page)?.is_file() {
        return None;
    }
    Some(format!(
        "Notchling answered this read from its local copy of the artifact, as last published from this Mac. \
Read the \"Local copy\" file with the Read tool; this is not an error.\n{}\n\
It doesn't hold edits made on claude.ai after that publish. To get those, read the live page with \
action \"read\", this url and path \"index.html\" (only the account that published it can).",
        lib.describe(&page)
    ))
}

pub fn read_fallback(lib: &PageLibrary, event: &Value) -> Option<String> {
    let page = saved_page(lib, event)?;
    Some(format!(
        "Notchling keeps a local copy of this artifact, as last published from this Mac. Read the local file instead:\n{}\n\
Only the account that published it can republish to this link; from another account, publishing makes a new link.",
        lib.describe(&page)
    ))
}

pub fn deny_output(reason: &str) -> Value {
    json!({"hookSpecificOutput": {
        "hookEventName": "PreToolUse",
        "permissionDecision": "deny",
        "permissionDecisionReason": reason,
    }})
}

pub fn context_output(event_name: &str, context: &str) -> Value {
    json!({"hookSpecificOutput": {
        "hookEventName": event_name,
        "additionalContext": context,
    }})
}

pub fn pre_tool_use(lib: &PageLibrary, event: &Value, owner: Option<Outcome>) -> Option<Value> {
    match owner {
        Some(Outcome::Done(reason) | Outcome::Refused(reason)) => Some(deny_output(&reason)),
        Some(Outcome::Failed(context)) => Some(context_output("PreToolUse", &context)),
        None => local_read(lib, event).map(|reason| deny_output(&reason)),
    }
}

pub fn post_tool_use(lib: &PageLibrary, event: &Value, ctx: &RecordContext<'_>) -> Option<Value> {
    let name = event.get("hook_event_name").and_then(Value::as_str)?;
    if name == "PostToolUse" {
        let _ = lib.record(event, ctx);
    }
    let failed = name == "PostToolUseFailure"
        || (name == "PostToolUse" && content::looks_missing(&flatten(event.get("tool_response"))));
    if !failed {
        return None;
    }
    read_fallback(lib, event).map(|context| context_output(name, &context))
}
