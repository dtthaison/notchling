use std::collections::{HashMap, HashSet};

use notchling_engine::inbox::{ActionId, AlertAction, Item, Kind, State, Waiter, NOTE_MAX};
use notchling_locale as t;

use super::requests::{HookRequest, RequestKind};
use crate::attention::BlockingTarget;
use crate::inbox::consent_card::ALLOW_DELAY_MS;
use crate::inbox::tray_rows::{consent_id, deny_route};

pub const LAST_CALL_MS: i64 = 60_000;
pub const ANSWERED_MS: i64 = 2_000;
pub const EXPIRED_MS: i64 = 4_000;
pub const GROW_MS: i64 = 4_000;
pub const NOTE_CHARS: usize = NOTE_MAX;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Button {
    Allow,
    AllowOnce,
    Deny,
    Change,
    Open,
    OpenForm,
    Later,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NoteRoute {
    Consent(String),
    Approval(String),
    Hook(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AllowRoute {
    Consent(String),
    Switch { consent: String, switch: String },
    ApprovalOnce(String),
}

#[derive(Clone, Debug, PartialEq)]
pub struct BlockingCard {
    pub target: BlockingTarget,
    pub id: String,
    pub sessions: Vec<String>,
    pub title: String,
    pub detail: String,
    pub who: Vec<String>,
    pub since_ms: i64,
    pub ends_ms: Option<i64>,
    pub buttons: Vec<Button>,
    pub held: bool,
    pub note_route: Option<NoteRoute>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Closing {
    Answered,
    Allowed,
    Denied,
    Changed,
    Expired,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Farewell {
    pub id: String,
    pub title: String,
    pub kind: Closing,
    pub until_ms: i64,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct BlockingCards {
    cards: Vec<BlockingCard>,
    current: Option<String>,
    pub shown_ms: i64,
    pub second_seen: i64,
    pub close_pending: bool,
    pub farewell: Option<Farewell>,
    drafts: HashMap<String, String>,
    note_open: Option<String>,
    noted: HashMap<String, Closing>,
    inflight: HashSet<String>,
}

fn ends_at(card: &BlockingCard) -> i64 {
    card.ends_ms.unwrap_or(i64::MAX)
}

fn who_of(item: &Item) -> Vec<String> {
    let Some(place) = item.place.as_ref() else {
        return Vec::new();
    };
    [
        place.session_name.clone(),
        place.branch.clone(),
        place.machine_name.clone(),
    ]
    .into_iter()
    .flatten()
    .filter(|part| !part.is_empty())
    .collect()
}

fn sessions_of(item: &Item) -> Vec<String> {
    let mut sessions: Vec<String> = item
        .place
        .iter()
        .filter_map(|place| place.session.clone())
        .chain(item.source.session.clone())
        .collect();
    sessions.dedup();
    sessions
}

const ALLOW_KINDS: [&str; 7] = [
    "setting",
    "session_stop",
    "job_stop",
    "switch",
    "session",
    "dispatch",
    "job",
];

pub fn consent_kind(item: &Item) -> Option<&str> {
    item.open
        .as_ref()
        .and_then(|open| open.get("kind"))
        .and_then(|kind| kind.as_str())
}

fn approval_ticket(item: &Item) -> Option<&str> {
    item.open
        .as_ref()
        .and_then(|open| open.get("vault_approval"))
        .and_then(|ticket| ticket.as_str())
}

fn unverified(item: &Item) -> bool {
    let offers = |alert: AlertAction| item.alert_actions.contains(&alert);
    item.alert_actions.len() == 2 && offers(AlertAction::Deny) && offers(AlertAction::Open)
}

pub fn allow_route(item: &Item) -> Option<AllowRoute> {
    if unverified(item) {
        return None;
    }
    if item.kind == Kind::SecretApproval {
        let once = item.actions.iter().any(|a| a.id == ActionId::AllowOnce);
        return approval_ticket(item)
            .filter(|_| once)
            .map(|ticket| AllowRoute::ApprovalOnce(ticket.to_owned()));
    }
    let consent = consent_id(item)?;
    let allowed = match consent_kind(item) {
        Some(kind) => ALLOW_KINDS.contains(&kind),
        None => item.alert_actions.contains(&AlertAction::Allow),
    };
    if !allowed {
        return None;
    }
    Some(match &item.switch {
        Some(switch) => AllowRoute::Switch {
            consent,
            switch: switch.clone(),
        },
        None => AllowRoute::Consent(consent),
    })
}

pub fn note_route_of(item: &Item) -> Option<NoteRoute> {
    let offers = |alert: AlertAction| item.alert_actions.contains(&alert);
    if !offers(AlertAction::Change) || offers(AlertAction::OpenForm) {
        return None;
    }
    if item.kind == Kind::SecretApproval {
        return approval_ticket(item).map(|ticket| NoteRoute::Approval(ticket.to_owned()));
    }
    consent_id(item).map(NoteRoute::Consent)
}

pub fn item_buttons(item: &Item, answer_on: bool) -> Vec<Button> {
    let offers = |alert: AlertAction| item.alert_actions.contains(&alert);
    if offers(AlertAction::OpenForm) {
        return vec![Button::OpenForm, Button::Later];
    }
    let allow_word = |allow: Button| if answer_on { allow } else { Button::Open };
    let mut buttons = Vec::new();
    if item.kind == Kind::SecretApproval {
        buttons.push(if allow_route(item).is_some() {
            allow_word(Button::AllowOnce)
        } else {
            Button::Open
        });
        buttons.extend(offers(AlertAction::Deny).then_some(Button::Deny));
        buttons.extend(offers(AlertAction::Change).then_some(Button::Change));
    } else if consent_kind(item).is_some() {
        if unverified(item) {
            buttons.extend([Button::Deny, Button::Open]);
        } else {
            if allow_route(item).is_some() {
                buttons.extend([allow_word(Button::Allow), Button::Deny]);
            } else {
                buttons.extend([Button::Deny, Button::Open]);
            }
            buttons.extend(offers(AlertAction::Change).then_some(Button::Change));
        }
    } else if item.alert_actions.is_empty() {
        buttons.extend(deny_route(item).map(|_| Button::Deny));
        buttons.push(Button::Open);
    } else {
        buttons.extend(item.alert_actions.iter().filter_map(|alert| match alert {
            AlertAction::Allow => Some(allow_word(Button::Allow)),
            AlertAction::Open => Some(Button::Open),
            AlertAction::Deny => Some(Button::Deny),
            AlertAction::Change => Some(Button::Change),
            _ => None,
        }));
    }
    buttons.dedup();
    buttons.push(Button::Later);
    buttons
}

pub fn hook_buttons(held: bool, answer_on: bool) -> Vec<Button> {
    let mut buttons = Vec::new();
    buttons.push(if answer_on {
        Button::Allow
    } else {
        Button::Open
    });
    buttons.push(Button::Deny);
    if held {
        buttons.push(Button::Change);
    }
    buttons.push(Button::Later);
    buttons
}

fn is_listed(item: &Item) -> bool {
    item.state == State::Open
        && item.is_blocking()
        && item.waiter != Some(Waiter::Ask)
        && item.kind != Kind::Question
}

pub fn card_of_item(item: &Item, answer_on: bool, title: String, detail: String) -> BlockingCard {
    BlockingCard {
        target: BlockingTarget::Item(item.id.clone()),
        id: item.id.clone(),
        sessions: sessions_of(item),
        title,
        detail,
        who: who_of(item),
        since_ms: item.waiting_since.unwrap_or(item.created_ms),
        ends_ms: item.expires_ms,
        buttons: item_buttons(item, answer_on),
        held: false,
        note_route: note_route_of(item),
    }
}

pub fn card_of_hook(request: &HookRequest, answer_on: bool) -> Option<BlockingCard> {
    if request.kind != RequestKind::Permission || request.agy.is_some() {
        return None;
    }
    Some(BlockingCard {
        target: BlockingTarget::HookPrompt(request.id.clone()),
        id: request.id.clone(),
        sessions: vec![request.session_id.clone()],
        title: t::consent::notch_wants_to_use(&request.tool_name),
        detail: request.summary.clone(),
        who: Vec::new(),
        since_ms: (request.created * 1000.0) as i64,
        ends_ms: None,
        buttons: hook_buttons(true, answer_on),
        held: true,
        note_route: Some(NoteRoute::Hook(request.id.clone())),
    })
}

pub fn cards_from(
    items: &[Item],
    words: impl Fn(&Item) -> (String, String),
    hooks: &[&HookRequest],
    answer_on: bool,
) -> Vec<BlockingCard> {
    let mut cards: Vec<BlockingCard> = items
        .iter()
        .filter(|item| is_listed(item))
        .map(|item| {
            let (title, detail) = words(item);
            card_of_item(item, answer_on, title, detail)
        })
        .chain(hooks.iter().filter_map(|r| card_of_hook(r, answer_on)))
        .collect();
    cards.sort_by(|a, b| {
        ends_at(a)
            .cmp(&ends_at(b))
            .then(a.since_ms.cmp(&b.since_ms))
            .then(a.id.cmp(&b.id))
    });
    cards
}

impl BlockingCards {
    pub fn cards(&self) -> &[BlockingCard] {
        &self.cards
    }

    pub fn count(&self) -> usize {
        self.cards.len()
    }

    pub fn is_empty(&self) -> bool {
        self.cards.is_empty()
    }

    pub fn position(&self) -> usize {
        self.current
            .as_ref()
            .and_then(|id| self.cards.iter().position(|c| &c.id == id))
            .unwrap_or(0)
    }

    pub fn current(&self) -> Option<&BlockingCard> {
        self.cards.get(self.position())
    }

    pub fn find(&self, id: &str) -> Option<&BlockingCard> {
        self.cards.iter().find(|c| c.id == id)
    }

    pub fn select(&mut self, id: &str) -> bool {
        let known = self.cards.iter().any(|c| c.id == id);
        if known {
            if self.current().map(|c| c.id.as_str()) != Some(id) {
                self.note_open = None;
            }
            self.current = Some(id.to_owned());
        }
        known
    }

    pub fn step(&mut self, forward: bool, now_ms: i64) {
        let n = self.cards.len();
        if n < 2 {
            return;
        }
        let at = self.position();
        let next = if forward {
            (at + 1) % n
        } else {
            (at + n - 1) % n
        };
        self.current = Some(self.cards[next].id.clone());
        self.shown_ms = now_ms;
        self.note_open = None;
    }

    pub fn set(
        &mut self,
        cards: Vec<BlockingCard>,
        ended: &HashMap<String, Closing>,
        now_ms: i64,
    ) -> bool {
        let changed = self.cards != cards;
        let gone: Vec<&BlockingCard> = self
            .cards
            .iter()
            .filter(|old| !cards.iter().any(|c| c.id == old.id))
            .collect();
        let current_gone = self
            .current()
            .filter(|now| gone.iter().any(|old| old.id == now.id));
        let leaving = current_gone
            .or_else(|| {
                gone.iter()
                    .copied()
                    .find(|card| ended.get(&card.id) == Some(&Closing::Expired))
            })
            .map(|card| (card.id.clone(), card.title.clone()));
        if let Some((id, title)) = leaving {
            let kind = match ended.get(&id) {
                Some(Closing::Expired) => Closing::Expired,
                other => self
                    .noted
                    .remove(&id)
                    .or(other.copied())
                    .unwrap_or(Closing::Answered),
            };
            self.farewell = Some(Farewell {
                id,
                title,
                kind,
                until_ms: now_ms
                    + if kind == Closing::Expired {
                        EXPIRED_MS
                    } else {
                        ANSWERED_MS
                    },
            });
        }
        self.cards = cards;
        let keeps = self
            .current
            .as_ref()
            .is_some_and(|id| self.cards.iter().any(|c| &c.id == id));
        if !keeps {
            self.current = self.cards.first().map(|c| c.id.clone());
            self.shown_ms = now_ms;
            self.note_open = None;
        }
        self.drafts
            .retain(|id, _| self.cards.iter().any(|c| &c.id == id));
        self.noted
            .retain(|id, _| self.cards.iter().any(|c| &c.id == id));
        self.inflight
            .retain(|id| self.cards.iter().any(|c| &c.id == id));
        changed
    }

    pub fn farewell_live(&self, now_ms: i64) -> Option<&Farewell> {
        self.farewell.as_ref().filter(|f| now_ms < f.until_ms)
    }

    pub fn clear_farewell(&mut self, now_ms: i64) -> bool {
        match &self.farewell {
            Some(f) if now_ms >= f.until_ms => {
                self.farewell = None;
                self.shown_ms = now_ms;
                true
            }
            _ => false,
        }
    }

    pub fn show_expired(&mut self, id: &str, until_ms: i64) -> bool {
        match self.farewell.as_mut() {
            Some(f) if f.id == id && f.kind == Closing::Expired => {
                f.until_ms = until_ms;
                true
            }
            _ => false,
        }
    }

    pub fn note_answer(&mut self, id: &str, how: Closing) {
        self.noted.insert(id.to_owned(), how);
    }

    pub fn note_open(&self) -> Option<&str> {
        self.note_open.as_deref()
    }

    pub fn open_note(&mut self, id: &str) {
        if self.find(id).is_some() {
            self.note_open = Some(id.to_owned());
        }
    }

    pub fn close_note(&mut self) {
        self.note_open = None;
    }

    pub fn note_to_send(&self, id: &str) -> Option<(NoteRoute, String)> {
        let route = self.find(id)?.note_route.clone()?;
        let note = self.draft(id).trim();
        if note.is_empty() {
            return None;
        }
        Some((route, note.chars().take(NOTE_CHARS).collect()))
    }

    pub fn note_settled(&mut self, id: &str, sent: bool) {
        if sent {
            self.drafts.remove(id);
            self.note_open = None;
            self.noted.insert(id.to_owned(), Closing::Changed);
        } else {
            self.noted.remove(id);
            if self.find(id).is_some() {
                self.note_open = Some(id.to_owned());
            }
        }
    }

    pub fn begin(&mut self, id: &str) -> bool {
        self.find(id).is_some() && self.inflight.insert(id.to_owned())
    }

    pub fn busy(&self, id: &str) -> bool {
        self.inflight.contains(id)
    }

    pub fn finish(&mut self, id: &str, sent: bool) {
        if !sent {
            self.inflight.remove(id);
        }
    }

    pub fn pressable(&self, id: &str, button: Button, now_ms: i64) -> bool {
        let answers = matches!(
            button,
            Button::Allow | Button::AllowOnce | Button::Deny | Button::Change
        );
        self.ready(button, now_ms) && !(answers && self.busy(id))
    }

    pub fn ready(&self, button: Button, now_ms: i64) -> bool {
        match button {
            Button::Allow | Button::AllowOnce => self.allow_ready(now_ms),
            _ => true,
        }
    }

    pub fn has_session(&self, session: &str) -> bool {
        self.cards
            .iter()
            .any(|c| c.sessions.iter().any(|s| s == session))
    }

    pub fn allow_ready(&self, now_ms: i64) -> bool {
        now_ms - self.shown_ms >= ALLOW_DELAY_MS
    }

    pub fn draft(&self, id: &str) -> &str {
        self.drafts.get(id).map_or("", String::as_str)
    }

    pub fn set_draft(&mut self, id: &str, text: &str) {
        let text: String = text.chars().take(NOTE_CHARS).collect();
        if text.is_empty() {
            self.drafts.remove(id);
        } else {
            self.drafts.insert(id.to_owned(), text);
        }
    }

    pub fn soonest_end(&self) -> Option<i64> {
        self.cards.first().and_then(|c| c.ends_ms)
    }
}

pub use super::blocking_words::*;

#[cfg(test)]
#[path = "blocking_tests.rs"]
mod tests;
