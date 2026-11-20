use std::collections::{HashMap, HashSet};

use notchling_engine::catalog::ModelInfo;
use notchling_engine::cluster::id::Tool as ClusterTool;
use notchling_engine::cluster::main_sessions::tool_of_row;
use notchling_engine::reload::{estimate, Need, Phase, Wait};
use notchling_engine::sessions::{Agent, AgentTarget};
use notchling_locale as t;
use serde_json::Value;

use crate::accounts::cost::dollars;
use crate::chat::claude_picker::{find, model_label};
use crate::chat::data::{Session, Tool};
use crate::chat::format::tokens;

pub const POLL_MS: i64 = 1_000;
pub const NOTE_MS: i64 = 6_000;
pub const POLL_FAILS: u32 = 5;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Reload,
    Terminal,
}

pub fn mode(session: &Session) -> Option<Mode> {
    if session.tool != Tool::Claude {
        return None;
    }
    mode_of(&session.agent)
}

pub fn mode_of(agent: &Agent) -> Option<Mode> {
    if !matches!(tool_of_row(&agent.id), ClusterTool::Claude)
        || agent.node.is_some()
        || agent.headless
        || agent.command.is_some()
    {
        return None;
    }
    match agent.target {
        AgentTarget::Job(_) => Some(Mode::Reload),
        AgentTarget::Process(_) => Some(Mode::Terminal),
        AgentTarget::Resume { .. } => None,
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Task {
    pub phase: Phase,
    pub since_ms: i64,
    pub session: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Status {
    pub offered: bool,
    pub task: Option<Task>,
}

pub fn parse(value: &Value) -> Option<Status> {
    let offered = value.get("offered").and_then(Value::as_bool)?;
    let task = match value.get("task") {
        None | Some(Value::Null) => None,
        Some(task) => Some(Task {
            phase: serde_json::from_value(task.clone()).ok()?,
            since_ms: task.get("since_ms").and_then(Value::as_i64).unwrap_or(0),
            session: task
                .get("session")
                .and_then(Value::as_str)
                .map(str::to_owned),
        }),
    };
    Some(Status { offered, task })
}

pub fn lead(tokens_read: u64, model: Option<&str>, catalog: &[ModelInfo]) -> String {
    if tokens_read == 0 {
        return t::chat::reload_lead().to_owned();
    }
    let price = model
        .and_then(|id| find(catalog, id))
        .and_then(|info| info.price);
    let found = estimate(tokens_read, price.as_ref());
    match (found.usd, model) {
        (Some(usd), Some(id)) => t::chat::reload_cost(
            &tokens(tokens_read),
            &dollars(usd),
            &model_label(Some(id), catalog),
        ),
        _ => t::chat::reload_tokens(&tokens(tokens_read)),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Act {
    Reload,
    Cancel,
    CancelReload,
    Close,
    TryAgain,
    CopyCommand,
}

impl Act {
    pub fn label(self) -> &'static str {
        match self {
            Act::Reload => t::chat::reload_reload(),
            Act::Cancel => t::chat::reload_cancel(),
            Act::CancelReload => t::chat::reload_cancel_reload(),
            Act::Close => t::chat::reload_close(),
            Act::TryAgain => t::chat::reload_try_again(),
            Act::CopyCommand => t::chat::reload_copy(),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Stage {
    Ask,
    Waiting(Wait),
    Working,
    Needs(Need),
    Terminal,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Card {
    pub session: String,
    pub lead: String,
    pub stage: Stage,
    pub command: String,
}

impl Card {
    pub fn open(agent: &Agent, lead: String) -> Option<Card> {
        let stage = match mode_of(agent)? {
            Mode::Reload => Stage::Ask,
            Mode::Terminal => Stage::Terminal,
        };
        let id = agent.id.clone();
        let resume = agent.resume_session_id.as_deref().unwrap_or(&agent.id);
        Some(Card {
            command: t::chat::reload_terminal_command(&notchling_engine::lanes::quote(resume)),
            session: id,
            lead,
            stage,
        })
    }

    pub fn intro(&self) -> String {
        match self.stage {
            Stage::Terminal => t::chat::reload_terminal().to_owned(),
            _ => self.lead.clone(),
        }
    }

    pub fn problem_text(need: &Need) -> String {
        match need {
            Need::Busy { by: Wait::Turn } => t::chat::reload_busy_turn().to_owned(),
            Need::Busy { by: Wait::Dialog } => t::chat::reload_busy_dialog().to_owned(),
            Need::Busy {
                by: Wait::Tasks { .. },
            } => t::chat::reload_busy_tasks().to_owned(),
            Need::TasksUnknown => t::chat::reload_tasks_unknown().to_owned(),
            Need::Failed { text } => t::chat::reload_failed(text),
        }
    }

    pub fn status(&self) -> Option<String> {
        Some(match &self.stage {
            Stage::Ask | Stage::Terminal => return None,
            Stage::Waiting(Wait::Turn) => t::chat::reload_wait_turn().to_owned(),
            Stage::Waiting(Wait::Dialog) => t::chat::reload_wait_dialog().to_owned(),
            Stage::Waiting(Wait::Tasks { count }) => t::chat::reload_wait_tasks(*count),
            Stage::Working => t::chat::reload_working().to_owned(),
            Stage::Needs(need) => Card::problem_text(need),
        })
    }

    pub fn note(&self) -> Option<String> {
        match self.stage {
            Stage::Waiting(_) => Some(t::chat::reload_wait_note().to_owned()),
            _ => None,
        }
    }

    pub fn problem(&self) -> bool {
        matches!(self.stage, Stage::Needs(_))
    }

    pub fn acts(&self) -> Vec<Act> {
        match &self.stage {
            Stage::Ask => vec![Act::Reload, Act::Cancel],
            Stage::Waiting(_) => vec![Act::CancelReload],
            Stage::Working => Vec::new(),
            Stage::Needs(_) => vec![Act::TryAgain, Act::Close],
            Stage::Terminal => vec![Act::CopyCommand, Act::Close],
        }
    }

    pub fn live(&self) -> bool {
        matches!(self.stage, Stage::Waiting(_) | Stage::Working)
    }
}

#[derive(Default)]
pub struct State {
    pub card: Option<Card>,
    pub notes: HashMap<String, (String, i64)>,
    pub watching: HashSet<String>,
    pub asking: HashSet<String>,
    pub poll_fails: u32,
    pub last_poll_ms: i64,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Applied {
    Nothing,
    Changed,
    Finished { followed: Option<String> },
}

impl State {
    pub fn poll_failed(&mut self) -> bool {
        self.poll_fails += 1;
        if self.poll_fails >= POLL_FAILS {
            self.poll_fails = 0;
            return true;
        }
        false
    }

    pub fn apply(&mut self, id: &str, status: &Status, now_ms: i64) -> Applied {
        self.poll_fails = 0;
        let Some(task) = &status.task else {
            self.watching.remove(id);
            return Applied::Nothing;
        };
        let shown = self.card.as_ref().is_some_and(|c| c.session == id);
        let stage = match &task.phase {
            Phase::Waiting { by } => Stage::Waiting(*by),
            Phase::Working { .. } => Stage::Working,
            Phase::NeedsYou { need } => Stage::Needs(need.clone()),
            Phase::Cancelled => {
                self.watching.remove(id);
                if shown {
                    self.card = None;
                }
                return Applied::Finished { followed: None };
            }
            Phase::Done { .. } => {
                self.watching.remove(id);
                if shown {
                    self.card = None;
                }
                let text = match &task.session {
                    Some(_) => t::chat::reload_done_new(),
                    None => t::chat::reload_done(),
                };
                let under = task.session.as_deref().unwrap_or(id);
                self.notes
                    .insert(under.to_owned(), (text.to_owned(), now_ms + NOTE_MS));
                return Applied::Finished {
                    followed: task.session.clone(),
                };
            }
        };
        if matches!(stage, Stage::Waiting(_) | Stage::Working) {
            self.watching.insert(id.to_owned());
        } else {
            self.watching.remove(id);
        }
        let Some(card) = self.card.as_mut().filter(|c| c.session == id) else {
            if let Stage::Needs(need) = &stage {
                let text = Card::problem_text(need);
                self.notes.insert(id.to_owned(), (text, now_ms + NOTE_MS));
                return Applied::Finished { followed: None };
            }
            return Applied::Nothing;
        };
        if card.stage == stage {
            return Applied::Nothing;
        }
        card.stage = stage;
        Applied::Changed
    }

    pub fn live(&self, id: &str) -> bool {
        self.watching.contains(id)
            || self
                .card
                .as_ref()
                .is_some_and(|c| c.session == id && c.live())
    }

    pub fn start(&mut self, id: &str, now_ms: i64) {
        self.watching.insert(id.to_owned());
        self.asking.insert(id.to_owned());
        self.last_poll_ms = now_ms;
    }

    pub fn stop(&mut self, id: &str) {
        self.watching.remove(id);
    }

    pub fn row_line(&self, id: &str) -> Option<&'static str> {
        self.live(id).then(t::chat::reload_row)
    }

    pub fn polls(&self) -> Vec<String> {
        let mut ids: Vec<String> = self.watching.iter().cloned().collect();
        if let Some(card) = self.card.as_ref().filter(|c| c.live()) {
            if !ids.contains(&card.session) {
                ids.push(card.session.clone());
            }
        }
        ids.sort();
        ids
    }

    pub fn note(&self, id: &str, now_ms: i64) -> Option<String> {
        if let Some(card) = self.card.as_ref().filter(|c| c.session == id) {
            if card.live() {
                return card.status();
            }
        }
        if self.watching.contains(id) {
            return Some(t::chat::reload_row().to_owned());
        }
        self.notes
            .get(id)
            .filter(|(_, until)| *until > now_ms)
            .map(|(text, _)| text.clone())
    }

    pub fn due(&self, now_ms: i64) -> bool {
        (self.card.as_ref().is_some_and(Card::live) || !self.watching.is_empty())
            && now_ms - self.last_poll_ms >= POLL_MS
    }
}

impl super::sidebar::SidebarModel {
    pub fn with_reload(mut self, state: &State) -> Self {
        for row in self.sections.iter_mut().flat_map(|s| &mut s.rows) {
            if let Some(line) = state.row_line(&row.id) {
                row.status_line = line.to_owned();
            }
        }
        self
    }
}
