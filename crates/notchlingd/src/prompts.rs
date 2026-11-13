use notchling_engine::reply::question::PendingQuestion;
use notchling_engine::tools::agy::config::clip;
use notchling_engine::transcript::snapshot;
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Mutex;

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

#[derive(Clone, Debug, PartialEq)]
pub struct Prompt {
    pub id: String,
    pub session_id: String,
    pub kind: Kind,
    pub event: Value,
    pub question: Option<PendingQuestion>,
    pub created_ms: i64,
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

#[derive(Default)]
pub struct PromptStore {
    inner: Mutex<Inner>,
}

impl PromptStore {
    fn inner(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

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

    pub fn resolve(&self, id: &str, decision: Value) -> Option<Prompt> {
        let held = self.inner().held.remove(id)?;
        let _ = held.tx.send(decision);
        Some(held.prompt)
    }

    pub fn forget(&self, id: &str) -> Option<Prompt> {
        self.inner().held.remove(id).map(|h| h.prompt)
    }

    pub fn get(&self, id: &str) -> Option<Prompt> {
        self.inner().held.get(id).map(|h| h.prompt.clone())
    }

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

    pub fn close(&self) -> Vec<Prompt> {
        let mut inner = self.inner();
        inner.closed = true;
        let held = std::mem::take(&mut inner.held);
        drop(inner);
        pass_all(held, "the daemon is stopping")
    }

    pub fn drain(&self) -> Vec<Prompt> {
        let held = std::mem::take(&mut self.inner().held);
        pass_all(held, "acting is off")
    }
}

fn pass_all(held: BTreeMap<String, Held>, reason: &str) -> Vec<Prompt> {
    let mut all: Vec<Prompt> = held
        .into_values()
        .map(|h| {
            let _ = h.tx.send(pass(reason));
            h.prompt
        })
        .collect();
    all.sort_by_key(|p| p.created_ms);
    all
}

const ROW_BYTES: usize = 128 * 1024;
const LIST_BYTES: usize = 768 * 1024;
const TEXT_CHARS: usize = 4000;
const DIFF_LINES: usize = 400;

pub fn fit_list(rows: Vec<Value>) -> Vec<Value> {
    let weight = |row: &Value| row.to_string().len();
    let mut used = 0;
    let mut out = Vec::with_capacity(rows.len());
    for mut row in rows {
        if weight(&row) > ROW_BYTES {
            trim_row(&mut row);
        }
        let size = weight(&row);
        if size > ROW_BYTES || used + size > LIST_BYTES {
            continue;
        }
        used += size;
        out.push(row);
    }
    out
}

fn trim_row(row: &mut Value) {
    let Some(map) = row.as_object_mut() else {
        return;
    };
    if let Some(input) = map.get_mut("input") {
        match input {
            Value::String(text) => *text = clip(text, TEXT_CHARS),
            other if other.to_string().len() > TEXT_CHARS => *other = json!({}),
            _ => {}
        }
    }
    for key in ["summary", "toolName"] {
        if let Some(Value::String(text)) = map.get_mut(key) {
            *text = clip(text, TEXT_CHARS);
        }
    }
    if let Some(Value::Array(diff)) = map.get_mut("diff") {
        diff.truncate(DIFF_LINES);
    }
    map.insert("trimmed".into(), Value::Bool(true));
}

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

    fn row(id: &str, input: Value) -> Value {
        json!({"id": id, "kind": "permission", "toolName": "Write", "summary": "s",
               "input": input, "question": null})
    }

    #[test]
    fn a_list_that_fits_is_left_alone() {
        let rows = vec![row("a", json!({"x": 1})), row("b", json!("two"))];
        assert_eq!(fit_list(rows.clone()), rows);
    }

    #[test]
    fn an_oversized_row_is_trimmed_and_the_list_stays_under_the_line() {
        let huge = json!({"file_path": "/a", "content": "x".repeat(900_000)});
        let mut diff = row("agy", json!("input"));
        diff["diff"] = json!((0..50_000)
            .map(|i| json!({"kind": "add", "text": format!("line {i}")}))
            .collect::<Vec<_>>());
        let rows = vec![row("hook", huge), diff, row("small", json!({}))];
        let fitted = fit_list(rows);
        let size = serde_json::to_string(&fitted).unwrap().len();
        assert!(size < 1 << 20, "{size} bytes");
        let ids: Vec<&str> = fitted.iter().filter_map(|r| r["id"].as_str()).collect();
        assert_eq!(
            ids,
            ["hook", "agy", "small"],
            "every prompt is still listed"
        );
        assert_eq!(fitted[0]["trimmed"], true);
        assert!(fitted[0]["input"].to_string().len() < 10_000);
        assert_eq!(fitted[1]["trimmed"], true);
        assert!(fitted[1]["diff"].as_array().unwrap().len() <= 400);
        assert!(fitted[2].get("trimmed").is_none());
    }

    #[test]
    fn a_row_that_cannot_be_trimmed_small_enough_is_skipped_not_fatal() {
        let mut stubborn = row("stubborn", json!({}));
        stubborn["question"] = json!({"items": [{"question": "q".repeat(400_000)}]});
        let fitted = fit_list(vec![stubborn, row("fine", json!({}))]);
        assert_eq!(fitted.len(), 1);
        assert_eq!(fitted[0]["id"], "fine");
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
