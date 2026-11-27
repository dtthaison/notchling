use std::collections::{HashMap, HashSet};

use notchling_engine::inbox::{ActionId, Item, Kind, OutcomeState, State, DONE_KEEP_MS};
use notchling_locale as t;

use super::model::{decided, row_of, words_of, ChangeRoute, InboxModel, Row};
use crate::chat::format::age;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Section {
    Blocking,
    Waiting,
    Running,
    DidntWork,
    Info,
    Done,
}

impl Section {
    pub const TABS: [Section; 5] = [
        Section::Waiting,
        Section::Running,
        Section::DidntWork,
        Section::Info,
        Section::Done,
    ];

    pub fn tab_index(self) -> usize {
        Self::TABS
            .iter()
            .position(|section| *section == self)
            .unwrap_or(0)
    }
}

fn wants_an_answer(item: &Item) -> bool {
    item.actions
        .iter()
        .any(|action| action.id != ActionId::Done)
}

fn closed_at(item: &Item) -> i64 {
    item.closed_ms.unwrap_or(item.created_ms)
}

fn ends_at(item: &Item) -> i64 {
    item.expires_ms.unwrap_or(i64::MAX)
}

fn did_not_work(item: &Item, now_ms: i64) -> Section {
    if now_ms - closed_at(item) > DONE_KEEP_MS {
        Section::Done
    } else {
        Section::DidntWork
    }
}

pub fn panel_max(viewport_h: f32) -> f32 {
    super::MAX_HEIGHT
        .min(viewport_h - super::OUTSIDE)
        .max(super::PANEL_MIN)
}

pub fn blocking_cap(panel_max: f32) -> f32 {
    let tabs_kept = panel_max - super::PANEL_CHROME - super::TABS_SHARE;
    (panel_max - super::BLOCKING_RESERVE)
        .max(tabs_kept.min(super::BLOCKING_MIN))
        .max(0.0)
}

pub fn is_applying(item: &Item) -> bool {
    item.outcome
        .as_ref()
        .is_some_and(|outcome| outcome.state == OutcomeState::Applying)
}

fn asks_a_question_of_the_user(item: &Item) -> bool {
    matches!(
        item.kind,
        Kind::Consent
            | Kind::SettingChange
            | Kind::SecretApproval
            | Kind::SecretRequest
            | Kind::SessionWaits
    ) || item
        .open
        .as_ref()
        .and_then(super::model::consent_of)
        .is_some()
}

pub fn section_of(item: &Item, now_ms: i64) -> Section {
    if is_applying(item) {
        return Section::Running;
    }
    if item.state == State::Open {
        return if item.is_blocking() {
            Section::Blocking
        } else if wants_an_answer(item) {
            Section::Waiting
        } else if item.seen {
            Section::Done
        } else {
            Section::Info
        };
    }
    match item.outcome.as_ref().map(|outcome| outcome.state) {
        Some(OutcomeState::Applying) => return Section::Running,
        Some(OutcomeState::Failed | OutcomeState::Withdrawn) => return did_not_work(item, now_ms),
        _ => {}
    }
    if item.state == State::Expired && asks_a_question_of_the_user(item) {
        return did_not_work(item, now_ms);
    }
    Section::Done
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Triage {
    pub blocking: Vec<Row>,
    pub waiting: Vec<Row>,
    pub running: Vec<Row>,
    pub didnt_work: Vec<Row>,
    pub info: Vec<Row>,
    pub done: Vec<Row>,
    pub last_answer: Option<String>,
}

impl Triage {
    pub fn rows(&self, section: Section) -> &[Row] {
        match section {
            Section::Blocking => &self.blocking,
            Section::Waiting => &self.waiting,
            Section::Running => &self.running,
            Section::DidntWork => &self.didnt_work,
            Section::Info => &self.info,
            Section::Done => &self.done,
        }
    }

    fn bucket(&mut self, section: Section) -> &mut Vec<Row> {
        match section {
            Section::Blocking => &mut self.blocking,
            Section::Waiting => &mut self.waiting,
            Section::Running => &mut self.running,
            Section::DidntWork => &mut self.didnt_work,
            Section::Info => &mut self.info,
            Section::Done => &mut self.done,
        }
    }
}

impl InboxModel {
    pub fn triage(&self) -> Triage {
        let now = self.now_ms;
        let mut by: HashMap<Section, Vec<&Item>> = HashMap::new();
        for item in &self.items {
            let section = section_of(item, now);
            let section = if section == Section::DidntWork && self.cleared.contains(&item.id) {
                Section::Done
            } else {
                section
            };
            by.entry(section).or_default().push(item);
        }
        let mut triage = Triage::default();
        for (section, mut items) in by {
            match section {
                Section::Blocking => items.sort_by(|a, b| {
                    ends_at(a)
                        .cmp(&ends_at(b))
                        .then(a.created_ms.cmp(&b.created_ms))
                        .then(a.id.cmp(&b.id))
                }),
                Section::Done | Section::DidntWork => {
                    items.sort_by(|a, b| closed_at(b).cmp(&closed_at(a)).then(a.id.cmp(&b.id)))
                }
                _ => items.sort_by(|a, b| b.created_ms.cmp(&a.created_ms).then(a.id.cmp(&b.id))),
            }
            *triage.bucket(section) = items
                .into_iter()
                .map(|item| row_of(item, now, section))
                .collect();
        }
        triage.last_answer = self
            .items
            .iter()
            .filter(|item| item.state == State::Answered && item.answer.is_some())
            .max_by_key(|item| closed_at(item))
            .map(|item| {
                t::inbox::last_answer(
                    &words_of(item).0,
                    &decided(item.answer).to_lowercase(),
                    &age(now, closed_at(item)),
                )
            });
        triage
    }

    pub fn clear_row(&mut self, id: &str) {
        self.cleared.insert(id.to_owned());
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Strip {
    pub id: String,
    pub title: String,
}

pub fn blocking_for(model: &InboxModel, node: Option<&str>, sessions: &[&str]) -> Option<Strip> {
    let names = |item: &Item| {
        let place = item
            .place
            .as_ref()
            .and_then(|place| place.session.as_deref());
        let source = item.source.session.as_deref();
        let item_node = item.source.node.as_deref().or(item
            .place
            .as_ref()
            .and_then(|place| place.machine.as_deref()));
        item_node == node
            && sessions
                .iter()
                .any(|session| place == Some(*session) || source == Some(*session))
    };
    model
        .items
        .iter()
        .filter(|item| {
            item.state == State::Open && item.is_blocking() && !is_applying(item) && names(item)
        })
        .min_by(|a, b| {
            ends_at(a)
                .cmp(&ends_at(b))
                .then(a.created_ms.cmp(&b.created_ms))
                .then(a.id.cmp(&b.id))
        })
        .map(|item| Strip {
            id: item.id.clone(),
            title: words_of(item).0,
        })
}

pub fn key_target<'a>(triage: &'a Triage, tab: Section, id: &str) -> Option<&'a Row> {
    triage
        .blocking
        .iter()
        .chain(triage.rows(tab))
        .find(|row| row.id == id && row.enabled)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyAction {
    None,
    CancelNote,
    Step(isize),
    Open,
    Deny,
}

pub fn key_action(key: &str, platform: bool, change_open: bool, panel_focused: bool) -> KeyAction {
    if change_open {
        return if key == "escape" {
            KeyAction::CancelNote
        } else {
            KeyAction::None
        };
    }
    if !panel_focused {
        return KeyAction::None;
    }
    match key {
        "down" => KeyAction::Step(1),
        "up" => KeyAction::Step(-1),
        "enter" => KeyAction::Open,
        "d" if platform => KeyAction::Deny,
        _ => KeyAction::None,
    }
}

pub fn change_busy(busy: &HashSet<String>, row_id: &str, route: &ChangeRoute) -> bool {
    busy.contains(row_id) || matches!(route, ChangeRoute::Consent(id) if busy.contains(id))
}

#[derive(Clone, Debug, Default)]
pub struct RowErrors {
    held: HashMap<String, (String, Item)>,
}

impl RowErrors {
    pub fn record(&mut self, model: &InboxModel, id: &str, message: &str) {
        if let Some(item) = model.item(id) {
            self.held
                .insert(id.to_owned(), (message.to_owned(), item.clone()));
        }
    }

    pub fn sync(&mut self, model: &InboxModel) {
        self.held
            .retain(|id, (_, item)| model.item(id) == Some(&*item));
    }

    pub fn of(&self, id: &str) -> Option<&str> {
        self.held.get(id).map(|(message, _)| message.as_str())
    }
}
