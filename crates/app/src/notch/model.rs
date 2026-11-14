use std::collections::{HashMap, HashSet};

use notchling_engine::sessions::{Agent, AgentGroup, AgentTarget};
use notchling_engine::transcript::snapshot::{PendingQuestion, QuestionItem, Snapshot};
use notchling_kit::Status;
use notchling_locale as t;

use super::layout::{
    collapsed_size, detail_size, list_size, peek_size, permission_height, question_height,
    row_height, NotchState, NotchWidth, PeekContent, Size, DEFAULT_NOTCH,
};
use super::prefs::NotchPrefs;
use super::requests::{HookRequest, RequestKind};

pub const TABS: [AgentGroup; 3] = [
    AgentGroup::NeedsInput,
    AgentGroup::Working,
    AgentGroup::Completed,
];

const OPEN_DELAY: i64 = 250;
const CLOSE_DELAY: i64 = 600;
const PEEK_WAITING: i64 = 45_000;
const PEEK_PLAIN: i64 = 8_000;

#[derive(Clone, Debug, PartialEq)]
pub enum Card {
    Question(PendingQuestion),
    Permission(Box<HookRequest>),
}

impl Card {
    pub fn height(&self) -> f64 {
        match self {
            Card::Question(q) => q.items.first().map_or(0.0, |item| {
                question_height(item.question.chars().count(), item.options.len())
            }),
            Card::Permission(r) => match &r.agy {
                Some(agy) => agy.height(),
                None => permission_height(r.summary.chars().count(), r.can_always_allow),
            },
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
struct Pending {
    state: NotchState,
    due_ms: i64,
    when_away: bool,
}

#[derive(Clone, Debug)]
pub struct NotchModel {
    pub agents: Vec<Agent>,
    pub snapshots: HashMap<String, Snapshot>,
    pub requests: HashMap<String, HookRequest>,
    pub state: NotchState,
    pub tab: AgentGroup,
    pub notch: Size,
    pub width: NotchWidth,
    pub show_last_reply: bool,
    pub daemon_note: Option<&'static str>,
    pub now_ms: i64,
    pub card_height: Option<f64>,
    pub answered: HashSet<String>,
    pub inbox_unread: usize,
    hovering: bool,
    pending: Option<Pending>,
    last_groups: Option<HashMap<String, AgentGroup>>,
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
            inbox_unread: 0,
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

    pub fn prompt_id(&self, id: &str) -> Option<String> {
        self.requests
            .get(id)
            .filter(|r| !self.answered.contains(&r.id))
            .map(|r| r.id.clone())
    }

    pub fn permission(&self, id: &str) -> Option<&HookRequest> {
        self.requests
            .get(id)
            .filter(|r| r.kind == RequestKind::Permission && !self.answered.contains(&r.id))
    }

    pub fn answer_key(&self, id: &str) -> Option<String> {
        match (self.requests.get(id), self.question(id)) {
            (Some(r), _) => Some(r.id.clone()),
            (None, Some(q)) => Some(q.tool_use_id),
            (None, None) => None,
        }
    }

    pub fn mark_answered_key(&mut self, key: &str) {
        self.answered.insert(key.to_owned());
    }

    pub fn mark_answered(&mut self, id: &str) {
        if let Some(key) = self.answer_key(id) {
            self.mark_answered_key(&key);
        }
    }

    pub fn card(&self, id: &str) -> Option<Card> {
        if let Some(r) = self.permission(id) {
            return Some(Card::Permission(Box::new(r.clone())));
        }
        self.question(id).map(Card::Question)
    }

    pub fn peek_text(&self, agent: &Agent) -> Option<String> {
        if !self.show_last_reply {
            return None;
        }
        self.snapshot(&agent.id)
            .and_then(|s| s.last_reply().map(str::to_owned))
            .or_else(|| agent.detail.clone())
    }

    pub fn status_line(&self, agent: &Agent) -> String {
        if let Some(r) = self.permission(&agent.id) {
            return t::notch::wants_to_use(&r.tool_name, &r.summary);
        }
        if let Some(first) = self
            .question(&agent.id)
            .and_then(|q| q.items.into_iter().next())
        {
            return t::notch::asks(&first.question);
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
            .unwrap_or_else(|| t::notch::thinking().to_owned())
    }

    pub fn row_height(&self, agent: &Agent) -> f64 {
        let has_prompt = self
            .snapshot(&agent.id)
            .is_some_and(|s| s.last_prompt.is_some());
        row_height(agent.group == AgentGroup::Completed, has_prompt)
    }

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

    pub fn apply(&mut self, prefs: &NotchPrefs) -> bool {
        let before = (self.width, self.show_last_reply, self.state.clone());
        self.width = prefs.width;
        self.show_last_reply = prefs.show_last_reply;
        if !prefs.shown {
            self.go(NotchState::Collapsed);
        }
        before != (self.width, self.show_last_reply, self.state.clone())
    }

    pub fn open(&mut self, id: &str) {
        self.go(NotchState::Detail(id.to_owned()));
    }

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
        let old_prompts: HashMap<String, String> = self
            .requests
            .iter()
            .map(|(row, r)| (row.clone(), r.id.clone()))
            .collect();
        self.agents = agents;
        self.snapshots = snapshots;
        self.requests = requests;
        for agent in &mut self.agents {
            let held = self
                .requests
                .get(&agent.id)
                .is_some_and(|r| !self.answered.contains(&r.id));
            if held && agent.group != AgentGroup::NeedsInput {
                agent.group = AgentGroup::NeedsInput;
                agent.label = t::notch::needs_input().into();
            }
        }
        self.agents.sort_by_key(|a| a.group);
        let current: HashMap<String, AgentGroup> = self
            .agents
            .iter()
            .map(|a| (a.id.clone(), a.group))
            .collect();
        let previous = self.last_groups.replace(current)?;
        let asked = self.agents.iter().find(|a| {
            self.question(&a.id)
                .is_some_and(|q| old_questions.get(&a.id) != Some(&q.tool_use_id))
                || self
                    .prompt_id(&a.id)
                    .is_some_and(|p| old_prompts.get(&a.id) != Some(&p))
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

pub fn status(group: AgentGroup) -> Status {
    match group {
        AgentGroup::NeedsInput => Status::Needs,
        AgentGroup::Working => Status::Working,
        AgentGroup::Completed => Status::Done,
    }
}

pub fn tab_title(group: AgentGroup) -> &'static str {
    match group {
        AgentGroup::NeedsInput => t::notch::tab_input(),
        AgentGroup::Working => t::notch::tab_working(),
        AgentGroup::Completed => t::notch::tab_done(),
    }
}

pub fn tab_id(group: AgentGroup) -> &'static str {
    match group {
        AgentGroup::NeedsInput => "input",
        AgentGroup::Working => "working",
        AgentGroup::Completed => "done",
    }
}

pub fn empty_text(model: &NotchModel) -> String {
    if model.agents.is_empty() {
        t::notch::empty_no_agents().to_owned()
    } else {
        match model.tab {
            AgentGroup::NeedsInput => t::notch::empty_input(),
            AgentGroup::Working => t::notch::empty_working(),
            AgentGroup::Completed => t::notch::empty_completed(),
        }
        .to_owned()
    }
}

pub fn short_host(host: &str) -> &str {
    match host {
        "Background" => t::notch::host_background(),
        "claude -p" => "-p",
        other => other,
    }
}

pub fn ago(since_ms: i64, now_ms: i64) -> String {
    let s = ((now_ms - since_ms) / 1000).max(0);
    match s {
        0..=59 => t::notch::age_now().to_owned(),
        60..=3599 => t::notch::age_minutes(s / 60),
        3600..=86_399 => t::notch::age_hours(s / 3600),
        _ => t::notch::age_days(s / 86_400),
    }
}

pub fn one_line(text: &str) -> String {
    text.replace('\n', " ").trim().to_owned()
}

pub fn last_paragraph(text: &str) -> String {
    let paragraph = text
        .rsplit("\n\n")
        .find(|p| !p.trim().is_empty())
        .unwrap_or(text);
    paragraph.chars().take(400).collect()
}

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
