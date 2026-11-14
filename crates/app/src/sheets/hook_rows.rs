use notchling_engine::install::summary;
use notchling_locale::{self as t, Text};

use super::agents::{info, HOOKS};
use crate::welcome::facts::{HookFacts, PresentHook};
use crate::welcome::hooks::DETACHED;

const LONGEST: usize = 40;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HookState {
    Installed,
    Missing,
    Swift,
    NotManaged,
    Unknown,
}

impl HookState {
    pub fn word(self) -> Option<&'static str> {
        match self {
            HookState::Installed => Some(t::settings::hook_installed()),
            HookState::Missing => Some(t::settings::hook_missing()),
            HookState::Swift => Some(t::settings::hook_swift()),
            HookState::NotManaged => Some(t::settings::hook_not_managed()),
            HookState::Unknown => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HookLine {
    pub title: String,
    pub state: HookState,
    pub command: Option<String>,
    pub what: &'static str,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HooksShown {
    Checking,
    Detached,
    Failed(String),
    Refused(&'static str),
    Found(Vec<PresentHook>),
}

impl HooksShown {
    pub fn from_plan(plan: Result<HookFacts, String>) -> HooksShown {
        match plan {
            Err(e) if e == DETACHED => HooksShown::Detached,
            Err(e) => HooksShown::Failed(e),
            Ok(facts) if facts.state == "refused" => {
                HooksShown::Refused(category(facts.refused.as_deref()))
            }
            Ok(facts) => HooksShown::Found(facts.entries),
        }
    }

    pub fn status(&self) -> Option<String> {
        match self {
            HooksShown::Checking => Some(t::settings::hooks_checking().into()),
            HooksShown::Detached => Some(DETACHED.into()),
            HooksShown::Failed(e) => Some(e.clone()),
            HooksShown::Refused(why) => Some((*why).to_owned()),
            HooksShown::Found(_) => None,
        }
    }

    pub fn lines(&self) -> Vec<HookLine> {
        let HooksShown::Found(present) = self else {
            return HOOKS
                .iter()
                .map(|h| slot_line(h.event, h.matcher, HookState::Unknown, None))
                .collect();
        };
        let mut used = vec![false; present.len()];
        let mut lines: Vec<HookLine> = HOOKS
            .iter()
            .map(|h| {
                let find = |swift: bool, used: &[bool]| {
                    present.iter().enumerate().position(|(i, p)| {
                        !used[i]
                            && p.swift == swift
                            && p.event == h.event
                            && p.matcher == h.matcher
                            && (swift || p.managed)
                    })
                };
                let (state, at) = match (find(false, &used), find(true, &used)) {
                    (Some(i), _) => (HookState::Installed, Some(i)),
                    (None, Some(i)) => (HookState::Swift, Some(i)),
                    (None, None) => (HookState::Missing, None),
                };
                if let Some(i) = at {
                    used[i] = true;
                }
                let command = at
                    .filter(|_| state == HookState::Installed)
                    .map(|i| summary(&present[i].command));
                slot_line(h.event, h.matcher, state, command)
            })
            .collect();
        for (p, _) in present.iter().zip(&used).filter(|(_, u)| !**u) {
            let state = if p.swift {
                HookState::Swift
            } else {
                HookState::NotManaged
            };
            lines.push(slot_line(
                &p.event,
                &p.matcher,
                state,
                Some(summary(&p.command)),
            ));
        }
        lines
    }
}

fn category(reason: Option<&str>) -> &'static str {
    let reason = reason.unwrap_or_default();
    let table: [(&str, Text); 9] = [
        ("valid JSON", t::settings::HOOKS_REFUSED_INVALID_JSON),
        ("JSON object", t::settings::HOOKS_REFUSED_NOT_OBJECT),
        ("twice", t::settings::HOOKS_REFUSED_TWICE),
        ("not an object", t::settings::HOOKS_REFUSED_HOOKS_NOT_OBJECT),
        ("not a list", t::settings::HOOKS_REFUSED_EVENT_NOT_LIST),
        ("link", t::settings::HOOKS_REFUSED_BROKEN_LINK),
        ("regular file", t::settings::HOOKS_REFUSED_NOT_REGULAR),
        ("too big", t::settings::HOOKS_REFUSED_TOO_BIG),
        ("read-only", t::settings::HOOKS_REFUSED_READ_ONLY),
    ];
    for (needle, text) in table {
        if reason.contains(needle) {
            return text.get();
        }
    }
    if reason.contains("UTF-8") {
        t::settings::hooks_refused_not_utf8()
    } else {
        t::settings::hooks_refused_unreadable()
    }
}

fn cut(text: &str) -> String {
    text.chars().take(LONGEST).collect()
}

fn slot_line(event: &str, matcher: &str, state: HookState, command: Option<String>) -> HookLine {
    let matcher = if matcher.trim().is_empty() {
        "*"
    } else {
        matcher.trim()
    };
    HookLine {
        title: format!("{} · {}", cut(event), cut(matcher)),
        state,
        command,
        what: info(event, matcher).map_or(t::settings::hooks_other(), |h| h.what.get()),
    }
}
