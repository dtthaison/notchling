//! The questions and permissions hooks hold open, waiting on the app (the Swift `HookBridge`'s
//! `requests/` folder, in memory). A hook's `hook.request` opens one and waits on its channel;
//! `session.permission` or `session.answer` resolves it, and the hook prints the decision.
//!
//! A decision is the Swift app's response shape: `{"action": "allow", "always"?: bool}`,
//! `{"action": "deny", "message"?: text}`, `{"action": "answer", "answers": {question: answer}}`,
//! or `{"action": "pass", "reason": text}`, which prints nothing.

use notchling_engine::reply::question::PendingQuestion;
use notchling_engine::transcript::snapshot;
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Mutex;

/// What a prompt asks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Question,
    Permission,
}

impl Kind {
    pub fn name(self) -> &'static str {
        match self {
            Kind::Question => "question",
            Kind::Permission => "permission",
        }
    }

    /// A PreToolUse hook is the AskUserQuestion one; in auto mode the question also arrives as a
    /// permission request for AskUserQuestion once that passes.
    pub fn of(event: &Value) -> Kind {
        let name = event.get("hook_event_name").and_then(Value::as_str);
        let tool = event.get("tool_name").and_then(Value::as_str);
        if name == Some("PreToolUse") || tool == Some("AskUserQuestion") {
            Kind::Question
        } else {
            Kind::Permission
        }
    }
}

/// One held prompt.
#[derive(Clone, Debug, PartialEq)]
pub struct Prompt {
    pub id: String,
    /// Claude Code's session id (the transcript's name), from the hook event.
    pub session_id: String,
    pub kind: Kind,
    /// The hook's whole event, as Claude Code sent it.
    pub event: Value,
    /// A question's items, read from its tool input.
    pub question: Option<PendingQuestion>,
    pub created_ms: i64,
    /// The hook process, so a hook Claude Code gave up on is noticed.
    pub hook_pid: Option<u32>,
}

impl Prompt {
    fn new(id: String, event: Value, hook_pid: Option<u32>, created_ms: i64) -> Prompt {
        let kind = Kind::of(&event);
        let input = event
            .get("tool_input")
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default();
        let question = match kind {
            Kind::Question => snapshot::question(&format!("hook:{id}"), &input),
            Kind::Permission => None,
        };
        Prompt {
            session_id: str_at(&event, "session_id").to_string(),
            id,
            kind,
            question,
            event,
            created_ms,
            hook_pid,
        }
    }

    /// `prompt.pending`'s data, and a `prompts.list` row.
    pub fn to_json(&self) -> Value {
        let input = self.event.get("tool_input").cloned().unwrap_or(json!({}));
        json!({
            "id": self.id,
            "sessionId": self.session_id,
            "kind": self.kind.name(),
            "toolName": str_at(&self.event, "tool_name"),
            "summary": self.summary(),
            "input": input,
            "canAlwaysAllow": !crate::hook_output::always_rules(&self.event).is_empty(),
            "question": self.question.as_ref().map(question_json),
            "cwd": str_at(&self.event, "cwd"),
            "transcriptPath": str_at(&self.event, "transcript_path"),
            "createdMs": self.created_ms,
        })
    }

    /// One line about it (`HookRequest.summary`): the first question, or what the tool touches.
    pub fn summary(&self) -> String {
        if let Some(q) = &self.question {
            return q
                .items
                .first()
                .map(|i| i.question.clone())
                .unwrap_or_default();
        }
        describe(self.event.get("tool_input").and_then(Value::as_object))
    }
}

fn str_at<'a>(v: &'a Value, key: &str) -> &'a str {
    v.get(key).and_then(Value::as_str).unwrap_or("")
}

/// A question as JSON: `{"items": [{header, question, options, details, multiSelect}]}`, the same
/// shape `session.answer` takes back.
pub fn question_json(q: &PendingQuestion) -> Value {
    let items: Vec<Value> = q
        .items
        .iter()
        .map(|i| {
            json!({"header": i.header, "question": i.question, "options": i.options,
                   "details": i.details, "multiSelect": i.multi_select})
        })
        .collect();
    json!({ "items": items })
}

/// `HookRequest.describe`: the command, path, URL, pattern or description, else every field as
/// `key: value` lines (nested values as JSON with sorted keys), at most 300 characters.
fn describe(input: Option<&Map<String, Value>>) -> String {
    let Some(input) = input else {
        return String::new();
    };
    for key in [
        "command",
        "file_path",
        "notebook_path",
        "url",
        "pattern",
        "description",
    ] {
        if let Some(text) = input.get(key).and_then(Value::as_str) {
            return text.to_string();
        }
    }
    let mut keys: Vec<&String> = input.keys().collect();
    keys.sort();
    let lines: Vec<String> = keys
        .into_iter()
        .map(|key| match &input[key] {
            Value::String(text) => format!("{key}: {text}"),
            other => format!("{key}: {}", sorted(other)),
        })
        .collect();
    lines.join("\n").chars().take(300).collect()
}

/// JSON text with object keys sorted, whatever order the map keeps.
fn sorted(value: &Value) -> String {
    match value {
        Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            let parts: Vec<String> = keys
                .into_iter()
                .map(|k| format!("{}:{}", Value::String(k.clone()), sorted(&map[k])))
                .collect();
            format!("{{{}}}", parts.join(","))
        }
        Value::Array(list) => {
            let parts: Vec<String> = list.iter().map(sorted).collect();
            format!("[{}]", parts.join(","))
        }
        other => other.to_string(),
    }
}

struct Held {
    prompt: Prompt,
    tx: Sender<Value>,
}

#[derive(Default)]
struct Inner {
    next: u64,
    held: BTreeMap<String, Held>,
    closed: bool,
}

/// The held prompts, oldest first.
#[derive(Default)]
pub struct PromptStore {
    inner: Mutex<Inner>,
}

impl PromptStore {
    fn inner(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Holds a prompt for `event`; its decision arrives on the receiver. `None` once closed.
    pub fn open(
        &self,
        event: Value,
        hook_pid: Option<u32>,
        now_ms: i64,
    ) -> Option<(Prompt, Receiver<Value>)> {
        let mut inner = self.inner();
        if inner.closed {
            return None;
        }
        inner.next += 1;
        let id = format!("p{}-{:x}", inner.next, now_ms);
        let prompt = Prompt::new(id.clone(), event, hook_pid, now_ms);
        let (tx, rx) = channel();
        inner.held.insert(
            id,
            Held {
                prompt: prompt.clone(),
                tx,
            },
        );
        Some((prompt, rx))
    }

    /// Hands `decision` to the hook waiting on `id`; the prompt, or `None` when it's no longer held.
    pub fn resolve(&self, id: &str, decision: Value) -> Option<Prompt> {
        let held = self.inner().held.remove(id)?;
        // The hook may have given up a moment ago; the prompt is gone either way.
        let _ = held.tx.send(decision);
        Some(held.prompt)
    }

    /// The waiting hook gives up on `id`; `None` when it was resolved meanwhile.
    pub fn forget(&self, id: &str) -> Option<Prompt> {
        self.inner().held.remove(id).map(|h| h.prompt)
    }

    pub fn get(&self, id: &str) -> Option<Prompt> {
        self.inner().held.get(id).map(|h| h.prompt.clone())
    }

    /// The oldest prompt of `kind` held for Claude Code session `session_id`.
    pub fn for_session(&self, session_id: &str, kind: Kind) -> Option<Prompt> {
        let inner = self.inner();
        let mut found: Vec<&Prompt> = inner
            .held
            .values()
            .map(|h| &h.prompt)
            .filter(|p| !session_id.is_empty() && p.session_id == session_id && p.kind == kind)
            .collect();
        found.sort_by_key(|p| p.created_ms);
        found.first().map(|p| (*p).clone())
    }

    pub fn list(&self) -> Vec<Prompt> {
        let mut all: Vec<Prompt> = self
            .inner()
            .held
            .values()
            .map(|h| h.prompt.clone())
            .collect();
        all.sort_by_key(|p| p.created_ms);
        all
    }

    /// The daemon is stopping: every held prompt passes, and no new one is held.
    pub fn close(&self) -> Vec<Prompt> {
        let mut inner = self.inner();
        inner.closed = true;
        let held = std::mem::take(&mut inner.held);
        held.into_values()
            .map(|h| {
                let _ = h.tx.send(pass("the daemon is stopping"));
                h.prompt
            })
            .collect()
    }
}

/// A pass decision with its reason.
pub fn pass(reason: &str) -> Value {
    json!({"action": "pass", "reason": reason})
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> Value {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/hook")
            .join(name);
        serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap()
    }

    #[test]
    fn a_held_prompt_reads_its_kind_question_and_summary() {
        let store = PromptStore::default();
        let (q, _rx) = store
            .open(fixture("ask-user-question.json"), Some(7), 10)
            .unwrap();
        assert_eq!(q.kind, Kind::Question);
        assert_eq!(q.session_id, "6f1c2a9e-3b7d-4e21-9a0c-5d8e7f6a1b2c");
        let items = &q.question.as_ref().unwrap().items;
        assert_eq!(items.len(), 2);
        assert!(items[0].multi_select);
        assert_eq!(items[1].options, vec!["Home", "Away"]);
        assert_eq!(q.summary(), "Which pets?");
        let json = q.to_json();
        assert_eq!(json["kind"], "question");
        assert_eq!(json["question"]["items"][0]["multiSelect"], true);
        assert_eq!(json["canAlwaysAllow"], false);

        let (p, _rx) = store
            .open(fixture("permission-request.json"), None, 11)
            .unwrap();
        assert_eq!(p.kind, Kind::Permission);
        assert_eq!(p.summary(), "rm -rf build");
        assert_eq!(p.to_json()["canAlwaysAllow"], true);
        assert_eq!(p.to_json()["question"], Value::Null);

        let (auto, _rx) = store
            .open(fixture("ask-as-permission.json"), None, 12)
            .unwrap();
        assert_eq!(auto.kind, Kind::Question, "auto mode's question permission");
        assert_eq!(store.list().len(), 3);
    }

    #[test]
    fn other_inputs_are_described_as_sorted_lines() {
        let input = json!({"b": {"z": 1, "a": "x/y"}, "a": "text"});
        assert_eq!(
            describe(input.as_object()),
            "a: text\nb: {\"a\":\"x/y\",\"z\":1}"
        );
        let long = json!({"k": "x".repeat(400)});
        assert_eq!(describe(long.as_object()).chars().count(), 300);
        assert_eq!(
            describe(json!({"url": "https://a", "command": "ls"}).as_object()),
            "ls"
        );
    }

    #[test]
    fn a_decision_reaches_the_waiting_hook_once() {
        let store = PromptStore::default();
        let (p, rx) = store
            .open(fixture("permission-request.json"), None, 1)
            .unwrap();
        assert_eq!(
            store
                .for_session(&p.session_id, Kind::Permission)
                .map(|f| f.id),
            Some(p.id.clone())
        );
        assert!(store.for_session(&p.session_id, Kind::Question).is_none());
        assert!(store.for_session("", Kind::Permission).is_none());
        assert!(store.resolve(&p.id, json!({"action": "allow"})).is_some());
        assert_eq!(rx.recv().unwrap(), json!({"action": "allow"}));
        assert!(store.resolve(&p.id, json!({"action": "deny"})).is_none());
        assert!(store.forget(&p.id).is_none());
        assert!(store.list().is_empty());
    }

    #[test]
    fn closing_passes_every_held_prompt_and_holds_no_more() {
        let store = PromptStore::default();
        let (_p, rx) = store
            .open(fixture("permission-request.json"), None, 1)
            .unwrap();
        assert_eq!(store.close().len(), 1);
        assert_eq!(rx.recv().unwrap()["action"], "pass");
        assert!(store
            .open(fixture("permission-request.json"), None, 2)
            .is_none());
    }
}
