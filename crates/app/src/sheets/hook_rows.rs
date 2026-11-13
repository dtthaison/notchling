use crate::welcome::facts::{HookFacts, PresentHook};
use crate::welcome::hooks::DETACHED;

pub const TITLE: &str = "Hooks";
pub const FOOTER: &str =
    "Read from Claude Code's settings.json when Settings opened. Install Hook in \
                          Welcome adds or changes them.";
pub const CHECKING: &str = "Checking Claude Code's settings";
pub const EMPTY: &str = "No hooks installed";
pub const EMPTY_NOTE: &str = "Use Install Hook in Welcome to add them.";
pub const SWIFT_TAG: &str = "Swift app";
pub const UNMANAGED_TAG: &str = "Not managed";

const LONGEST: usize = 40;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HookLine {
    pub title: String,
    pub note: Option<&'static str>,
    pub tag: Option<&'static str>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HooksShown {
    Checking,
    Detached,
    Failed(String),
    Refused(&'static str),
    Empty,
    Found(Vec<HookLine>),
}

impl HooksShown {
    pub fn from_plan(plan: Result<HookFacts, String>) -> HooksShown {
        match plan {
            Err(e) if e == DETACHED => HooksShown::Detached,
            Err(e) => HooksShown::Failed(e),
            Ok(facts) if facts.state == "refused" => {
                HooksShown::Refused(category(facts.refused.as_deref()))
            }
            Ok(facts) if facts.entries.is_empty() => HooksShown::Empty,
            Ok(facts) => HooksShown::Found(facts.entries.iter().map(line).collect()),
        }
    }

    pub fn lines(&self) -> Vec<HookLine> {
        let plain = |title: String, note| HookLine {
            title,
            note,
            tag: None,
        };
        match self {
            HooksShown::Checking => vec![plain(CHECKING.into(), None)],
            HooksShown::Detached => vec![plain(DETACHED.into(), None)],
            HooksShown::Failed(e) => vec![plain(e.clone(), None)],
            HooksShown::Refused(why) => vec![plain(
                format!("Claude Code's settings.json was left alone: it {why}."),
                None,
            )],
            HooksShown::Empty => vec![plain(EMPTY.into(), Some(EMPTY_NOTE))],
            HooksShown::Found(lines) => lines.clone(),
        }
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

fn matcher(m: &str) -> String {
    match m.trim() {
        "" | "*" => "All tools".into(),
        other => cut(other),
    }
}

fn what(hook: &PresentHook) -> Option<&'static str> {
    match (hook.event.as_str(), hook.matcher.as_str()) {
        ("PermissionRequest", _) => Some("Asks for permission in the notch and Chat"),
        ("PreToolUse", "AskUserQuestion") => Some("Shows Claude's questions in the notch and Chat"),
        ("PreToolUse" | "PostToolUse" | "PostToolUseFailure", "Artifact") => {
            Some("Saves and finds published pages on this Mac")
        }
        _ => None,
    }
}

fn line(hook: &PresentHook) -> HookLine {
    HookLine {
        title: format!("{} · {}", cut(&hook.event), matcher(&hook.matcher)),
        note: Some(what(hook).unwrap_or("Runs Notchling's hook")),
        tag: if hook.swift {
            Some(SWIFT_TAG)
        } else if !hook.managed {
            Some(UNMANAGED_TAG)
        } else {
            None
        },
    }
}
