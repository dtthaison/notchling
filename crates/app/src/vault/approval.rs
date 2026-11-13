use serde_json::Value;

use super::wire::{Approval, Decision};

pub const WINDOW_TITLE: &str = "Use a Secret";
pub const EXPLANATION: &str = "The command gets the values as environment variables; its output comes back with them hidden. Deny it if the command would print, send or save a value somewhere you don't expect.";
pub const ALWAYS: &str = "Always Allow";
pub const DENY: &str = "Deny";
pub const ONCE: &str = "Allow Once";
pub const FOLDER: &str = "Folder";
pub const COMMAND: &str = "Command it says it will run";
pub const FOLDER_WARNING: &str = "The folder it names isn't where it's running";
pub const FOLDER_UNCHECKED: &str = "The folder couldn't be checked";

pub const PROMPT_SIZE: (f32, f32) = (500.0, 470.0);

pub const ARMING_MS: i64 = 600;

pub const COMMAND_LINES: usize = 6;
const COMMAND_COLUMNS: usize = 62;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Prompt {
    pub id: String,
    pub title: String,
    pub folder: String,
    pub claimed_folder: Option<String>,
    pub requested_by: Option<String>,
    pub command: String,
    pub command_size: Option<String>,
    pub explanation: &'static str,
    pub always_tip: String,
    pub warning: Option<&'static str>,
    pub unchecked: Option<&'static str>,
}

impl Prompt {
    pub fn new(a: &Approval) -> Prompt {
        let names = a.names.join(", ");
        let claimed = if a.folder.is_empty() {
            "unknown folder".to_owned()
        } else {
            a.folder.clone()
        };
        let verified = a.verified.as_ref();
        let real = verified
            .and_then(|v| v.cwd.clone())
            .filter(|c| !c.is_empty());
        let matches = verified.and_then(|v| v.folder_matches);
        let mismatch = matches == Some(false);
        Prompt {
            id: a.id.clone(),
            title: format!("Allow an agent to use {names}?"),
            folder: real.clone().unwrap_or_else(|| claimed.clone()),
            claimed_folder: (mismatch || real.as_deref().is_some_and(|r| r != claimed))
                .then(|| format!("It names {claimed}")),
            requested_by: verified
                .filter(|v| !v.parents.is_empty())
                .map(|v| format!("Requested by {}", v.parents.join(" ← "))),
            command: visible(&a.command),
            command_size: command_size(&a.command),
            explanation: EXPLANATION,
            always_tip: format!("Stop asking for {names}; you can turn it back on in the vault"),
            warning: mismatch.then_some(FOLDER_WARNING),
            unchecked: matches.is_none().then_some(FOLDER_UNCHECKED),
        }
    }

    pub fn element(&self, part: &str) -> String {
        format!("{part}-{}", self.id)
    }
}

fn hidden(c: char) -> bool {
    (c.is_control() && c != '\n' && c != '\t')
        || matches!(c, '\u{200B}'..='\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}' | '\u{FEFF}')
}

pub fn visible(command: &str) -> String {
    let mut out = String::with_capacity(command.len());
    for c in command.chars() {
        match c {
            '\n' => out.push_str("⏎\n"),
            c if hidden(c) => out.push_str(&format!("⟨U+{:04X}⟩", c as u32)),
            c => out.push(c),
        }
    }
    out
}

pub fn command_size(command: &str) -> Option<String> {
    let lines = command.split('\n').count();
    let rows: usize = command
        .split('\n')
        .map(|l| l.chars().count().div_ceil(COMMAND_COLUMNS).max(1))
        .sum();
    let chars = command.chars().count();
    (rows > COMMAND_LINES).then(|| {
        let l = if lines == 1 { "line" } else { "lines" };
        format!("{lines} {l}, {chars} characters; scroll to read it all")
    })
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ApprovalModel {
    pub prompt: Prompt,
    shown_at: Option<i64>,
    sending: Option<Decision>,
    pub resolved: bool,
    pub note: Option<String>,
}

impl ApprovalModel {
    pub fn new(approval: &Approval) -> ApprovalModel {
        ApprovalModel {
            prompt: Prompt::new(approval),
            shown_at: None,
            sending: None,
            resolved: false,
            note: None,
        }
    }

    pub fn shown(&mut self, now_ms: i64) {
        self.shown_at = Some(now_ms);
    }

    pub fn hidden(&mut self) {
        self.shown_at = None;
    }

    pub fn armed(&self, now_ms: i64) -> bool {
        self.shown_at
            .is_some_and(|at| now_ms.saturating_sub(at) >= ARMING_MS)
    }

    pub fn arms_at(&self) -> Option<i64> {
        self.shown_at.map(|at| at + ARMING_MS)
    }

    pub fn busy(&self) -> bool {
        self.sending.is_some()
    }

    pub fn live(&self, decision: Decision, now_ms: i64) -> bool {
        !self.resolved
            && self.sending.is_none()
            && (decision == Decision::Deny || self.armed(now_ms))
    }

    pub fn answer(&mut self, decision: Decision, now_ms: i64) -> Option<(String, Decision)> {
        if !self.live(decision, now_ms) {
            return None;
        }
        self.sending = Some(decision);
        self.note = None;
        Some((self.prompt.id.clone(), decision))
    }

    pub fn finished(&mut self, result: Result<(), String>) {
        self.sending = None;
        match result {
            Ok(()) => self.resolved = true,
            Err(e) => self.note = Some(e),
        }
    }
}

pub fn pending(events: &[Value]) -> Vec<Approval> {
    events.iter().filter_map(Approval::from_event).collect()
}

#[cfg(test)]
#[path = "approval_tests.rs"]
mod tests;
