use super::wire::BridgeFailure;
use notchling_protocol::generated as wire;
use std::collections::BTreeMap;

pub use wire::{
    ChatMessage, Command, Facts, LastMessage, Limit, PromptQuestion, SessionState, Subagent,
    VizSpec,
};

pub const ALIVE_REASON: &str =
    "Close the session where it is open (Claude desktop / VS Code), then take it over";

pub const ALREADY_CONTROLLABLE: &str = "Notchling can already control this session.";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Control {
    ConsolePty,
    Job(String),
    ViewOnly,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TakeoverState {
    Available,
    Stoppable { busy: bool },
    Unavailable(String),
}

#[derive(Debug, Clone, PartialEq)]
pub enum PromptKind {
    Permission,
    Question(Vec<PromptQuestion>),
}

#[derive(Debug, Clone, PartialEq)]
pub struct PendingPrompt {
    pub id: String,
    pub session_id: String,
    pub kind: PromptKind,
    pub tool_name: String,
    pub summary: String,
    pub created_ms: f64,
}

impl PendingPrompt {
    pub fn to_wire(&self) -> wire::Prompt {
        let (kind, questions) = match &self.kind {
            PromptKind::Permission => (wire::PromptKind::Permission, Vec::new()),
            PromptKind::Question(items) => (wire::PromptKind::Question, items.clone()),
        };
        wire::Prompt {
            request_id: self.id.clone(),
            session_id: self.session_id.clone(),
            kind,
            tool_name: self.tool_name.clone(),
            summary: self.summary.clone(),
            questions,
            created: self.created_ms,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PromptResponse {
    Allow,
    Deny,
    Answers(Vec<String>),
}

#[derive(Debug, Clone, PartialEq)]
pub struct SessionSubagent {
    pub info: Subagent,
    pub transcript_path: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Session {
    pub id: String,
    pub pid: Option<i32>,
    pub name: String,
    pub project: String,
    pub cwd: String,
    pub entrypoint: String,
    pub kind: String,
    pub state: SessionState,
    pub status_updated_ms: f64,
    pub started_ms: f64,
    pub transcript_path: Option<String>,
    pub subagents: Vec<SessionSubagent>,
    pub pending: Option<PendingPrompt>,
    pub control: Control,
    pub takeover: Option<TakeoverState>,
    pub activity: Option<String>,
    pub facts: Option<Facts>,
    pub last_message: Option<LastMessage>,
    pub tool: String,
}

impl Session {
    pub fn new(id: &str, name: &str, cwd: &str, control: Control, state: SessionState) -> Session {
        let project = std::path::Path::new(cwd)
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        Session {
            id: id.to_string(),
            pid: None,
            name: name.to_string(),
            project,
            cwd: cwd.to_string(),
            entrypoint: "cli".into(),
            kind: "interactive".into(),
            state,
            status_updated_ms: 0.0,
            started_ms: 0.0,
            transcript_path: None,
            subagents: Vec::new(),
            pending: None,
            control,
            takeover: None,
            activity: None,
            facts: None,
            last_message: None,
            tool: "claude".into(),
        }
    }

    pub fn last_activity_ms(&self) -> f64 {
        self.subagents
            .iter()
            .map(|s| s.info.last_activity)
            .fold(self.status_updated_ms, f64::max)
    }

    pub fn to_wire(&self) -> wire::Session {
        wire::Session {
            id: self.id.clone(),
            name: self.name.clone(),
            project: self.project.clone(),
            cwd: self.cwd.clone(),
            entrypoint: self.entrypoint.clone(),
            state: self.state,
            control: match self.control {
                Control::ConsolePty => wire::SessionControl::Console,
                Control::Job(_) => wire::SessionControl::Job,
                Control::ViewOnly => wire::SessionControl::ViewOnly,
            },
            status_updated_at: self.status_updated_ms,
            started_at: self.started_ms,
            last_activity: self.last_activity_ms(),
            subagents: self.subagents.iter().map(|s| s.info.clone()).collect(),
            pending: self.pending.as_ref().map(PendingPrompt::to_wire),
            takeover: self.takeover.as_ref().map(takeover_wire),
            activity: self.activity.clone(),
            facts: self.facts.clone(),
            last_message: self.last_message.clone(),
            tool: self.tool.clone(),
        }
    }
}

fn takeover_wire(state: &TakeoverState) -> wire::Takeover {
    let (available, reason, stoppable, busy) = match state {
        TakeoverState::Available => (true, None, false, false),
        TakeoverState::Stoppable { busy } => (false, Some(ALIVE_REASON.to_string()), true, *busy),
        TakeoverState::Unavailable(why) => (false, Some(why.clone()), false, false),
    };
    wire::Takeover {
        available,
        reason,
        stoppable,
        busy,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelChoice {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TakeoverOutcome {
    pub session_id: String,
    pub stopped_pid: Option<i32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PhoneOptions {
    pub device_window_id: Option<u32>,
    pub fps: i64,
    pub width: i64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct BridgeViz {
    pub suggested_tab: String,
    pub web_url: Option<String>,
    pub specs: BTreeMap<String, VizSpec>,
    pub web_candidates: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ChatPage {
    pub messages: Vec<ChatMessage>,
    pub has_more: bool,
    pub reset: bool,
}

impl ChatPage {
    pub fn page(
        all: &[ChatMessage],
        before: Option<&str>,
        after: Option<&str>,
        limit: usize,
    ) -> Result<ChatPage, BridgeFailure> {
        let newest = |list: &[ChatMessage], reset: bool| ChatPage {
            messages: list[list.len().saturating_sub(limit)..].to_vec(),
            has_more: list.len() > limit,
            reset,
        };
        if let Some(before) = before {
            let index = all
                .iter()
                .position(|m| m.id == before)
                .ok_or_else(|| BridgeFailure::not_found("Message \"before\" not found."))?;
            return Ok(newest(&all[..index], false));
        }
        if let Some(after) = after {
            return Ok(match all.iter().position(|m| m.id == after) {
                Some(index) => newest(&all[index + 1..], false),
                None => newest(all, true),
            });
        }
        Ok(newest(all, false))
    }
}
