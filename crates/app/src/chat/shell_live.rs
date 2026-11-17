use notchling_engine::sessions::{Agent, AgentTarget};
use notchling_engine::transcript::model::{Conversation, ItemKind};
use serde_json::Value;

use super::data::Tool;
use super::remote;
use super::rows::Row;

pub const EVERY_MS: i64 = 3_000;
pub const STEADY_MS: i64 = 8_000;
pub const FAILED_MS: i64 = 10_000;
pub const STEADY_AFTER: u32 = 2;
pub const GIVE_UP_AFTER: u32 = 6;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pending {
    pub session: String,
    pub item: String,
    pub command: String,
}

pub fn pending(session: &str, conversation: &Conversation) -> Option<Pending> {
    let item = conversation.items.last()?;
    let ItemKind::User { text, .. } = &item.kind else {
        return None;
    };
    let command = text.strip_prefix("! ")?.trim();
    (!command.is_empty()).then(|| Pending {
        session: session.to_owned(),
        item: item.id.clone(),
        command: command.to_owned(),
    })
}

pub fn readable(agent: &Agent, tool: Tool) -> bool {
    tool == Tool::Claude
        && agent.command.is_none()
        && !remote::is_remote(agent)
        && matches!(agent.target, AgentTarget::Job(_))
        && agent.can_reply()
}

pub fn lines_of(reply: &Value) -> Vec<String> {
    reply
        .get("lines")
        .and_then(Value::as_array)
        .map(|lines| {
            lines
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

pub fn skipped(reply: &Value) -> bool {
    reply.get("busy").and_then(Value::as_bool) == Some(true)
}

pub fn row(lines: &[String]) -> Row {
    Row::ShellLive {
        text: lines.join("\n"),
    }
}

#[derive(Debug, Default)]
pub struct Live {
    at: Option<Pending>,
    lines: Vec<String>,
    reading: Vec<String>,
    next_ms: i64,
    same: u32,
    empty: u32,
}

impl Live {
    pub fn sync(&mut self, want: Option<&Pending>) -> bool {
        if self.at.as_ref() == want {
            return false;
        }
        let shown = !self.lines.is_empty();
        *self = Live {
            at: want.cloned(),
            reading: std::mem::take(&mut self.reading),
            ..Live::default()
        };
        shown
    }

    pub fn reset(&mut self) {
        *self = Live {
            reading: std::mem::take(&mut self.reading),
            ..Live::default()
        };
    }

    pub fn due(&self, now_ms: i64) -> bool {
        self.at
            .as_ref()
            .is_some_and(|at| !self.reading.contains(&at.session))
            && self.empty < GIVE_UP_AFTER
            && now_ms >= self.next_ms
    }

    pub fn begin(&mut self) {
        if let Some(at) = &self.at {
            self.reading.push(at.session.clone());
        }
    }

    fn done(&mut self, from: &Pending) -> bool {
        if let Some(n) = self.reading.iter().position(|s| *s == from.session) {
            self.reading.remove(n);
        }
        self.at.as_ref() == Some(from)
    }

    pub fn arrived(&mut self, from: &Pending, lines: Vec<String>, now_ms: i64) -> bool {
        if !self.done(from) {
            return false;
        }
        if lines.is_empty() {
            self.empty += 1;
            self.same += 1;
        } else {
            self.empty = 0;
        }
        let changed = !lines.is_empty() && lines != self.lines;
        if changed {
            self.same = 0;
            self.lines = lines;
        } else if !lines.is_empty() {
            self.same += 1;
        }
        self.next_ms = now_ms + self.delay();
        changed
    }

    pub fn failed(&mut self, from: &Pending, now_ms: i64) {
        if !self.done(from) {
            return;
        }
        self.empty += 1;
        self.next_ms = now_ms + FAILED_MS;
    }

    pub fn skipped(&mut self, from: &Pending, now_ms: i64) {
        if self.done(from) {
            self.next_ms = now_ms + EVERY_MS;
        }
    }

    fn delay(&self) -> i64 {
        if self.same < STEADY_AFTER {
            EVERY_MS
        } else {
            STEADY_MS
        }
    }

    pub fn row(&self, shown: Option<&Pending>) -> Option<Row> {
        let want = shown?;
        (self.at.as_ref() == Some(want) && !self.lines.is_empty()).then(|| row(&self.lines))
    }
}
