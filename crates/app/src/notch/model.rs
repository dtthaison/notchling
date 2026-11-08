//! What the notch knows and decides, pure (`NotchModel` without the reply box): the rows, each
//! session's snapshot and held prompt, the state, the tab, the shape's size, when to peek and when
//! hovering opens or folds it. The window feeds it the mouse and the clock; nothing here draws.

use std::collections::{HashMap, HashSet};

use notchling_engine::sessions::{Agent, AgentGroup, AgentTarget};
use notchling_engine::transcript::snapshot::{PendingQuestion, QuestionItem, Snapshot};
use notchling_kit::Status;

use super::layout::{
    collapsed_size, detail_size, list_size, peek_size, permission_height, question_height,
    row_height, NotchState, NotchWidth, PeekContent, Size, DEFAULT_NOTCH,
};
use super::requests::{HookRequest, RequestKind};

/// The tabs in order; the order is also the rows' sort order.
pub const TABS: [AgentGroup; 3] = [
    AgentGroup::NeedsInput,
    AgentGroup::Working,
    AgentGroup::Completed,
];

/// Hovering the closed strip opens the list after this long (ms).
const OPEN_DELAY: i64 = 250;
/// Leaving the open shape folds it after this long, unless the pointer comes back.
const CLOSE_DELAY: i64 = 600;
/// How long a peek stays out with the pointer away: a question or permission waits on you.
const PEEK_WAITING: i64 = 45_000;
const PEEK_PLAIN: i64 = 8_000;

/// A question or permission prompt on show.
#[derive(Clone, Debug, PartialEq)]
pub enum Card {
    Question(PendingQuestion),
    Permission(HookRequest),
}

impl Card {
    /// Its estimated height (`questionHeight`, `permissionHeight`); the first question's when
    /// there are several, since the read-only card shows one at a time.
    pub fn height(&self) -> f64 {
        match self {
            Card::Question(q) => q.items.first().map_or(0.0, |item| {
                question_height(item.question.chars().count(), item.options.len())
            }),
            Card::Permission(r) => permission_height(r.summary.chars().count(), r.can_always_allow),
        }
    }
}

/// A state change waiting for its moment.
#[derive(Clone, Debug, PartialEq)]
struct Pending {
    state: NotchState,
    due_ms: i64,
    /// Only while the pointer is away (folding); opening needs it to stay.
    when_away: bool,
}

/// The notch's data and state.
#[derive(Clone, Debug)]
pub struct NotchModel {
    /// Needs input, then working, then done, newest first in each (`sessions::list`'s order).
    pub agents: Vec<Agent>,
    pub snapshots: HashMap<String, Snapshot>,
    /// Prompts the hook holds open, by session id.
    pub requests: HashMap<String, HookRequest>,
    pub state: NotchState,
    pub tab: AgentGroup,
    /// The camera housing.
    pub notch: Size,
    pub width: NotchWidth,
    /// Off, a peek shows only who finished or wants you.
    pub show_last_reply: bool,
    /// What the list says about the daemon while the rows are read from the files (it is down, or
    /// not trusted); `None` says nothing.
    pub daemon_note: Option<&'static str>,
    /// The clock for ages, in milliseconds since the epoch.
    pub now_ms: i64,
    /// The card on show's drawn height, which beats the estimate (`cardHeight`); the view sets it.
    pub card_height: Option<f64>,
    /// Prompts answered from here (a hook request's id, a question's tool use id), hidden until
    /// the session's files catch up.
    pub answered: HashSet<String>,
    hovering: bool,
    pending: Option<Pending>,
    /// Each row's group at the last update; `None` before the first, which only sets the baseline.
    last_groups: Option<HashMap<String, AgentGroup>>,
    /// Kept so an open conversation stays readable if its agent drops off the list.
    detail_agent: Option<Agent>,
}

impl Default for NotchModel {
    fn default() -> Self {
        NotchModel {
            agents: Vec::new(),
            snapshots: HashMap::new(),
            requests: HashMap::new(),
            state: NotchState::Collapsed,
            tab: AgentGroup::NeedsInput,
            notch: DEFAULT_NOTCH,
            width: NotchWidth::default(),
            show_last_reply: true,
            daemon_note: None,
            now_ms: 0,
            card_height: None,
            answered: HashSet::new(),
            hovering: false,
            pending: None,
            last_groups: None,
            detail_agent: None,
        }
    }
}

impl NotchModel {
    pub fn agent(&self, id: &str) -> Option<&Agent> {
        self.agents
            .iter()
            .find(|a| a.id == id)
            .or_else(|| self.detail_agent.as_ref().filter(|a| a.id == id))
    }

    pub fn snapshot(&self, id: &str) -> Option<&Snapshot> {
        self.snapshots.get(id)
    }

    pub fn count(&self, group: AgentGroup) -> usize {
        self.agents.iter().filter(|a| a.group == group).count()
    }

    pub fn rows(&self, group: AgentGroup) -> Vec<&Agent> {
        self.agents.iter().filter(|a| a.group == group).collect()
    }

    /// The question a session is asking, from the best source there is: the hook holding it, the
    /// transcript, then the summary in the job's state. Only one ever shows.
    pub fn question(&self, id: &str) -> Option<PendingQuestion> {
        if let Some(r) = self.requests.get(id) {
            if let Some(q) = r.question() {
                return (!self.answered.contains(&r.id)).then(|| q.clone());
            }
        }
        self.snapshot(id)
            .and_then(|s| s.question.clone())
            .or_else(|| self.agent(id).and_then(question_from_needs))
            .filter(|q| !self.answered.contains(&q.tool_use_id))
    }

    /// The permission prompt the hook holds for a session.
    pub fn permission(&self, id: &str) -> Option<&HookRequest> {
        self.requests
            .get(id)
            .filter(|r| r.kind == RequestKind::Permission && !self.answered.contains(&r.id))
    }

    /// Hides a session's card once it was answered from here; the key is the hook request's id,
    /// else the question's tool use id.
    pub fn mark_answered(&mut self, id: &str) {
        let key = match (self.requests.get(id), self.question(id)) {
            (Some(r), _) => Some(r.id.clone()),
            (None, Some(q)) => Some(q.tool_use_id),
            (None, None) => None,
        };
        self.answered.extend(key);
    }

    /// The card to show for a session: a permission, else a question.
    pub fn card(&self, id: &str) -> Option<Card> {
        if let Some(r) = self.permission(id) {
            return Some(Card::Permission(r.clone()));
        }
        self.question(id).map(Card::Question)
    }

    /// The agent's last message for a peek, or `None` when that's turned off or there is none.
    pub fn peek_text(&self, agent: &Agent) -> Option<String> {
        if !self.show_last_reply {
            return None;
        }
        self.snapshot(&agent.id)
            .and_then(|s| s.last_reply().map(str::to_owned))
            .or_else(|| agent.detail.clone())
    }

    /// The colored line under a row that isn't done: what it wants, or what it's doing.
    pub fn status_line(&self, agent: &Agent) -> String {
        if let Some(r) = self.permission(&agent.id) {
            return format!("Wants to use {}: {}", r.tool_name, r.summary);
        }
        if let Some(first) = self
            .question(&agent.id)
            .and_then(|q| q.items.into_iter().next())
        {
            return format!("Asks: {}", first.question);
        }
        let snapshot = self.snapshot(&agent.id);
        if agent.group == AgentGroup::NeedsInput {
            return agent
                .detail
                .clone()
                .or_else(|| snapshot.and_then(|s| s.last_reply().map(str::to_owned)))
                .unwrap_or_else(|| agent.label.clone());
        }
        snapshot
            .and_then(|s| s.activity.clone())
            .or_else(|| agent.detail.clone())
            .unwrap_or_else(|| "Thinking…".to_owned())
    }

    /// One row's height.
    pub fn row_height(&self, agent: &Agent) -> f64 {
        let has_prompt = self
            .snapshot(&agent.id)
            .is_some_and(|s| s.last_prompt.is_some());
        row_height(agent.group == AgentGroup::Completed, has_prompt)
    }

    /// The black shape's size for a state: the single source of truth.
    pub fn size(&self, state: &NotchState) -> Size {
        match state {
            NotchState::Collapsed => collapsed_size(self.notch),
            NotchState::Peek(id) => {
                let content = match self.card(id) {
                    Some(card) => PeekContent::Card {
                        height: card.height(),
                    },
                    None => PeekContent::Plain {
                        has_text: self.agent(id).is_some_and(|a| self.peek_text(a).is_some()),
                    },
                };
                peek_size(self.notch, self.width, content, self.card_height)
            }
            NotchState::List => {
                let rows: Vec<f64> = self
                    .rows(self.tab)
                    .into_iter()
                    .map(|a| self.row_height(a))
                    .collect();
                list_size(
                    self.notch,
                    self.width,
                    &rows,
                    self.tab == AgentGroup::Completed,
                )
            }
            NotchState::Detail(id) => detail_size(self.notch, self.width, self.card(id).is_some()),
        }
    }

    /// Moves to a state. Opening the list from closed or a peek shows the tab most likely to need
    /// you; anything waiting is dropped.
    pub fn go(&mut self, state: NotchState) {
        if state == self.state {
            return;
        }
        self.pending = None;
        let peeking = matches!(self.state, NotchState::Peek(_));
        if state == NotchState::List && (self.state == NotchState::Collapsed || peeking) {
            self.tab = TABS
                .into_iter()
                .find(|g| self.count(*g) > 0)
                .unwrap_or(AgentGroup::NeedsInput);
        }
        if let NotchState::Detail(id) = &state {
            self.detail_agent = self.agents.iter().find(|a| &a.id == id).cloned();
        }
        self.state = state;
        self.card_height = None;
    }

    /// Opens a session's conversation.
    pub fn open(&mut self, id: &str) {
        self.go(NotchState::Detail(id.to_owned()));
    }

    /// The pointer entered or left the shape (`setHover`). Hovering the strip opens the list after a
    /// moment; leaving the open shape folds it after a moment, unless it comes back; a peek that
    /// waits on you starts its long timer over instead.
    pub fn hover(&mut self, inside: bool, now_ms: i64) {
        if inside == self.hovering {
            return;
        }
        self.hovering = inside;
        self.pending = None;
        if inside {
            if self.state == NotchState::Collapsed {
                self.schedule(NotchState::List, now_ms + OPEN_DELAY, false);
            }
            return;
        }
        match &self.state {
            NotchState::Peek(id) => {
                let wait = if self.card(id).is_some() {
                    PEEK_WAITING
                } else {
                    CLOSE_DELAY
                };
                self.schedule(NotchState::Collapsed, now_ms + wait, true);
            }
            NotchState::List | NotchState::Detail(_) => {
                self.schedule(NotchState::Collapsed, now_ms + CLOSE_DELAY, true)
            }
            NotchState::Collapsed => {}
        }
    }

    pub fn hovering(&self) -> bool {
        self.hovering
    }

    fn schedule(&mut self, state: NotchState, due_ms: i64, when_away: bool) {
        self.pending = Some(Pending {
            state,
            due_ms,
            when_away,
        });
    }

    /// Applies a waiting change whose time has come. True when the state changed.
    pub fn tick(&mut self, now_ms: i64) -> bool {
        let Some(p) = self.pending.clone() else {
            return false;
        };
        if now_ms < p.due_ms {
            return false;
        }
        self.pending = None;
        if p.when_away == self.hovering {
            return false;
        }
        let before = self.state.clone();
        self.go(p.state);
        before != self.state
    }

    /// Slides out a peek for an agent, unless the notch is open or a peek is being read.
    pub fn peek(&mut self, id: &str, now_ms: i64) {
        let peeking = matches!(self.state, NotchState::Peek(_));
        if !(self.state == NotchState::Collapsed || (peeking && !self.hovering)) {
            return;
        }
        self.go(NotchState::Peek(id.to_owned()));
        let wait = if self.card(id).is_some() {
            PEEK_WAITING
        } else {
            PEEK_PLAIN
        };
        self.schedule(NotchState::Collapsed, now_ms + wait, true);
    }

    /// Takes a fresh poll and returns the agent to peek for, if any (`peekTransitions`): a new
    /// question beats someone newly waiting on you, which beats something finishing. The first poll
    /// only sets the baseline.
    pub fn update(
        &mut self,
        agents: Vec<Agent>,
        snapshots: HashMap<String, Snapshot>,
        requests: HashMap<String, HookRequest>,
    ) -> Option<String> {
        let old_questions: HashMap<String, String> = self
            .agents
            .iter()
            .filter_map(|a| Some((a.id.clone(), self.question(&a.id)?.tool_use_id)))
            .collect();
        self.agents = agents;
        self.snapshots = snapshots;
        self.requests = requests;
        let current: HashMap<String, AgentGroup> = self
            .agents
            .iter()
            .map(|a| (a.id.clone(), a.group))
            .collect();
        let previous = self.last_groups.replace(current)?;
        let asked = self.agents.iter().find(|a| {
            self.question(&a.id)
                .is_some_and(|q| old_questions.get(&a.id) != Some(&q.tool_use_id))
        });
        let changed: Vec<&Agent> = self
            .agents
            .iter()
            .filter(|a| {
                previous
                    .get(&a.id)
                    .is_some_and(|before| *before != a.group && a.group != AgentGroup::Working)
            })
            .collect();
        asked
            .or_else(|| {
                changed
                    .iter()
                    .copied()
                    .find(|a| a.group == AgentGroup::NeedsInput)
            })
            .or_else(|| changed.first().copied())
            .map(|a| a.id.clone())
    }
}

/// The kit's status for a row's group.
pub fn status(group: AgentGroup) -> Status {
    match group {
        AgentGroup::NeedsInput => Status::Needs,
        AgentGroup::Working => Status::Working,
        AgentGroup::Completed => Status::Done,
    }
}

/// Short enough for three tabs to fit under the camera.
pub fn tab_title(group: AgentGroup) -> &'static str {
    match group {
        AgentGroup::NeedsInput => "Input",
        AgentGroup::Working => "Working",
        AgentGroup::Completed => "Done",
    }
}

/// What the empty list says.
pub fn empty_text(model: &NotchModel) -> String {
    if model.agents.is_empty() {
        "No Claude agents running".to_owned()
    } else {
        let title = match model.tab {
            AgentGroup::NeedsInput => "needs input",
            AgentGroup::Working => "working",
            AgentGroup::Completed => "completed",
        };
        format!("Nothing {title}")
    }
}

/// Fits a narrow row: "bg", "-p", or the app's name.
pub fn short_host(host: &str) -> &str {
    match host {
        "Background" => "bg",
        "claude -p" => "-p",
        other => other,
    }
}

/// "now", "5m", "3h", "2d".
pub fn ago(since_ms: i64, now_ms: i64) -> String {
    let s = ((now_ms - since_ms) / 1000).max(0);
    match s {
        0..=59 => "now".to_owned(),
        60..=3599 => format!("{}m", s / 60),
        3600..=86_399 => format!("{}h", s / 3600),
        _ => format!("{}d", s / 86_400),
    }
}

/// Newlines as spaces, trimmed: a row shows one line.
pub fn one_line(text: &str) -> String {
    text.replace('\n', " ").trim().to_owned()
}

/// The last paragraph is usually the question or the summary; at most 400 characters of it.
pub fn last_paragraph(text: &str) -> String {
    let paragraph = text
        .rsplit("\n\n")
        .find(|p| !p.trim().is_empty())
        .unwrap_or(text);
    paragraph.chars().take(400).collect()
}

/// A background job only writes its question into the transcript once it's answered. While it's
/// open, its state carries a summary like "answer: Which target? (Production · Staging · Local)".
/// Several questions ("Q1 (…); Q2 (…)") and a question with brackets go to the terminal.
pub fn question_from_needs(agent: &Agent) -> Option<PendingQuestion> {
    if agent.group != AgentGroup::NeedsInput || !matches!(agent.target, AgentTarget::Job(_)) {
        return None;
    }
    let needs = agent.detail.as_deref()?;
    let body = needs.strip_prefix("answer: ")?.strip_suffix(')')?;
    let (text, options) = body.rsplit_once(" (")?;
    let options: Vec<String> = options.split(" · ").map(|o| o.trim().to_owned()).collect();
    if options.len() < 2 || text.contains('(') {
        return None;
    }
    let details = vec![String::new(); options.len()];
    Some(PendingQuestion {
        tool_use_id: format!("needs:{}:{needs}", agent.id),
        items: vec![QuestionItem {
            header: String::new(),
            question: text.to_owned(),
            options,
            details,
            multi_select: false,
        }],
    })
}

#[cfg(test)]
#[path = "model_tests.rs"]
mod tests;
