//! Prompts the hook is holding open (`HookBridge.pendingRequests`), read-only: the hook writes
//! `requests/<id>.json` in Notchling's data folder for a question or a permission prompt. The
//! read-only notch shows them; it never writes a response, removes a file or signals a process, so
//! it reads by age instead of checking the hook's process: past the hook's wait, a file is stale.

use std::collections::HashMap;
use std::path::Path;

use notchling_engine::transcript::snapshot::{question, PendingQuestion};
use serde_json::{Map, Value};

/// The hook waits 290 s for a decision; a request older than this is gone or about to be.
pub const STALE_SECS: f64 = 300.0;

/// What the hook is asking.
#[derive(Clone, Debug, PartialEq)]
pub enum RequestKind {
    Question(PendingQuestion),
    Permission,
}

/// One held prompt.
#[derive(Clone, Debug, PartialEq)]
pub struct HookRequest {
    pub id: String,
    pub session_id: String,
    /// Seconds since the epoch.
    pub created: f64,
    pub kind: RequestKind,
    pub tool_name: String,
    /// The command, path, URL or pattern the tool would use; a question's first question.
    pub summary: String,
    /// Claude Code offered an "always allow" rule.
    pub can_always_allow: bool,
}

impl HookRequest {
    /// `HookRequest.init(_:)`: `None` without an id, session, pid and event name, or for a question
    /// with no questions in it.
    pub fn parse(obj: &Value) -> Option<HookRequest> {
        let id = obj.get("id")?.as_str()?;
        let session = obj.get("session_id")?.as_str()?;
        obj.get("pid")?.as_i64()?;
        let event = obj.get("hook_event_name")?.as_str()?;
        let tool_name = obj.get("tool_name").and_then(Value::as_str).unwrap_or("");
        let empty = Map::new();
        let input = obj
            .get("tool_input")
            .and_then(Value::as_object)
            .unwrap_or(&empty);
        let can_always_allow = obj
            .get("permission_suggestions")
            .and_then(Value::as_array)
            .is_some_and(|s| {
                s.iter()
                    .any(|r| r.get("type").and_then(Value::as_str) != Some("setMode"))
            });
        // In auto mode the question also comes as a permission prompt once PreToolUse passes.
        let (kind, summary) = if event == "PreToolUse" || tool_name == "AskUserQuestion" {
            let q = question(&format!("hook:{id}"), input)?;
            let first = q
                .items
                .first()
                .map(|i| i.question.clone())
                .unwrap_or_default();
            (RequestKind::Question(q), first)
        } else {
            (RequestKind::Permission, describe(input))
        };
        Some(HookRequest {
            id: id.to_owned(),
            session_id: session.to_owned(),
            created: obj.get("created").and_then(Value::as_f64).unwrap_or(0.0),
            kind,
            tool_name: tool_name.to_owned(),
            summary,
            can_always_allow,
        })
    }

    pub fn question(&self) -> Option<&PendingQuestion> {
        match &self.kind {
            RequestKind::Question(q) => Some(q),
            RequestKind::Permission => None,
        }
    }
}

/// A tool's input in one line or a few: the field that says what it does, else `key: value` lines.
pub fn describe(input: &Map<String, Value>) -> String {
    for key in [
        "command",
        "file_path",
        "notebook_path",
        "url",
        "pattern",
        "description",
    ] {
        if let Some(text) = input.get(key).and_then(Value::as_str) {
            return text.to_owned();
        }
    }
    let mut keys: Vec<&String> = input.keys().collect();
    keys.sort();
    keys.into_iter()
        .map(|key| match &input[key] {
            Value::String(text) => format!("{key}: {text}"),
            other => format!("{key}: {other}"),
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Every fresh request in `dir`, by session id (the newest wins when a session has several).
/// Hidden files (the hook's half-written `.<id>.json.tmp`) and anything unreadable are skipped.
pub fn read_dir(dir: &Path, now_secs: f64) -> HashMap<String, HookRequest> {
    let mut found: HashMap<String, HookRequest> = HashMap::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return found;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name();
        let hidden = name.to_string_lossy().starts_with('.');
        if hidden || path.extension().is_none_or(|e| e != "json") {
            continue;
        }
        let Some(request) = std::fs::read(&path)
            .ok()
            .and_then(|data| serde_json::from_slice::<Value>(&data).ok())
            .as_ref()
            .and_then(HookRequest::parse)
        else {
            continue;
        };
        if now_secs - request.created > STALE_SECS {
            continue;
        }
        let newer = found
            .get(&request.session_id)
            .is_none_or(|old| old.created <= request.created);
        if newer {
            found.insert(request.session_id.clone(), request);
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn permission(id: &str, created: f64) -> Value {
        json!({
            "id": id, "session_id": "s1", "pid": 42, "created": created,
            "hook_event_name": "PermissionRequest", "tool_name": "Bash",
            "tool_input": {"command": "rm -rf build", "description": "clean"},
            "permission_suggestions": [{"type": "addRules"}]
        })
    }

    #[test]
    fn a_permission_names_its_tool_and_command() {
        let r = HookRequest::parse(&permission("r1", 10.0)).unwrap();
        assert_eq!(r.kind, RequestKind::Permission);
        assert_eq!(
            (r.tool_name.as_str(), r.summary.as_str()),
            ("Bash", "rm -rf build")
        );
        assert!(r.can_always_allow);
        assert_eq!(r.question(), None);
    }

    #[test]
    fn set_mode_alone_is_no_always_rule() {
        let mut v = permission("r1", 10.0);
        v["permission_suggestions"] = json!([{"type": "setMode"}]);
        assert!(!HookRequest::parse(&v).unwrap().can_always_allow);
        v.as_object_mut().unwrap().remove("permission_suggestions");
        assert!(!HookRequest::parse(&v).unwrap().can_always_allow);
    }

    #[test]
    fn a_question_comes_from_pre_tool_use_or_ask_user_question() {
        let v = json!({
            "id": "q1", "session_id": "s2", "pid": 7, "hook_event_name": "PreToolUse",
            "tool_name": "AskUserQuestion",
            "tool_input": {"questions": [{"question": "Which db?", "header": "DB",
                "options": [{"label": "Postgres"}, {"label": "SQLite"}]}]}
        });
        let r = HookRequest::parse(&v).unwrap();
        let q = r.question().unwrap();
        assert_eq!(q.tool_use_id, "hook:q1");
        assert_eq!(q.items[0].options, vec!["Postgres", "SQLite"]);
        assert_eq!(r.summary, "Which db?");
    }

    #[test]
    fn missing_fields_are_refused() {
        for key in ["id", "session_id", "pid", "hook_event_name"] {
            let mut v = permission("r1", 1.0);
            v.as_object_mut().unwrap().remove(key);
            assert_eq!(HookRequest::parse(&v), None, "{key}");
        }
    }

    #[test]
    fn describe_falls_back_to_sorted_key_lines() {
        let input = json!({"b": 2, "a": "x", "c": {"d": true}});
        assert_eq!(
            describe(input.as_object().unwrap()),
            "a: x\nb: 2\nc: {\"d\":true}"
        );
        let path = json!({"file_path": "/w/a.rs", "content": "…"});
        assert_eq!(describe(path.as_object().unwrap()), "/w/a.rs");
    }

    #[test]
    fn reads_fresh_requests_newest_per_session_and_skips_the_rest() {
        let dir = std::env::temp_dir().join(format!("notchling-requests-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let write = |name: &str, v: &Value| std::fs::write(dir.join(name), v.to_string()).unwrap();
        write("old.json", &permission("old", 1_000.0));
        write("new.json", &permission("new", 1_100.0));
        write("stale.json", &{
            let mut v = permission("stale", 1.0);
            v["session_id"] = json!("s9");
            v
        });
        write(".half.json.tmp", &permission("half", 1_200.0));
        std::fs::write(dir.join("broken.json"), "{").unwrap();
        let found = read_dir(&dir, 1_200.0);
        std::fs::remove_dir_all(&dir).ok();
        assert_eq!(found.len(), 1);
        assert_eq!(found["s1"].id, "new");
        assert!(read_dir(Path::new("/nonexistent/notchling"), 0.0).is_empty());
    }
}
