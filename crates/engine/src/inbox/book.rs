use super::model::{ActionId, Item, Kind, Live, Outcome, State, REASON_MAX};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

pub const MAX_OPEN: usize = 200;
pub const MAX_DONE: usize = 300;
pub const DONE_KEEP_MS: i64 = 7 * 86_400_000;
pub const TITLE_MAX: usize = 120;
pub const DETAIL_MAX: usize = 400;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum Change {
    Raised(Item),
    Updated(Item),
    Removed(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AnswerError {
    Unknown,
    NotOpen(State),
    NotAnAction,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Book {
    items: Vec<Item>,
}

pub(super) fn cut(text: String, max: usize) -> String {
    if text.chars().count() <= max {
        return text;
    }
    let mut out: String = text.chars().take(max - 1).collect();
    out.push('…');
    out
}

fn is_notice(kind: Kind) -> bool {
    matches!(kind, Kind::Notice | Kind::LimitNotice)
}

fn is_secret(kind: Kind) -> bool {
    matches!(kind, Kind::SecretApproval | Kind::SecretRequest)
}

impl Book {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn from_items(items: Vec<Item>) -> Self {
        Self { items }
    }

    pub fn raise(&mut self, mut item: Item) -> Vec<Change> {
        item.title = cut(item.title, TITLE_MAX);
        item.detail = cut(item.detail, DETAIL_MAX);
        let now_ms = item.created_ms;
        let id = item.id.clone();
        let mut changes = Vec::new();
        match self.items.iter().position(|existing| existing.id == id) {
            Some(index) => {
                if self.items[index].state != State::Open {
                    return changes;
                }
                let before = &self.items[index];
                item.blocking = before.blocking;
                item.waiting_since = before.waiting_since;
                item.waiter = before.waiter;
                self.items[index] = item.clone();
                changes.push(Change::Updated(item));
            }
            None => {
                self.items.push(item.clone());
                changes.push(Change::Raised(item));
            }
        }
        if self.open_count() > MAX_OPEN {
            if let Some(index) = self.victim(&id) {
                if is_secret(self.items[index].kind) {
                    let item = &mut self.items[index];
                    item.state = State::Withdrawn;
                    item.closed_ms = Some(now_ms);
                    changes.push(Change::Updated(item.clone()));
                } else {
                    let gone = self.items.remove(index);
                    changes.push(Change::Removed(gone.id));
                }
            }
        }
        changes
    }

    pub fn answer(
        &mut self,
        id: &str,
        action: ActionId,
        by: &str,
        now_ms: i64,
    ) -> Result<Change, AnswerError> {
        self.answer_noted(id, action, by, None, now_ms)
    }

    pub fn answer_noted(
        &mut self,
        id: &str,
        action: ActionId,
        by: &str,
        note: Option<&str>,
        now_ms: i64,
    ) -> Result<Change, AnswerError> {
        let item = self
            .items
            .iter_mut()
            .find(|item| item.id == id)
            .ok_or(AnswerError::Unknown)?;
        if item.state != State::Open {
            return Err(AnswerError::NotOpen(item.state));
        }
        if !item.actions.iter().any(|offered| offered.id == action) {
            return Err(AnswerError::NotAnAction);
        }
        item.state = State::Answered;
        item.answer = Some(action);
        item.answered_by = Some(by.to_string());
        item.closed_ms = Some(now_ms);
        item.clear_live();
        item.note = note.map(str::to_string);
        Ok(Change::Updated(item.clone()))
    }

    pub fn set_note(&mut self, id: &str, note: &str) -> Option<Change> {
        let item = self.items.iter_mut().find(|item| item.id == id)?;
        if item.state != State::Open {
            return None;
        }
        item.note = Some(note.to_string());
        Some(Change::Updated(item.clone()))
    }

    pub fn resolve(
        &mut self,
        id: &str,
        state: State,
        answer: Option<ActionId>,
        by: Option<&str>,
        now_ms: i64,
    ) -> Option<Change> {
        let item = self.items.iter_mut().find(|item| item.id == id)?;
        if item.state != State::Open {
            return None;
        }
        item.state = state;
        item.answer = answer;
        item.answered_by = by.map(str::to_string);
        item.closed_ms = Some(now_ms);
        item.clear_live();
        Some(Change::Updated(item.clone()))
    }

    pub fn withdraw(&mut self, id: &str, now_ms: i64) -> Option<Change> {
        let item = self.items.iter_mut().find(|item| item.id == id)?;
        if !matches!(item.state, State::Open | State::Answered) {
            return None;
        }
        item.state = State::Withdrawn;
        item.closed_ms = Some(now_ms);
        item.clear_live();
        Some(Change::Updated(item.clone()))
    }

    pub fn set_live(&mut self, id: &str, live: Option<Live>) -> bool {
        let Some(item) = self.items.iter_mut().find(|item| item.id == id) else {
            return false;
        };
        if item.state != State::Open {
            return false;
        }
        let (blocking, waiting_since, waiter) = match live {
            Some(live) => (Some(true), Some(live.waiting_since), Some(live.waiter)),
            None => (None, None, None),
        };
        let changed = item.blocking != blocking
            || item.waiting_since != waiting_since
            || item.waiter != waiter;
        item.blocking = blocking;
        item.waiting_since = waiting_since;
        item.waiter = waiter;
        changed
    }

    pub fn set_outcome(&mut self, id: &str, mut outcome: Outcome) -> Option<Change> {
        let item = self.items.iter_mut().find(|item| item.id == id)?;
        outcome.reason = outcome.reason.map(|reason| cut(reason, REASON_MAX));
        item.outcome = Some(outcome);
        Some(Change::Updated(item.clone()))
    }

    pub fn get(&self, id: &str) -> Option<Item> {
        self.items.iter().find(|item| item.id == id).cloned()
    }

    pub fn remove(&mut self, id: &str) -> Option<Change> {
        let at = self.items.iter().position(|item| item.id == id)?;
        self.items.remove(at);
        Some(Change::Removed(id.to_string()))
    }

    pub fn expire(&mut self, now_ms: i64) -> Vec<Change> {
        self.expire_except(now_ms, &|_| false)
    }

    pub fn expire_except(&mut self, now_ms: i64, held: &dyn Fn(&str) -> bool) -> Vec<Change> {
        let mut changes = Vec::new();
        for item in &mut self.items {
            if item.state == State::Open
                && item.expires_ms.is_some_and(|at| at <= now_ms)
                && !held(&item.id)
            {
                item.state = State::Expired;
                item.clear_live();
                item.closed_ms = Some(now_ms);
                changes.push(Change::Updated(item.clone()));
            }
        }
        changes
    }

    pub fn mark_seen(&mut self, ids: &[String]) -> Vec<Change> {
        let mut changes = Vec::new();
        for item in &mut self.items {
            if !item.seen && ids.contains(&item.id) {
                item.seen = true;
                changes.push(Change::Updated(item.clone()));
            }
        }
        changes
    }

    pub fn prune(&mut self, now_ms: i64) {
        let closed_at = |item: &Item| item.closed_ms.unwrap_or(item.created_ms);
        self.items
            .retain(|item| item.state == State::Open || now_ms - closed_at(item) <= DONE_KEEP_MS);
        let mut closed: Vec<(i64, usize)> = self
            .items
            .iter()
            .enumerate()
            .filter(|(_, item)| item.state != State::Open)
            .map(|(index, item)| (closed_at(item), index))
            .collect();
        if closed.len() <= MAX_DONE {
            return;
        }
        closed.sort_by(|a, b| b.cmp(a));
        let gone: HashSet<usize> = closed[MAX_DONE..].iter().map(|(_, index)| *index).collect();
        let mut index = 0;
        self.items.retain(|_| {
            let keep = !gone.contains(&index);
            index += 1;
            keep
        });
    }

    pub fn items(&self) -> Vec<Item> {
        let mut out: Vec<Item> = self.items.iter().rev().cloned().collect();
        out.sort_by_key(|item| std::cmp::Reverse(item.created_ms));
        out
    }

    pub fn unread(&self) -> usize {
        self.items
            .iter()
            .filter(|item| item.state == State::Open && !item.seen)
            .count()
    }

    fn open_count(&self) -> usize {
        self.items
            .iter()
            .filter(|item| item.state == State::Open)
            .count()
    }

    fn victim(&self, keep: &str) -> Option<usize> {
        let oldest = |only_notices: bool| {
            self.items
                .iter()
                .enumerate()
                .filter(|(_, item)| {
                    item.id != keep
                        && !item.is_blocking()
                        && item.state == State::Open
                        && (!only_notices || is_notice(item.kind))
                })
                .min_by_key(|(_, item)| item.created_ms)
                .map(|(index, _)| index)
        };
        oldest(true).or_else(|| oldest(false))
    }
}
