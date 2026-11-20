use std::collections::HashMap;

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
use crate::chat::reload_feed::ReloadFeed;

pub const NOTE_MS: i64 = 6_000;
pub const ACK_RETRY_MS: i64 = 5_000;

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
    pub cancelling: bool,
}

pub fn parse_task(task: &Value) -> Option<Task> {
    if task.is_null() {
        return None;
    }
    Some(Task {
        phase: serde_json::from_value(task.clone()).ok()?,
        since_ms: task.get("since_ms").and_then(Value::as_i64).unwrap_or(0),
        session: task
            .get("session")
            .and_then(Value::as_str)
            .map(str::to_owned),
        cancelling: task.get("cancelling").and_then(Value::as_bool) == Some(true),
    })
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
    Starting,
    Waiting(Wait),
    Working,
    Cancelling,
    Needs(Need),
    Terminal,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Intent {
    Ask,
    Terminal,
    Starting,
    Following,
    Cancelling,
    Refused(String),
    Ended(Need),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Settle {
    Same,
    Changed,
    Close,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Card {
    pub session: String,
    pub lead: String,
    pub stage: Stage,
    pub command: String,
    pub intent: Intent,
    pub floor: u64,
}

impl Card {
    pub fn open(agent: &Agent, lead: String, floor: u64) -> Option<Card> {
        let (stage, intent) = match mode_of(agent)? {
            Mode::Reload => (Stage::Ask, Intent::Ask),
            Mode::Terminal => (Stage::Terminal, Intent::Terminal),
        };
        let id = agent.id.clone();
        let resume = agent.resume_session_id.as_deref().unwrap_or(&agent.id);
        Some(Card {
            command: t::chat::reload_terminal_command(&notchling_engine::lanes::quote(resume)),
            session: id,
            lead,
            stage,
            intent,
            floor,
        })
    }

    pub fn begin(&mut self, floor: u64) {
        self.intent = Intent::Starting;
        self.floor = floor;
    }

    pub fn settle(&mut self, rev: u64, task: Option<&Task>) -> Settle {
        let task = task.filter(|t| !t.phase.is_over() || rev > self.floor);
        let stage = match (&self.intent, task) {
            (Intent::Terminal, _) => Stage::Terminal,
            (_, Some(task)) => match &task.phase {
                Phase::Done { .. } | Phase::Cancelled => return Settle::Close,
                Phase::NeedsYou { need } => {
                    self.intent = Intent::Ended(need.clone());
                    self.floor = rev;
                    Stage::Needs(need.clone())
                }
                Phase::Waiting { .. } if task.cancelling || self.intent == Intent::Cancelling => {
                    Stage::Cancelling
                }
                Phase::Waiting { by } => Stage::Waiting(*by),
                Phase::Working { .. } => Stage::Working,
            },
            (Intent::Starting, None) => Stage::Starting,
            (Intent::Cancelling, None) => Stage::Cancelling,
            (Intent::Ended(need), None) => Stage::Needs(need.clone()),
            (Intent::Ask | Intent::Following | Intent::Refused(_), None) => Stage::Ask,
        };
        if self.stage == stage {
            return Settle::Same;
        }
        self.stage = stage;
        Settle::Changed
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
            Stage::Starting => t::chat::reload_starting().to_owned(),
            Stage::Waiting(Wait::Turn) => t::chat::reload_wait_turn().to_owned(),
            Stage::Waiting(Wait::Dialog) => t::chat::reload_wait_dialog().to_owned(),
            Stage::Waiting(Wait::Tasks { count }) => t::chat::reload_wait_tasks(*count),
            Stage::Working => t::chat::reload_working().to_owned(),
            Stage::Cancelling => t::chat::reload_cancelling().to_owned(),
            Stage::Needs(need) => Card::problem_text(need),
        })
    }

    pub fn refusal(&self) -> Option<&str> {
        match (&self.intent, &self.stage) {
            (Intent::Refused(text), Stage::Waiting(_) | Stage::Working) => Some(text),
            _ => None,
        }
    }

    pub fn note(&self) -> Option<String> {
        match self.stage {
            Stage::Waiting(_) if self.refusal().is_none() => {
                Some(t::chat::reload_wait_note().to_owned())
            }
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
            Stage::Starting | Stage::Working | Stage::Cancelling => Vec::new(),
            Stage::Needs(_) => vec![Act::TryAgain, Act::Close],
            Stage::Terminal => vec![Act::CopyCommand, Act::Close],
        }
    }

    pub fn live(&self) -> bool {
        matches!(
            self.stage,
            Stage::Starting | Stage::Waiting(_) | Stage::Working | Stage::Cancelling
        )
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Synced {
    pub acks: Vec<(String, u64)>,
    pub follows: Vec<(String, String)>,
    pub settle: Option<Settle>,
}

#[derive(Default)]
pub struct State {
    pub card: Option<Card>,
    pub notes: HashMap<String, (String, i64)>,
    pub handled: HashMap<String, u64>,
    pub acks: HashMap<String, (u64, i64)>,
    pub feed: ReloadFeed,
}

impl State {
    pub fn open(&mut self, mut card: Card) -> Card {
        card.settle(self.feed.rev(&card.session), self.feed.task(&card.session));
        self.card = Some(card.clone());
        card
    }

    pub fn card_for(&mut self, id: &str) -> Option<&mut Card> {
        self.card.as_mut().filter(|card| card.session == id)
    }

    pub fn sync(&mut self, feed: &ReloadFeed, now_ms: i64) -> Synced {
        let mut out = Synced::default();
        if feed.boot != self.feed.boot {
            self.handled.clear();
            self.acks.clear();
            if let Some(card) = self.card.as_mut() {
                card.floor = 0;
            }
        }
        for (id, rev, task) in feed.ended() {
            if self.handled.get(id).is_some_and(|seen| *seen >= rev) {
                continue;
            }
            self.handled.insert(id.to_owned(), rev);
            let carded = self
                .card
                .as_ref()
                .is_some_and(|card| card.session == id && rev > card.floor);
            match &task.phase {
                Phase::Done { .. } => {
                    let text = match &task.session {
                        Some(_) => t::chat::reload_done_new(),
                        None => t::chat::reload_done(),
                    };
                    let under = task.session.as_deref().unwrap_or(id);
                    self.notes
                        .insert(under.to_owned(), (text.to_owned(), now_ms + NOTE_MS));
                    if let Some(next) = &task.session {
                        out.follows.push((id.to_owned(), next.clone()));
                    }
                }
                Phase::NeedsYou { need } if !carded => {
                    self.notes
                        .insert(id.to_owned(), (Card::problem_text(need), now_ms + NOTE_MS));
                }
                _ => {}
            }
        }
        if let Some(card) = self.card.as_mut() {
            let settle = card.settle(feed.rev(&card.session), feed.task(&card.session));
            if settle == Settle::Close {
                self.card = None;
            }
            out.settle = Some(settle);
        }
        self.feed = feed.clone();
        out.acks = self.due_acks(now_ms);
        out
    }

    pub fn due_acks(&mut self, now_ms: i64) -> Vec<(String, u64)> {
        let mut due = Vec::new();
        for (id, rev, _) in self.feed.ended() {
            let send = self
                .acks
                .get(id)
                .is_none_or(|(seen, at)| *seen < rev || (*seen == rev && now_ms >= *at));
            if send {
                self.acks.insert(id.to_owned(), (rev, i64::MAX));
                due.push((id.to_owned(), rev));
            }
        }
        due
    }

    pub fn ack_failed(&mut self, id: &str, rev: u64, now_ms: i64) {
        if let Some(entry) = self.acks.get_mut(id).filter(|(seen, _)| *seen == rev) {
            entry.1 = now_ms + ACK_RETRY_MS;
        }
    }

    pub fn live(&self, id: &str) -> bool {
        self.feed.live(id)
            || self
                .card
                .as_ref()
                .is_some_and(|c| c.session == id && c.live())
    }

    pub fn row_line(&self, id: &str) -> Option<&'static str> {
        self.live(id).then(t::chat::reload_row)
    }

    pub fn note(&self, id: &str, now_ms: i64) -> Option<String> {
        if let Some(card) = self.card.as_ref().filter(|c| c.session == id) {
            if card.live() {
                return card.status();
            }
        }
        if self.feed.live(id) {
            return Some(t::chat::reload_row().to_owned());
        }
        self.notes
            .get(id)
            .filter(|(_, until)| *until > now_ms)
            .map(|(text, _)| text.clone())
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
