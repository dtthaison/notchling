use super::check::{CheckPart, CheckResult, CheckTier};
use super::{Method, Tool, Version};
use serde::{Deserialize, Serialize};
use std::sync::Mutex;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Cause {
    UserUpdate,
    SelfUpdate,
    Rollout,
    Rollback,
    Outside,
}

impl Cause {
    pub fn word(self) -> &'static str {
        match self {
            Cause::UserUpdate => "user_update",
            Cause::SelfUpdate => "self_update",
            Cause::Rollout => "rollout",
            Cause::Rollback => "rollback",
            Cause::Outside => "outside",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Fact {
    Seen {
        tool: Tool,
        version: Version,
    },
    Changed {
        tool: Tool,
        machine: String,
        from: Option<Version>,
        to: Option<Version>,
        method: Method,
        cause: Cause,
    },
    LoadFailed {
        doc: &'static str,
    },
    Drift {
        tool: Tool,
        machine: String,
        expected: Version,
        found: Version,
    },
    SelfUpdate {
        tool: Tool,
        machine: String,
        action: String,
        ok: bool,
        detail: String,
    },
    Checked {
        tool: Tool,
        version: Version,
        tier: CheckTier,
        result: CheckResult,
        failed_part: Option<CheckPart>,
        unknown_count: u32,
        duration_ms: u64,
    },
}

fn plain(text: &str) -> String {
    text.chars()
        .map(|c| match c.to_ascii_lowercase() {
            c if c.is_ascii_lowercase() || c.is_ascii_digit() || "_.-".contains(c) => c,
            _ => '_',
        })
        .collect()
}

fn word(version: &Option<Version>) -> String {
    version
        .as_ref()
        .map_or_else(|| "none".to_string(), |v| plain(v.as_str()))
}

impl Fact {
    pub fn audit_words(&self) -> (&'static str, String) {
        match self {
            Fact::Seen { tool, version } => (
                "versions.seen",
                format!("{} {}", tool.name(), plain(version.as_str())),
            ),
            Fact::Changed {
                tool,
                machine,
                from,
                to,
                cause,
                ..
            } => (
                "versions.changed",
                format!(
                    "{} {} {} to {} {}",
                    plain(machine),
                    tool.name(),
                    word(from),
                    word(to),
                    cause.word()
                ),
            ),
            Fact::LoadFailed { doc } => ("versions.load_failed", plain(doc)),
            Fact::Drift {
                tool,
                machine,
                expected,
                found,
            } => (
                "versions.drift",
                format!(
                    "{} {} {} expected {}",
                    plain(machine),
                    tool.name(),
                    plain(found.as_str()),
                    plain(expected.as_str())
                ),
            ),
            Fact::SelfUpdate {
                tool,
                machine,
                action,
                ok,
                detail,
            } => (
                "versions.self_update",
                format!(
                    "{} {} {} {} {}",
                    plain(machine),
                    tool.name(),
                    plain(action),
                    if *ok { "ok" } else { "failed" },
                    plain(detail)
                ),
            ),
            Fact::Checked {
                tool,
                version,
                tier,
                result,
                failed_part,
                unknown_count,
                duration_ms,
            } => (
                "versions.checked",
                format!(
                    "{} {} {} {} {} {} {}",
                    tool.name(),
                    plain(version.as_str()),
                    tier.word(),
                    result.word(),
                    failed_part.map_or("none", |p| p.word()),
                    unknown_count,
                    duration_ms
                ),
            ),
        }
    }
}

pub trait Recorder: Send + Sync {
    fn record(&self, fact: Fact);
}

#[derive(Default)]
pub struct MemoryRecorder {
    facts: Mutex<Vec<Fact>>,
}

impl MemoryRecorder {
    pub fn new() -> MemoryRecorder {
        MemoryRecorder::default()
    }

    pub fn facts(&self) -> Vec<Fact> {
        self.facts.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }
}

impl Recorder for MemoryRecorder {
    fn record(&self, fact: Fact) {
        self.facts
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(fact);
    }
}
