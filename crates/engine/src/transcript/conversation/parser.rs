use super::queue::Queue;
use super::repl::{self, Repl};
use super::tools::{apply_task_result, clip, diff_lines, make_call, result_text, written_lines};
use super::user::{model_id_in_confirmation, user_item};
use crate::transcript::browser;
use crate::transcript::login;
use crate::transcript::scan::HEAD_BYTES;
use crate::transcript::shells;
use crate::transcript::snapshot::{self, PendingQuestion};
use crate::transcript::subagents::{notification_statuses, result_end, resume_end};
use crate::transcript::tail::{LineParser, Spot};
use crate::transcript::util::{count_of, date_field, parse_object, str_of};
use crate::transcript::{glance::has, model::*};
use serde_json::{Map, Value};
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};

pub(crate) const MAX_LINE: usize = 400_000;
const TOO_LARGE: &str = "(too large to show)";
const KEEP_ITEMS: usize = 500;
const TRIM_AT: usize = 1000;

#[derive(Default)]
pub struct Parser {
    c: Conversation,
    call_index: HashMap<String, (usize, usize)>,
    stops: shells::Stops,
    counted: HashSet<String>,
    counted_order: Vec<String>,
    turn_output: HashMap<String, u64>,
    files: Vec<String>,
    serial: u64,
    thinking_at: Option<usize>,
    thinking_seconds: BTreeMap<String, f64>,
    queue: Queue,
    pub include_sidechain: bool,
    last_question: Option<PendingQuestion>,
    last_question_answered: bool,
    image_calls: VecDeque<String>,
    repl: Repl,
}

impl LineParser for Parser {
    fn consume(&mut self, line: &[u8]) {
        self.parse_line(line);
        self.trim();
    }

    fn consume_oversized(&mut self, head: &[u8], _len: u64, spot: Option<Spot<'_>>) {
        self.oversized_result(head, spot);
    }
}

impl Parser {
    pub fn new() -> Parser {
        Parser::default()
    }

    pub fn for_subagent() -> Parser {
        Parser {
            include_sidechain: true,
            ..Parser::default()
        }
    }

    pub fn consume(&mut self, line: &[u8]) {
        LineParser::consume(self, line);
    }

    pub fn consume_oversized(&mut self, head: &[u8], len: u64, spot: Option<Spot<'_>>) {
        LineParser::consume_oversized(self, head, len, spot);
    }

    fn oversized_result(&mut self, head: &[u8], spot: Option<Spot<'_>>) {
        self.oversized_output(head, spot);
        self.oversized_event(head);
        if self.oversized_ends_batch(head) {
            self.queue.end_batch();
        }
    }

    fn oversized_event(&mut self, head: &[u8]) {
        let (events, polled) = repl::oversized_events(head);
        let evals: Vec<u64> = events.iter().map(|e| e.eval).collect();
        for mut event in events {
            if !event.closed {
                if !event.text.is_empty() {
                    event.text.push('\n');
                }
                event.text.push_str(TOO_LARGE);
            }
            self.settle(event);
        }
        if polled {
            self.repl.polled(&evals);
        }
    }

    fn oversized_ends_batch(&self, head: &[u8]) -> bool {
        if !self.include_sidechain && has(head, br#""isSidechain":true"#) {
            return false;
        }
        let Some(message) = find(head, br#""message":"#) else {
            return false;
        };
        let top = &head[..message];
        has(top, br#""type":"user""#) || has(top, br#""type":"assistant""#)
    }

    fn oversized_output(&mut self, head: &[u8], spot: Option<Spot<'_>>) {
        let Some(id) = oversized_tool_result(head) else {
            return;
        };
        let Some(&(item, index)) = self.call_index.get(id.0) else {
            return;
        };
        let Some(ChatItem {
            kind: ItemKind::Tools(calls),
            ..
        }) = self.c.items.get_mut(item)
        else {
            return;
        };
        let Some(call) = calls.get_mut(index) else {
            return;
        };
        call.finished = true;
        let mut queued = None;
        if call.kind == ToolKind::Browser {
            if id.1 {
                let state = call.browser_mut();
                state.shots = state.shots.max(1);
                if let Some(spot) = spot {
                    state.spots = vec![browser::spot_for(spot.path, spot.at)];
                    queued = Some(call.id.clone());
                }
            }
            call.output = None;
        } else {
            call.output = Some(if id.1 { "[Image]" } else { TOO_LARGE }.to_string());
        }
        if let Some(queued) = queued {
            self.image_calls.push_back(queued);
        }
        self.drop_old_images();
    }

    fn next_id(&mut self, prefix: &str) -> String {
        self.serial += 1;
        format!("{prefix}{}", self.serial)
    }

    fn append(&mut self, kind: ItemKind, date: Option<Timestamp>) {
        if matches!(
            kind,
            ItemKind::User { .. } | ItemKind::TurnEnd { .. } | ItemKind::Notice { .. }
        ) {
            self.thinking_at = None;
            self.thinking_seconds.clear();
        }
        let id = self.next_id("i");
        self.c.items.push(ChatItem { id, kind, date });
    }

    fn append_user(&mut self, kind: ItemKind, date: Option<Timestamp>, starts_turn: bool) {
        let is_user = matches!(kind, ItemKind::User { .. });
        self.append(kind, date);
        if starts_turn && is_user {
            self.c.turn_start = date;
            self.turn_output.clear();
        }
    }

    fn parse_line(&mut self, line: &[u8]) {
        if line.len() > MAX_LINE {
            self.oversized_result(&line[..line.len().min(HEAD_BYTES)], None);
            return;
        }
        let Some(map) = parse_object(line) else {
            return;
        };
        let obj = Value::Object(map);
        let Some(kind) = str_of(&obj, "type") else {
            return;
        };
        if !self.include_sidechain && obj.get("isSidechain").and_then(Value::as_bool) == Some(true)
        {
            return;
        }
        if !self.include_sidechain {
            if let Some(step) = login::step(&obj) {
                self.c.login_needed = step == login::Step::Needed;
            }
        }
        if has(line, b"<task-notification>") {
            if let Ok(text) = std::str::from_utf8(line) {
                for (id, status) in notification_statuses(&text.replace("\\/", "/")) {
                    self.c.finished_agents.insert(id, status);
                }
            }
        }
        let date = date_field(&obj);
        if let Some(branch) = str_of(&obj, "gitBranch") {
            if !branch.is_empty() && branch != "HEAD" {
                self.c.facts.branch = Some(branch.to_string());
            }
        }
        match kind {
            "system" => self.system(&obj, date),
            "assistant" => {
                self.queue.end_batch();
                self.assistant(&obj, date);
            }
            "user" => {
                self.user(&obj, date);
                self.queue.end_batch();
            }
            "queue-operation" => self.queue_operation(&obj),
            "attachment" => self.attachment(&obj, date),
            _ => {}
        }
    }

    fn system(&mut self, obj: &Value, date: Option<Timestamp>) {
        match str_of(obj, "subtype") {
            Some("turn_duration") => {
                let ms = obj.get("durationMs").and_then(Value::as_f64).unwrap_or(0.0);
                let model = self.c.facts.model.clone();
                self.append(
                    ItemKind::TurnEnd {
                        seconds: ms / 1000.0,
                        model,
                    },
                    date,
                );
                self.c.facts.turns += 1;
            }
            Some("compact_boundary") => {
                self.append(compacted(), date);
                if let Some(post) = obj.get("compactMetadata").and_then(|m| m.get("postTokens")) {
                    self.c.facts.context_tokens = count_of(post);
                }
            }
            Some("local_command") => {
                if let Some(text) = str_of(obj, "content") {
                    if let Some(id) = model_id_in_confirmation(text) {
                        self.c.facts.model = Some(id);
                    }
                    if let Some(kind) = user_item(text, 0) {
                        self.append(kind, date);
                    }
                }
            }
            _ => {}
        }
    }

    fn assistant(&mut self, obj: &Value, date: Option<Timestamp>) {
        let Some(message) = obj.get("message") else {
            return;
        };
        let Some(blocks) = message.get("content").and_then(Value::as_array) else {
            return;
        };
        if let Some(model) = str_of(message, "model") {
            if model != "<synthetic>" {
                self.c.facts.model = Some(model.to_string());
            }
        }
        if let Some(effort) = str_of(obj, "effort") {
            self.c.facts.effort = Some(effort.to_string());
        }
        let message_id = str_of(message, "id");
        let usage = message.get("usage").filter(|u| u.is_object());
        let n = |usage: &Value, k: &str| usage.get(k).map_or(0, count_of);
        if let (Some(id), Some(usage)) = (message_id, usage) {
            if let Some(out) = usage.get("output_tokens") {
                let entry = self.turn_output.entry(id.to_string()).or_insert(0);
                *entry = (*entry).max(count_of(out));
            }
            if !self.counted.contains(id) {
                self.counted.insert(id.to_string());
                self.counted_order.push(id.to_string());
                let input = n(usage, "input_tokens")
                    .saturating_add(n(usage, "cache_creation_input_tokens"))
                    .saturating_add(n(usage, "cache_read_input_tokens"));
                self.c.facts.input_tokens = self.c.facts.input_tokens.saturating_add(input);
                self.c.facts.cached_tokens = self
                    .c
                    .facts
                    .cached_tokens
                    .saturating_add(n(usage, "cache_read_input_tokens"));
                self.c.facts.output_tokens = self
                    .c
                    .facts
                    .output_tokens
                    .saturating_add(n(usage, "output_tokens"));
            }
        }
        if let Some(usage) = usage {
            let held = n(usage, "input_tokens")
                .saturating_add(n(usage, "cache_creation_input_tokens"))
                .saturating_add(n(usage, "cache_read_input_tokens"))
                .saturating_add(n(usage, "output_tokens"));
            if held > 0 {
                self.c.facts.context_tokens = held;
            }
        }
        for block in blocks {
            match str_of(block, "type") {
                Some("text") => self.assistant_text(block, date),
                Some("thinking" | "redacted_thinking") => self.thinking(obj, message, block, date),
                Some("tool_use") => self.tool_use(block, date),
                _ => {}
            }
        }
    }

    fn assistant_text(&mut self, block: &Value, date: Option<Timestamp>) {
        let text = str_of(block, "text").unwrap_or("").trim().to_string();
        if text.is_empty() {
            return;
        }
        if let Some(ChatItem {
            kind: ItemKind::Assistant { text: prior },
            ..
        }) = self.c.items.last_mut()
        {
            prior.push_str("\n\n");
            prior.push_str(&text);
        } else {
            self.append(ItemKind::Assistant { text }, date);
        }
    }

    fn thinking(&mut self, obj: &Value, message: &Value, block: &Value, date: Option<Timestamp>) {
        let seconds = obj
            .get("thinkingDurationMs")
            .and_then(Value::as_f64)
            .unwrap_or(0.0)
            / 1000.0;
        let text = str_of(block, "thinking").unwrap_or("").trim().to_string();
        let key = match str_of(message, "id") {
            Some(id) => id.to_string(),
            None => self.next_id("m"),
        };
        let slot = self.thinking_seconds.entry(key).or_insert(0.0);
        *slot = slot.max(seconds);
        let total: f64 = self.thinking_seconds.values().sum();
        if let Some(at) = self.thinking_at {
            if let Some(ChatItem {
                kind:
                    ItemKind::Thinking {
                        seconds,
                        text: prior,
                    },
                ..
            }) = self.c.items.get_mut(at)
            {
                *seconds = total;
                if !text.is_empty() {
                    if prior.is_empty() {
                        *prior = text;
                    } else {
                        prior.push_str("\n\n");
                        prior.push_str(&text);
                    }
                }
                return;
            }
        }
        self.append(
            ItemKind::Thinking {
                seconds: total,
                text,
            },
            date,
        );
        self.thinking_at = Some(self.c.items.len() - 1);
    }

    fn tool_use(&mut self, block: &Value, date: Option<Timestamp>) {
        let Some(id) = str_of(block, "id") else {
            return;
        };
        let name = str_of(block, "name").unwrap_or("Tool");
        let empty = Map::new();
        let input = block
            .get("input")
            .and_then(Value::as_object)
            .unwrap_or(&empty);
        if name == "AskUserQuestion" {
            if let Some(q) = snapshot::question(id, input) {
                self.last_question = Some(q);
                self.last_question_answered = false;
            }
        }
        self.stops.call(id, name, input);
        let call = make_call(id, name, input);
        if call.kind == ToolKind::Shell {
            self.c.facts.commands += 1;
        }
        if matches!(call.kind, ToolKind::Edit | ToolKind::Write) {
            let path = input
                .get("file_path")
                .and_then(Value::as_str)
                .or_else(|| input.get("notebook_path").and_then(Value::as_str));
            if let Some(path) = path {
                if !self.files.iter().any(|f| f == path) {
                    self.files.push(path.to_string());
                }
            }
        }
        if name == "TodoWrite" {
            if let Some(todos) = input.get("todos").and_then(Value::as_array) {
                self.c.tasks = todos
                    .iter()
                    .enumerate()
                    .map(|(i, t)| TaskEntry {
                        id: i.to_string(),
                        subject: str_of(t, "content").unwrap_or("").to_string(),
                        status: str_of(t, "status")
                            .and_then(TaskStatus::parse)
                            .unwrap_or(TaskStatus::Pending),
                    })
                    .collect();
            }
        }
        let call = if repl::is_repl(name) {
            self.repl.started(id);
            call
        } else if repl::is_inner(id) {
            match self.repl.parent_for_inner() {
                Some(parent) => match self.nest(&parent, call) {
                    Some(call) => call,
                    None => return,
                },
                None => call,
            }
        } else {
            self.repl.left_script();
            call
        };
        let last = self.c.items.len().saturating_sub(1);
        if let Some(ChatItem {
            kind: ItemKind::Tools(calls),
            ..
        }) = self.c.items.last_mut()
        {
            calls.push(call);
            let at = (last, calls.len() - 1);
            self.call_index.insert(id.to_string(), at);
        } else {
            self.append(ItemKind::Tools(vec![call]), date);
            self.call_index
                .insert(id.to_string(), (self.c.items.len() - 1, 0));
        }
    }

    fn user(&mut self, obj: &Value, date: Option<Timestamp>) {
        self.queue.user_line(obj);
        if obj.get("isMeta").and_then(Value::as_bool) == Some(true) {
            return;
        }
        let Some(message) = obj.get("message").filter(|m| m.is_object()) else {
            return;
        };
        if obj.get("isCompactSummary").and_then(Value::as_bool) == Some(true) {
            if self.c.items.last().map(|i| &i.kind) != Some(&compacted()) {
                self.append(compacted(), date);
            }
            return;
        }
        let result = obj.get("toolUseResult").filter(|r| r.is_object());
        if let Some(text) = str_of(message, "content") {
            if let Some(id) = model_id_in_confirmation(text) {
                self.c.facts.model = Some(id);
            }
            if let Some(kind) = user_item(text, 0) {
                let kind = self.merged_kind(kind, text);
                self.append_user(kind, date, true);
            }
            return;
        }
        let Some(blocks) = message.get("content").and_then(Value::as_array) else {
            return;
        };
        let mut text = String::new();
        let mut images = 0;
        for block in blocks {
            match str_of(block, "type") {
                Some("tool_result") => self.tool_result(block, result, date),
                Some("text") => {
                    if !text.is_empty() {
                        text.push_str("\n\n");
                    }
                    text.push_str(str_of(block, "text").unwrap_or(""));
                }
                Some("image") => images += 1,
                _ => {}
            }
        }
        if !text.is_empty() || images > 0 {
            if let Some(kind) = user_item(&text, images) {
                let kind = self.merged_kind(kind, &text);
                self.append_user(kind, date, true);
            }
        }
    }

    fn tool_result(&mut self, block: &Value, result: Option<&Value>, date: Option<Timestamp>) {
        let tool_use_id = str_of(block, "tool_use_id");
        if tool_use_id.is_some()
            && tool_use_id == self.last_question.as_ref().map(|q| q.tool_use_id.as_str())
        {
            self.last_question_answered = true;
        }
        let is_error = block.get("is_error").and_then(Value::as_bool) == Some(true);
        if let Some((ids, ended)) = result_end(tool_use_id, is_error, result) {
            for id in ids {
                self.c
                    .finished_agents
                    .insert(id.to_string(), ended.to_string());
            }
        }
        if let Some((agent, word)) = resume_end(is_error, result) {
            self.c
                .finished_agents
                .insert(agent.to_string(), word.to_string());
        }
        self.stops
            .result(tool_use_id, is_error, &mut self.c.finished_agents);
        let Some(&(item, index)) = tool_use_id.and_then(|id| self.call_index.get(id)) else {
            return;
        };
        let Some(ItemKind::Tools(calls)) = self.c.items.get(item).map(|i| &i.kind) else {
            return;
        };
        let Some(mut call) = calls.get(index).cloned() else {
            return;
        };
        if call.kind == ToolKind::Repl {
            if let Some(eval) = repl::dispatched_eval(result) {
                self.repl.dispatched(eval, &call.id);
                return;
            }
        }
        let text = result_text(block.get("content"));
        call.finished = true;
        call.is_error = is_error;
        call.output = clip(&text, result);
        if call.kind == ToolKind::Repl && call.output.is_none() {
            call.output = clip(&text, None);
        }
        let patch = result
            .and_then(|r| r.get("structuredPatch"))
            .and_then(Value::as_array)
            .filter(|p| !p.is_empty());
        if let Some(patch) = patch {
            call.diff = diff_lines(patch);
        } else if call.kind == ToolKind::Write {
            if let Some(content) = result.and_then(|r| str_of(r, "content")) {
                call.diff = written_lines(content);
                if result.and_then(|r| str_of(r, "type")) == Some("create") {
                    call.title = call.title.replace("Wrote ", "Created ");
                }
            }
        }
        if matches!(call.kind, ToolKind::Edit | ToolKind::Write) {
            call.output = None;
        }
        if call.kind == ToolKind::Browser {
            browser::finish(&mut call, &text, block.get("content"), is_error);
            call.input = String::new();
            if !call.images().is_empty() {
                self.image_calls.push_back(call.id.clone());
            }
        }
        if call.kind == ToolKind::Shell {
            if let Some(launch) = shells::launch(&text) {
                if !self.c.shells.iter().any(|s| s.id == launch.id) {
                    self.c.shells.push(BackgroundShell {
                        id: launch.id,
                        command: call.input.clone(),
                        title: call.title.clone(),
                        output_path: launch.path,
                        description: call.description.clone(),
                        started: date,
                    });
                }
            }
        }
        let name = call.name.clone();
        if let Some(ChatItem {
            kind: ItemKind::Tools(calls),
            ..
        }) = self.c.items.get_mut(item)
        {
            calls[index] = call;
        }
        self.drop_old_images();
        apply_task_result(&name, result, &mut self.c);
    }

    fn queue_operation(&mut self, obj: &Value) {
        if str_of(obj, "content").is_some_and(repl::is_event_text) {
            if str_of(obj, "operation") == Some("enqueue") {
                for event in repl::queued_event(obj) {
                    self.settle(event);
                }
            }
            return;
        }
        self.queue.operation(obj);
    }

    fn settle(&mut self, event: repl::Event) {
        let Some(&(item, index)) = self
            .repl
            .call_of(event.eval)
            .and_then(|id| self.call_index.get(id))
        else {
            return;
        };
        let Some(ChatItem {
            kind: ItemKind::Tools(calls),
            ..
        }) = self.c.items.get_mut(item)
        else {
            return;
        };
        let Some(call) = calls.get_mut(index) else {
            return;
        };
        call.finished = true;
        call.is_error = event.failed;
        call.output = clip(&event.text, None);
    }

    fn nest(&mut self, parent: &str, mut call: ToolCall) -> Option<ToolCall> {
        let Some(&(item, index)) = self.call_index.get(parent) else {
            return Some(call);
        };
        let Some(ChatItem {
            kind: ItemKind::Tools(calls),
            ..
        }) = self.c.items.get_mut(item)
        else {
            return Some(call);
        };
        let mut at = index + 1;
        while calls
            .get(at)
            .is_some_and(|c| c.parent.as_deref() == Some(parent))
        {
            at += 1;
        }
        call.parent = Some(parent.to_string());
        calls.insert(at, call);
        for (offset, moved) in calls[at..].iter().enumerate() {
            self.call_index
                .insert(moved.id.clone(), (item, at + offset));
        }
        None
    }

    fn drop_old_images(&mut self) {
        while self.image_calls.len() > browser::KEEP_IMAGE_CALLS {
            let Some(old) = self.image_calls.pop_front() else {
                break;
            };
            let Some(&(item, index)) = self.call_index.get(&old) else {
                continue;
            };
            if let Some(ChatItem {
                kind: ItemKind::Tools(calls),
                ..
            }) = self.c.items.get_mut(item)
            {
                if let Some(call) = calls.get_mut(index) {
                    let state = call.browser_mut();
                    state.images = Vec::new();
                    state.spots = Vec::new();
                }
            }
        }
    }

    fn attachment(&mut self, obj: &Value, date: Option<Timestamp>) {
        let Some(a) = obj.get("attachment") else {
            return;
        };
        if str_of(a, "type") == Some("poll_events") {
            let events = repl::envelopes(a);
            let evals: Vec<u64> = events.iter().map(|e| e.eval).collect();
            for event in events {
                self.settle(event);
            }
            self.repl.polled(&evals);
            return;
        }
        if str_of(a, "type") != Some("queued_command")
            || str_of(a, "commandMode").unwrap_or("prompt") != "prompt"
        {
            return;
        }
        let mut text = str_of(a, "prompt").unwrap_or("").to_string();
        let mut images = 0;
        if let Some(blocks) = a.get("prompt").and_then(Value::as_array) {
            text = blocks
                .iter()
                .filter_map(|b| str_of(b, "text"))
                .collect::<Vec<_>>()
                .join("\n\n");
            images = blocks
                .iter()
                .filter(|b| str_of(b, "type") == Some("image"))
                .count();
        }
        if let Some(kind @ ItemKind::User { .. }) = user_item(&text, images) {
            let kind = self.merged_kind(kind, &text);
            self.append(kind, date);
        }
    }

    fn merged_kind(&mut self, kind: ItemKind, raw: &str) -> ItemKind {
        match kind {
            ItemKind::User { text, images, .. } => ItemKind::User {
                text,
                images,
                merged: self.queue.merged(raw),
            },
            other => other,
        }
    }

    fn trim(&mut self) {
        if self.c.items.len() > TRIM_AT {
            let drop = self.c.items.len() - KEEP_ITEMS;
            self.c.items.drain(..drop);
            self.call_index = std::mem::take(&mut self.call_index)
                .into_iter()
                .filter_map(|(k, (item, call))| (item >= drop).then(|| (k, (item - drop, call))))
                .collect();
            self.thinking_at = self.thinking_at.and_then(|at| at.checked_sub(drop));
            let live = &self.call_index;
            self.image_calls.retain(|id| live.contains_key(id));
        }
        let live = &self.call_index;
        self.repl.trim(|id| live.contains_key(id));
        if self.counted_order.len() > 4000 {
            for id in self.counted_order.drain(..2000) {
                self.counted.remove(&id);
            }
        }
    }

    pub fn conversation(&self) -> Conversation {
        let mut c = self.c.clone();
        c.facts.files = self.files.clone();
        c.turn_tokens = self
            .turn_output
            .values()
            .fold(0u64, |sum, n| sum.saturating_add(*n));
        c.queued = self
            .queue
            .items()
            .iter()
            .filter_map(|q| match user_item(q, 0) {
                Some(ItemKind::User { text, .. }) => Some(text),
                _ => None,
            })
            .collect();
        if let Some(ChatItem {
            kind: ItemKind::Tools(calls),
            ..
        }) = c.items.last()
        {
            if let Some(open) = calls.iter().rev().find(|call| !call.finished) {
                c.activity = Some(open.title.clone());
            }
        }
        if c.items.len() > KEEP_ITEMS {
            let extra = c.items.len() - KEEP_ITEMS;
            c.items.drain(..extra);
        }
        c.pending_question = self
            .last_question
            .clone()
            .filter(|_| !self.last_question_answered);
        c
    }
}

fn oversized_tool_result(head: &[u8]) -> Option<(&str, bool)> {
    let message = find(head, br#""message":"#)?;
    find(&head[..message], br#""type":"user""#)?;
    const KEY: &[u8] = br#""tool_use_id":""#;
    let from = message + find(&head[message..], KEY)? + KEY.len();
    let len = head[from..].iter().position(|&b| b == b'"')?;
    let id = std::str::from_utf8(&head[from..from + len]).ok()?;
    let rest = &head[from + len..];
    let image = find(rest, br#""type":"image""#).is_some();
    Some((id, image))
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

fn compacted() -> ItemKind {
    ItemKind::notice("Conversation compacted", "rectangle.compress.vertical")
}
