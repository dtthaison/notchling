//! The Artifact tool's hook events as pure functions over the library: record a publish
//! (`PostToolUse`), answer a whole-page read of a saved link from the local copy (`PreToolUse`),
//! and point a failed read at the local copy (`PostToolUseFailure`, or a `PostToolUse` that reads
//! like a 404). They return the JSON the hook prints, or `None` for "print nothing".
//!
//! Not wired into `notchling hook` until E13 (M6) installs the hook; the Swift app's `--hook`
//! still owns these events until then (decision 0011).

use super::content;
use super::library::PageLibrary;
use super::owner::Outcome;
use super::record::{flatten, RecordContext};
use crate::search::{artifact_id_in, PageEntry};
use serde_json::{json, Value};

/// True for a hook event this module handles: the Artifact tool's `PreToolUse`, `PostToolUse` and
/// `PostToolUseFailure`.
pub fn handles(event: &Value) -> bool {
    event.get("tool_name").and_then(Value::as_str) == Some("Artifact")
        && matches!(
            event.get("hook_event_name").and_then(Value::as_str),
            Some("PreToolUse" | "PostToolUse" | "PostToolUseFailure")
        )
}

/// The saved page an `Artifact read` of a whole page asks for. Reads of one file (`path`/`paths`)
/// aren't matched, so `path: "index.html"` still fetches the live page.
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

/// The deny reason for a `PreToolUse(Artifact)` read we can answer locally: where the copy is.
/// `None` (the real read runs) when we don't keep the page or its file is gone.
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

/// Context for a failed `Artifact read`: the link's page is usually another account's (artifacts
/// are private to their login), and the local copy still reads.
pub fn read_fallback(lib: &PageLibrary, event: &Value) -> Option<String> {
    let page = saved_page(lib, event)?;
    Some(format!(
        "Notchling keeps a local copy of this artifact, as last published from this Mac. Read the local file instead:\n{}\n\
Only the account that published it can republish to this link; from another account, publishing makes a new link.",
        lib.describe(&page)
    ))
}

/// A `PreToolUse` answer that denies the call with a reason Claude reads.
pub fn deny_output(reason: &str) -> Value {
    json!({"hookSpecificOutput": {
        "hookEventName": "PreToolUse",
        "permissionDecision": "deny",
        "permissionDecisionReason": reason,
    }})
}

/// An answer that adds context for Claude and decides nothing.
pub fn context_output(event_name: &str, context: &str) -> Value {
    json!({"hookSpecificOutput": {
        "hookEventName": event_name,
        "additionalContext": context,
    }})
}

/// What `PreToolUse(Artifact)` prints. `owner` is what a publish to another saved account's page
/// came to ([`super::owner::run`]; `None` when it doesn't apply): done or refused stop the call,
/// failed lets the real publish run with the reason as context. With no owner outcome, a read of
/// a saved page is answered from the local copy.
pub fn pre_tool_use(lib: &PageLibrary, event: &Value, owner: Option<Outcome>) -> Option<Value> {
    match owner {
        Some(Outcome::Done(reason) | Outcome::Refused(reason)) => Some(deny_output(&reason)),
        Some(Outcome::Failed(context)) => Some(context_output("PreToolUse", &context)),
        None => local_read(lib, event).map(|reason| deny_output(&reason)),
    }
}

/// What `PostToolUse(Artifact)` and `PostToolUseFailure(Artifact)` print. A `PostToolUse` first
/// records the publish (a read-only library, or any failure, is silent: the hook never answers
/// Claude Code with an error). A failed read, or one whose short result reads like a 404, is
/// pointed at the local copy.
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
