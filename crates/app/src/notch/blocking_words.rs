use notchling_engine::inbox::{ActionId, Item, State};
use notchling_locale as t;

use super::blocking::{BlockingCard, Button, Closing, LAST_CALL_MS};
use crate::chat::format::age;

pub fn button_word(button: Button) -> &'static str {
    match button {
        Button::Allow => t::consent::notch_allow(),
        Button::AllowOnce => t::consent::notch_allow_once(),
        Button::Deny => t::consent::notch_deny(),
        Button::Change => t::consent::notch_change(),
        Button::Open => t::consent::notch_open(),
        Button::OpenForm => t::consent::notch_open_form(),
        Button::Later => t::consent::notch_later(),
    }
}

pub fn button_label(card: &BlockingCard, button: Button) -> String {
    t::notch::blocking_button_for(button_word(button), &card.title)
}

pub fn step_label(forward: bool, at: usize, total: usize) -> String {
    let landing = if forward {
        (at + 1) % total.max(1)
    } else {
        (at + total.max(1) - 1) % total.max(1)
    };
    let word = if forward {
        t::notch::blocking_next()
    } else {
        t::notch::blocking_previous()
    };
    t::notch::blocking_stepper_label(
        word,
        &t::notch::blocking_step((landing + 1) as i64, total as i64),
    )
}

pub fn closing_word(kind: Closing) -> &'static str {
    match kind {
        Closing::Answered => t::notch::blocking_answered(),
        Closing::Allowed => t::consent::notch_allowed(),
        Closing::Denied => t::consent::notch_denied(),
        Closing::Changed => t::consent::notch_changed(),
        Closing::Expired => t::notch::blocking_expired(),
    }
}

pub fn closing_of(item: &Item) -> Option<Closing> {
    match item.state {
        State::Expired => Some(Closing::Expired),
        State::Answered => Some(match item.answer {
            Some(ActionId::Deny) if item.note.is_some() => Closing::Changed,
            Some(ActionId::Deny | ActionId::Cancel) => Closing::Denied,
            Some(
                ActionId::Allow | ActionId::AllowOnce | ActionId::AllowAlways | ActionId::Continue,
            ) => Closing::Allowed,
            _ => Closing::Answered,
        }),
        _ => None,
    }
}

fn clock(ms: i64) -> String {
    let s = (ms / 1000).max(0);
    format!("{}:{:02}", s / 60, s % 60)
}

pub fn time_words(card: &BlockingCard, now_ms: i64) -> String {
    if let Some(end) = card.ends_ms {
        let left = end - now_ms;
        if left <= LAST_CALL_MS {
            return t::notch::blocking_ends_in(&clock(left));
        }
    }
    t::notch::blocking_stopped(&waited(card, now_ms))
}

pub fn waited(card: &BlockingCard, now_ms: i64) -> String {
    let s = ((now_ms - card.since_ms) / 1000).max(0);
    if s < 60 {
        clock(s * 1000)
    } else {
        age(now_ms, card.since_ms)
    }
}

pub fn last_call(card: &BlockingCard, now_ms: i64) -> bool {
    card.ends_ms.is_some_and(|end| end - now_ms <= LAST_CALL_MS)
}
