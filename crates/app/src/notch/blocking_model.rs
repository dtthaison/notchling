use std::collections::HashMap;

use notchling_engine::inbox::{Item, State};

use super::blocking::{
    cards_from, closing_of, closing_word, last_call, BlockingCard, Closing, GROW_MS,
};
use super::layout::{
    blocked_size, blocking_card_height, blocking_size, glance_size, CardDims, NotchState, Size,
};
use super::model::NotchModel;
use super::requests::{HookRequest, RequestKind};
use crate::attention::{BlockingTarget, Effect, Level};
use crate::inbox::model::words_of;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ClosedCount {
    pub count: usize,
    pub blocking: bool,
}

impl NotchModel {
    pub fn set_inbox_items(&mut self, items: &[Item], now_ms: i64) {
        self.waiting = items
            .iter()
            .filter(|item| {
                item.state == State::Open && !item.is_blocking() && item.waiting_since.is_some()
            })
            .count();
        self.blocking_items = items
            .iter()
            .filter(|item| item.is_blocking() || self.blocking.find(&item.id).is_some())
            .cloned()
            .collect();
        self.rebuild_blocking(now_ms);
    }

    pub(super) fn rebuild_blocking(&mut self, now_ms: i64) {
        let hooks: Vec<&HookRequest> = self
            .requests
            .values()
            .filter(|r| r.kind == RequestKind::Permission && !self.answered.contains(&r.id))
            .collect();
        let cards = cards_from(&self.blocking_items, words_of, &hooks, self.answer_on);
        let ended: HashMap<String, Closing> = self
            .blocking_items
            .iter()
            .filter_map(|item| closing_of(item).map(|how| (item.id.clone(), how)))
            .collect();
        self.blocking.set(cards, &ended, now_ms);
        self.settle_blocking(now_ms);
    }

    fn settle_blocking(&mut self, now_ms: i64) {
        let showing = matches!(self.state, NotchState::Blocking | NotchState::Glance);
        if showing && self.blocking.is_empty() && self.blocking.farewell_live(now_ms).is_none() {
            self.go(NotchState::Collapsed);
        }
    }

    pub fn open_blocking(&mut self, key: &str, ms: i64, stay: bool, now_ms: i64) {
        let busy = self.hovering()
            && matches!(
                self.state,
                NotchState::List | NotchState::Detail(_) | NotchState::Peek(_)
            );
        if busy {
            return;
        }
        let before = self.blocking.current().map(|c| c.id.clone());
        if self.blocking.select(key) {
            let after = self.blocking.current().map(|c| c.id.clone());
            if self.state != NotchState::Blocking || before != after {
                self.blocking.shown_ms = now_ms;
            }
        } else if !self.blocking.show_expired(key, now_ms + ms) {
            return;
        }
        self.blocking.close_pending = false;
        self.state_to(NotchState::Blocking);
        if !stay {
            self.schedule_close(now_ms + ms);
        }
    }

    pub fn grow_blocking(&mut self, key: &str, now_ms: i64) {
        if self.state != NotchState::Collapsed || !self.blocking.select(key) {
            return;
        }
        self.blocking.shown_ms = now_ms;
        self.state_to(NotchState::Glance);
        self.schedule_close(now_ms + GROW_MS);
    }

    pub fn apply_effects(&mut self, effects: &[Effect], now_ms: i64) {
        for effect in effects {
            self.apply_effect(effect, now_ms);
        }
    }

    pub fn apply_effect(&mut self, effect: &Effect, now_ms: i64) {
        match effect {
            Effect::OpenNotch { key, ms, stay } => {
                self.open_blocking(key_id(key), *ms, *stay, now_ms)
            }
            Effect::GrowNotch { key } => self.grow_blocking(key_id(key), now_ms),
            Effect::CloseNotch => self.close_when_shown(now_ms),
            _ => {}
        }
    }

    fn close_when_shown(&mut self, now_ms: i64) {
        if self.blocking.farewell_live(now_ms).is_some() {
            self.blocking.close_pending = true;
        } else {
            self.close_blocking();
        }
    }

    pub fn close_blocking(&mut self) {
        if matches!(self.state, NotchState::Blocking | NotchState::Glance) {
            self.go(NotchState::Collapsed);
        }
    }

    pub fn step_blocking(&mut self, forward: bool, now_ms: i64) {
        self.blocking.step(forward, now_ms);
    }

    pub fn blocking_tick(&mut self, now_ms: i64) -> bool {
        let cleared = self.blocking.clear_farewell(now_ms);
        if cleared {
            if std::mem::take(&mut self.blocking.close_pending) {
                self.close_blocking();
            }
            self.settle_blocking(now_ms);
        }
        let open = matches!(self.state, NotchState::Blocking | NotchState::Glance);
        let second = now_ms / 1000;
        let ticked = open && self.blocking.second_seen != second;
        self.blocking.second_seen = second;
        cleared || ticked
    }

    pub fn blocked_session(&self, id: &str) -> bool {
        self.blocking.has_session(id)
    }

    pub fn blocking_current(&self) -> Option<&BlockingCard> {
        self.blocking.current()
    }

    pub fn blocking_content_height(&self) -> f64 {
        match self.blocking.current() {
            Some(card) => blocking_card_height(CardDims {
                detail_chars: card.detail.chars().count(),
                title_chars: card.title.chars().count(),
                last_call: last_call(card, self.now_ms),
                note: self.blocking.note_open() == Some(card.id.as_str()),
                mono: card.held,
            }),
            None => blocking_card_height(CardDims {
                detail_chars: 0,
                title_chars: 0,
                last_call: false,
                note: false,
                mono: false,
            }),
        }
    }

    pub fn shown_farewell(&self) -> Option<(String, &'static str)> {
        self.blocking
            .farewell_live(self.now_ms)
            .map(|f| (f.title.clone(), closing_word(f.kind)))
    }

    pub fn glow(&self) -> bool {
        matches!(self.state, NotchState::Blocking | NotchState::Glance) && self.level != Level::Calm
    }

    pub fn animates(&self) -> bool {
        !self.reduce_motion
    }

    pub fn closed_count(&self) -> Option<ClosedCount> {
        if self.blocking.count() > 0 {
            Some(ClosedCount {
                count: self.blocking.count(),
                blocking: true,
            })
        } else if self.waiting > 0 {
            Some(ClosedCount {
                count: self.waiting,
                blocking: false,
            })
        } else {
            None
        }
    }

    pub fn blocking_size(&self, state: &NotchState) -> Size {
        match state {
            NotchState::Blocking => blocking_size(
                self.notch,
                self.width,
                self.blocking_content_height(),
                self.screen_h,
            ),
            NotchState::Glance => glance_size(self.notch, self.width),
            _ => blocked_size(self.notch),
        }
    }
}

fn key_id(key: &BlockingTarget) -> &str {
    match key {
        BlockingTarget::Item(id) | BlockingTarget::HookPrompt(id) => id,
    }
}

#[cfg(test)]
#[path = "blocking_flow_tests.rs"]
mod flow_tests;

#[cfg(test)]
#[path = "blocking_wire_tests.rs"]
mod wire_tests;
