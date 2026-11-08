//! A conversation built from Agent Client Protocol `session/update` payloads (`params.update`), as
//! Antigravity CLI's `agy acp` sends them: the whole history once on `session/load`, then live while a
//! turn runs. Text arrives in chunks, merged while the same kind keeps coming; tool calls arrive as a
//! `tool_call` and then `tool_call_update`s with the same id. Pure, so it is tested with a recording.

use super::model::*;
use super::util::{int_of, json_sorted, str_of};
use serde_json::{Map, Value};

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Transcript {
    items: Vec<ChatItem>,
    tasks: Vec<TaskEntry>,
    /// Antigravity's own title for the conversation (`session_info_update`).
    title: Option<String>,
    pub model: Option<String>,
    facts: SessionFacts,
    /// When the running turn began; None between turns.
    turn_start: Option<Timestamp>,
    /// The kind of the last text or tool update, so chunks of one message join up.
    last_kind: Option<String>,
    counter: u64,
    /// A `usage_update` came in this turn, so the context size is known and the turn's totals only add up.
    reported_context: bool,
}

impl Transcript {
    pub fn items(&self) -> &[ChatItem] {
        &self.items
    }

    pub fn tasks(&self) -> &[TaskEntry] {
        &self.tasks
    }

    pub fn title(&self) -> Option<&str> {
        self.title.as_deref()
    }

    pub fn facts(&self) -> &SessionFacts {
        &self.facts
    }

    pub fn turn_start(&self) -> Option<Timestamp> {
        self.turn_start
    }

    // MARK: Updates

    /// Applies one `session/update` payload (`params.update`).
    pub fn apply(&mut self, update: &Value, at: Option<Timestamp>) {
        let Some(kind) = str_of(update, "sessionUpdate") else {
            return;
        };
        match kind {
            "user_message_chunk" | "agent_message_chunk" | "agent_thought_chunk" => {
                let text = text(update.get("content"));
                if text.is_empty() {
                    return;
                }
                // Gemini reports an approval-mode change as a message of its own; the mode shows in the picker.
                if kind == "agent_message_chunk" && text.starts_with("[MODE_UPDATE]") {
                    return;
                }
                let joined = if self.last_kind.as_deref() == Some(kind) {
                    match self.items.last_mut().map(|i| &mut i.kind) {
                        Some(
                            ItemKind::User { text: t, .. }
                            | ItemKind::Assistant { text: t }
                            | ItemKind::Thinking { text: t, .. },
                        ) => {
                            t.push_str(&text);
                            true
                        }
                        _ => false,
                    }
                } else {
                    false
                };
                if !joined {
                    self.append(chunk_item(kind, text), at);
                }
                self.last_kind = Some(kind.to_string());
            }
            "tool_call" | "tool_call_update" => {
                let Some(id) = str_of(update, "toolCallId") else {
                    return;
                };
                if let Some((i, j)) = self.tool_index(id) {
                    if let ItemKind::Tools(calls) = &mut self.items[i].kind {
                        calls[j] = tool_call(update, Some(&calls[j]));
                    }
                } else if self.last_kind.as_deref() == Some("tool_call")
                    && matches!(self.items.last().map(|i| &i.kind), Some(ItemKind::Tools(_)))
                {
                    if let Some(ChatItem {
                        kind: ItemKind::Tools(calls),
                        ..
                    }) = self.items.last_mut()
                    {
                        calls.push(tool_call(update, None));
                    }
                } else {
                    self.append(ItemKind::Tools(vec![tool_call(update, None)]), at);
                }
                self.note_facts(update);
                self.last_kind = Some("tool_call".to_string());
            }
            "plan" => {
                let entries = update
                    .get("entries")
                    .and_then(Value::as_array)
                    .map(Vec::as_slice)
                    .unwrap_or(&[]);
                self.tasks = entries
                    .iter()
                    .enumerate()
                    .map(|(i, e)| TaskEntry {
                        id: format!("plan-{i}"),
                        subject: str_of(e, "content").unwrap_or("").to_string(),
                        status: match str_of(e, "status") {
                            Some("completed") => TaskStatus::Completed,
                            Some("in_progress") => TaskStatus::InProgress,
                            _ => TaskStatus::Pending,
                        },
                    })
                    .collect();
            }
            "session_info_update" => {
                if let Some(t) = str_of(update, "title").filter(|t| !t.is_empty()) {
                    self.title = Some(t.to_string());
                }
            }
            "usage_update" => {
                // Gemini: tokens in the context now (`used`) and the model's window (`size`).
                let used = int(update.get("used"));
                if used > 0 {
                    self.facts.context_tokens = used as u64;
                    self.reported_context = true;
                }
            }
            _ => {}
        }
    }

    /// You sent a prompt: the next user text starts a new message even if the last one was yours too.
    pub fn begin_turn(&mut self, at: Timestamp) {
        self.turn_start = Some(at);
        self.last_kind = None;
    }

    /// The prompt's answer arrived (`stopReason`, `usage`).
    pub fn end_turn(&mut self, stop_reason: Option<&str>, usage: Option<&Value>, at: Timestamp) {
        let seconds = self.turn_start.map_or(0.0, |s| at - s);
        self.turn_start = None;
        self.last_kind = None;
        let date = Some(at);
        match stop_reason {
            Some("cancelled") => self.append(ItemKind::notice("Stopped", "stop.circle"), date),
            Some("refusal") => self.append(
                ItemKind::notice("The model refused this request", "exclamationmark.triangle"),
                date,
            ),
            Some("max_tokens" | "max_turn_requests") => {
                self.append(
                    ItemKind::notice("The turn hit its limit", "exclamationmark.triangle"),
                    date,
                );
            }
            _ => {}
        }
        let model = self.model.clone();
        self.append(ItemKind::TurnEnd { seconds, model }, date);
        self.facts.turns += 1;
        if let Some(usage) = usage.filter(|u| u.is_object()) {
            // Gemini's input count already includes its cached tokens; Antigravity's doesn't.
            let cached = if self.reported_context {
                0
            } else {
                count(usage.get("cachedReadTokens"))
            };
            let input = count(usage.get("inputTokens")).saturating_add(cached);
            let output = count(usage.get("outputTokens"));
            self.facts.input_tokens = self.facts.input_tokens.saturating_add(input);
            self.facts.output_tokens = self.facts.output_tokens.saturating_add(output);
            // A slash command Gemini handled itself reports zeros; the context didn't change.
            let context = input.saturating_add(output);
            if !self.reported_context && context > 0 {
                self.facts.context_tokens = context;
            }
        }
        self.reported_context = false;
    }

    /// A turn that never answered (the process died, or the request failed).
    pub fn fail_turn(&mut self, message: &str, at: Timestamp) {
        self.turn_start = None;
        self.last_kind = None;
        self.append(
            ItemKind::notice(message, "exclamationmark.triangle"),
            Some(at),
        );
    }

    pub fn conversation(&self) -> Conversation {
        let mut c = Conversation {
            items: self.items.clone(),
            tasks: self.tasks.clone(),
            facts: self.facts.clone(),
            turn_start: self.turn_start,
            ..Conversation::default()
        };
        c.facts.model = self.model.clone();
        if self.turn_start.is_some() {
            if let Some(ChatItem {
                kind: ItemKind::Tools(calls),
                ..
            }) = self.items.last()
            {
                if let Some(open) = calls.iter().rev().find(|c| !c.finished) {
                    c.activity = Some(open.title.clone());
                }
            }
        }
        c
    }

    // MARK: Helpers

    fn append(&mut self, kind: ItemKind, at: Option<Timestamp>) {
        self.counter += 1;
        self.items.push(ChatItem {
            id: format!("agy-{}", self.counter),
            kind,
            date: at,
        });
    }

    fn tool_index(&self, id: &str) -> Option<(usize, usize)> {
        for (i, item) in self.items.iter().enumerate().rev() {
            if let ItemKind::Tools(calls) = &item.kind {
                if let Some(j) = calls.iter().position(|c| c.id == id) {
                    return Some((i, j));
                }
            }
        }
        None
    }

    fn note_facts(&mut self, update: &Value) {
        if str_of(update, "status") != Some("completed") {
            return;
        }
        match str_of(update, "kind") {
            Some("edit" | "delete" | "move") => {
                for path in paths(update) {
                    if !self.facts.files.contains(&path) {
                        self.facts.files.push(path);
                    }
                }
            }
            Some("execute") => self.facts.commands += 1,
            _ => {}
        }
    }
}

fn chunk_item(kind: &str, text: String) -> ItemKind {
    match kind {
        "user_message_chunk" => ItemKind::User { text, images: 0 },
        "agent_thought_chunk" => ItemKind::Thinking { seconds: 0.0, text },
        _ => ItemKind::Assistant { text },
    }
}

/// The text of a content block (`text`, or a resource link's name in brackets).
pub fn text(content: Option<&Value>) -> String {
    let Some(c) = content.filter(|c| c.is_object()) else {
        return String::new();
    };
    if let Some(t) = str_of(c, "text") {
        return t.to_string();
    }
    if str_of(c, "type") == Some("resource_link") {
        if let Some(name) = str_of(c, "name") {
            return format!("[{name}]");
        }
    }
    String::new()
}

fn int(v: Option<&Value>) -> i64 {
    v.and_then(int_of).unwrap_or(0)
}

/// A token count: a negative or missing number is zero.
fn count(v: Option<&Value>) -> u64 {
    int(v).max(0) as u64
}

fn paths(u: &Value) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let locations = u
        .get("locations")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    let content = u
        .get("content")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    let from_diffs = content
        .iter()
        .filter(|c| str_of(c, "type") == Some("diff"))
        .filter_map(|c| str_of(c, "path"));
    for path in locations
        .iter()
        .filter_map(|l| str_of(l, "path"))
        .chain(from_diffs)
    {
        if !out.iter().any(|p| p == path) {
            out.push(path.to_string());
        }
    }
    out
}

/// One ACP tool call as Chat's `ToolCall`; an update carries only what changed, so it merges into `old`.
pub fn tool_call(u: &Value, old: Option<&ToolCall>) -> ToolCall {
    let acp_kind = str_of(u, "kind");
    let content: &[Value] = u
        .get("content")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    let diffs: Vec<&Value> = content
        .iter()
        .filter(|c| str_of(c, "type") == Some("diff"))
        .collect();
    let kind = match acp_kind {
        Some("execute") => ToolKind::Shell,
        Some("read") => ToolKind::Read,
        Some("edit") => {
            let creates = !diffs.is_empty()
                && diffs
                    .iter()
                    .all(|d| str_of(d, "oldText").unwrap_or("").is_empty());
            if creates {
                ToolKind::Write
            } else {
                ToolKind::Edit
            }
        }
        Some("delete" | "move") => ToolKind::Write,
        Some("search") => ToolKind::Search,
        Some("fetch") => ToolKind::Web,
        _ => old.map_or(ToolKind::Other, |o| o.kind),
    };
    let mut call = match old {
        Some(o) => {
            let mut c = o.clone();
            if acp_kind.is_some() {
                c.kind = kind;
            }
            c
        }
        None => ToolCall::new(
            str_of(u, "toolCallId").unwrap_or("tool"),
            str_of(u, "name").or(acp_kind).unwrap_or("tool"),
            kind,
            String::new(),
            String::new(),
        ),
    };
    if let Some(title) = str_of(u, "title").filter(|t| !t.is_empty()) {
        call.title = title.to_string();
    }
    if let Some(raw) = u.get("rawInput").and_then(Value::as_object) {
        if let Some(input) = raw_input(raw) {
            call.input = input;
        }
    } else if call.input.is_empty() {
        if let Some(path) = paths(u).into_iter().next() {
            call.input = path;
        }
    }
    if call.title.is_empty() {
        call.title = if call.input.is_empty() {
            call.name.clone()
        } else {
            call.input.clone()
        };
    }
    let texts: Vec<String> = content
        .iter()
        .filter(|c| str_of(c, "type") == Some("content"))
        .map(|c| text(c.get("content")))
        .filter(|t| !t.is_empty())
        .collect();
    if !texts.is_empty() {
        call.output = Some(texts.join("\n"));
    } else if let Some(out) = u
        .get("rawOutput")
        .filter(|r| r.is_object())
        .and_then(|r| str_of(r, "output"))
        .filter(|o| !o.is_empty())
    {
        call.output = Some(out.to_string());
    }
    if !diffs.is_empty() {
        call.diff = diffs
            .iter()
            .flat_map(|d| diff(str_of(d, "oldText"), str_of(d, "newText").unwrap_or("")))
            .collect();
    }
    match str_of(u, "status") {
        Some("completed") => call.finished = true,
        Some("failed") => {
            call.finished = true;
            call.is_error = true;
        }
        Some("pending" | "in_progress") => call.finished = false,
        _ => {}
    }
    call
}

/// The command, path or query a tool call was given, from Antigravity's argument names.
fn raw_input(raw: &Map<String, Value>) -> Option<String> {
    for key in [
        "CommandLine",
        "command",
        "Command",
        "AbsolutePath",
        "Path",
        "path",
        "TargetFile",
        "Query",
        "query",
        "Url",
        "url",
        "SearchPath",
    ] {
        if let Some(v) = raw
            .get(key)
            .and_then(Value::as_str)
            .filter(|v| !v.is_empty())
        {
            return Some(v.to_string());
        }
    }
    let shown: Map<String, Value> = raw
        .iter()
        .filter(|(k, _)| *k != "toolSummary" && *k != "WaitMsBeforeAsync")
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    if shown.is_empty() {
        return None;
    }
    let json = json_sorted(&Value::Object(shown), true);
    Some(json.chars().take(400).collect())
}

/// Old and new text as diff lines: what both share at the start and end is trimmed to three context
/// lines, the middle shows as removed then added.
pub fn diff(old: Option<&str>, new: &str) -> Vec<DiffLine> {
    fn lines(s: &str) -> Vec<&str> {
        let mut l: Vec<&str> = s.split('\n').collect();
        while l.last().is_some_and(|x| x.is_empty()) {
            l.pop();
        }
        l
    }
    let a = lines(old.unwrap_or(""));
    let b = lines(new);
    let mut start = 0;
    while start < a.len() && start < b.len() && a[start] == b[start] {
        start += 1;
    }
    let (mut end_a, mut end_b) = (a.len(), b.len());
    while end_a > start && end_b > start && a[end_a - 1] == b[end_b - 1] {
        end_a -= 1;
        end_b -= 1;
    }
    let line = |kind: DiffKind, text: &str, i: usize| DiffLine {
        kind,
        text: text.to_string(),
        number: Some(i as i64 + 1),
    };
    let mut out = Vec::new();
    for (i, t) in a
        .iter()
        .enumerate()
        .take(start)
        .skip(start.saturating_sub(3))
    {
        out.push(line(DiffKind::Context, t, i));
    }
    for (i, t) in a.iter().enumerate().take(end_a).skip(start) {
        out.push(line(DiffKind::Remove, t, i));
    }
    for (i, t) in b.iter().enumerate().take(end_b).skip(start) {
        out.push(line(DiffKind::Add, t, i));
    }
    for (i, t) in a
        .iter()
        .enumerate()
        .take((end_a + 3).min(a.len()))
        .skip(end_a)
    {
        out.push(line(DiffKind::Context, t, i));
    }
    out
}
