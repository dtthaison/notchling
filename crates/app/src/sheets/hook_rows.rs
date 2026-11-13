use notchling_engine::install::summary;

use super::agents::{info, HOOKS};
use crate::welcome::facts::{HookFacts, PresentHook};
use crate::welcome::hooks::DETACHED;

pub const TITLE: &str = "Hooks";
pub const FOOTER: &str =
    "Read from Claude Code's settings.json when Settings opened. Install Hook in \
                          Welcome adds or changes them.";
pub const CHECKING: &str = "Checking Claude Code's settings";
pub const OTHER: &str = "Runs Notchling's hook";

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
            HookState::Installed => Some("Installed"),
            HookState::Missing => Some("Not installed"),
            HookState::Swift => Some("Swift app"),
            HookState::NotManaged => Some("Not managed"),
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
            HooksShown::Checking => Some(CHECKING.into()),
            HooksShown::Detached => Some(DETACHED.into()),
            HooksShown::Failed(e) => Some(e.clone()),
            HooksShown::Refused(why) => Some(format!(
                "Claude Code's settings.json was left alone: it {why}."
            )),
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
    let table: [(&str, &str); 9] = [
        ("valid JSON", "isn't valid JSON"),
        ("JSON object", "isn't a JSON object"),
        ("twice", "has a key written twice"),
        ("not an object", "has hooks that aren't an object"),
        ("not a list", "has a hook event that isn't a list"),
        ("link", "is a link to a missing file"),
        ("regular file", "isn't a regular file"),
        ("too big", "is too big"),
        ("read-only", "is read-only"),
    ];
    for (needle, text) in table {
        if reason.contains(needle) {
            return text;
        }
    }
    if reason.contains("UTF-8") {
        "isn't UTF-8 text"
    } else {
        "can't be read"
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
        what: info(event, matcher).map_or(OTHER, |h| h.what),
    }
}
