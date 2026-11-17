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
    pub key: String,
    pub command: String,
}

pub fn pending(conversation: &Conversation) -> Option<Pending> {
    let item = conversation.items.last()?;
    let ItemKind::User { text, .. } = &item.kind else {
        return None;
    };
    let command = text.strip_prefix("! ")?.trim();
    (!command.is_empty()).then(|| Pending {
        key: item.id.clone(),
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

pub fn row(lines: &[String]) -> Row {
    Row::ShellLive {
        text: lines.join("\n"),
    }
}

#[derive(Debug, Default)]
pub struct Live {
    key: Option<String>,
    lines: Vec<String>,
    busy: bool,
    next_ms: i64,
    same: u32,
    empty: u32,
}

impl Live {
    pub fn sync(&mut self, want: Option<&Pending>) -> bool {
        let key = want.map(|p| p.key.as_str());
        if self.key.as_deref() == key {
            return false;
        }
        let shown = !self.lines.is_empty();
        *self = Live {
            key: key.map(str::to_owned),
            ..Live::default()
        };
        shown
    }

    pub fn due(&self, now_ms: i64) -> bool {
        self.key.is_some() && !self.busy && self.empty < GIVE_UP_AFTER && now_ms >= self.next_ms
    }

    pub fn begin(&mut self) {
        self.busy = true;
    }

    pub fn arrived(&mut self, key: &str, lines: Vec<String>, now_ms: i64) -> bool {
        if self.key.as_deref() != Some(key) {
            return false;
        }
        self.busy = false;
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

    pub fn failed(&mut self, key: &str, now_ms: i64) {
        if self.key.as_deref() != Some(key) {
            return;
        }
        self.busy = false;
        self.empty += 1;
        self.next_ms = now_ms + FAILED_MS;
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
        (self.key.as_deref() == Some(want.key.as_str()) && !self.lines.is_empty())
            .then(|| row(&self.lines))
    }
}
