//! The vault approval prompt, pure (the Swift `VaultPrompt.approve`): an agent waits to use
//! secrets marked ask-first, and only the app can say yes. The words are the Swift panel's: the
//! names, the folder, the command, what Deny is for, and Always Allow / Deny / Allow Once.
//!
//! Each approval gets its own window, bound to that one approval for its whole life
//! ([`ApprovalModel`]): it never shows another request under live buttons. Allow buttons are armed
//! only [`ARMING_MS`] after the window appeared or became key, so a click or key meant for another
//! window can't hand out a secret. An answer resolves the prompt only once the daemon has taken it.
//! Closing the window is Deny. The daemon denies on its own after `APPROVAL_WAIT` (300 s).

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

/// The prompt window's size in points.
pub const PROMPT_SIZE: (f32, f32) = (500.0, 470.0);

/// How long after the window appears or becomes key before Allow accepts a click.
pub const ARMING_MS: i64 = 600;

/// How many lines the command box shows before it scrolls.
pub const COMMAND_LINES: usize = 6;
/// Roughly how many characters fit on one line of the command box.
const COMMAND_COLUMNS: usize = 62;

/// What the prompt draws.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Prompt {
    /// The id the answer names.
    pub id: String,
    /// "Allow an agent to use github, npm?"
    pub title: String,
    /// Where it runs: the daemon's verified working folder when it has one, else what it claims.
    pub folder: String,
    /// "It names /x", when the claimed folder differs from the verified one.
    pub claimed_folder: Option<String>,
    /// "Requested by zsh ← claude ← Terminal", from the daemon's verified process chain.
    pub requested_by: Option<String>,
    /// The command as claimed, with newlines and invisible characters made visible.
    pub command: String,
    /// "12 lines, 840 characters; scroll to read it all", when the box can't show it at once.
    pub command_size: Option<String>,
    pub explanation: &'static str,
    /// "Stop asking for github, npm; you can turn it back on in the vault".
    pub always_tip: String,
    /// The red line above the buttons when the folder it names isn't where it runs.
    pub warning: Option<&'static str>,
    /// Under the folder when the daemon couldn't check it against where the caller runs.
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

    /// An element id for this prompt's part, so a click can only ever reach this approval.
    pub fn element(&self, part: &str) -> String {
        format!("{part}-{}", self.id)
    }
}

/// Whether `c` would hide what the command does: control characters other than a newline or a
/// tab, bidirectional overrides and isolates, zero-width characters.
fn hidden(c: char) -> bool {
    (c.is_control() && c != '\n' && c != '\t')
        || matches!(c, '\u{200B}'..='\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}' | '\u{FEFF}')
}

/// The command with every newline shown as ⏎ (then a break) and every invisible character as its
/// code point, so what is shown is what runs.
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

/// "N lines, M characters; scroll to read it all" when the box clips the command.
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

/// One approval's window: bound to it, armed after a delay, resolved only when the daemon took
/// the answer. Times are milliseconds on any clock that only goes forward.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ApprovalModel {
    pub prompt: Prompt,
    /// When the window last appeared or became key; Allow waits [`ARMING_MS`] after it.
    shown_at: Option<i64>,
    /// The answer on its way to the daemon.
    sending: Option<Decision>,
    /// The daemon took an answer: the window goes.
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

    /// The window appeared or became key again: Allow waits once more.
    pub fn shown(&mut self, now_ms: i64) {
        self.shown_at = Some(now_ms);
    }

    /// The window is no longer key: nothing is armed until it is again.
    pub fn hidden(&mut self) {
        self.shown_at = None;
    }

    pub fn armed(&self, now_ms: i64) -> bool {
        self.shown_at
            .is_some_and(|at| now_ms.saturating_sub(at) >= ARMING_MS)
    }

    /// When the buttons arm, for a redraw then.
    pub fn arms_at(&self) -> Option<i64> {
        self.shown_at.map(|at| at + ARMING_MS)
    }

    pub fn busy(&self) -> bool {
        self.sending.is_some()
    }

    /// Whether `decision`'s button takes a click now. Deny never hands anything out, so it needs
    /// no arming; Allow Once and Always Allow do.
    pub fn live(&self, decision: Decision, now_ms: i64) -> bool {
        !self.resolved
            && self.sending.is_none()
            && (decision == Decision::Deny || self.armed(now_ms))
    }

    /// A click: the id and answer to send, or nothing when the button isn't live. The prompt
    /// stays until [`ApprovalModel::finished`] says the daemon took it.
    pub fn answer(&mut self, decision: Decision, now_ms: i64) -> Option<(String, Decision)> {
        if !self.live(decision, now_ms) {
            return None;
        }
        self.sending = Some(decision);
        self.note = None;
        Some((self.prompt.id.clone(), decision))
    }

    /// The daemon's answer to the answer: taken resolves the prompt; a failure says why and lets
    /// the person try again.
    pub fn finished(&mut self, result: Result<(), String>) {
        self.sending = None;
        match result {
            Ok(()) => self.resolved = true,
            Err(e) => self.note = Some(e),
        }
    }
}

/// The approvals still pending, out of the feed's events, oldest first.
pub fn pending(events: &[Value]) -> Vec<Approval> {
    events.iter().filter_map(Approval::from_event).collect()
}

#[cfg(test)]
#[path = "approval_tests.rs"]
mod tests;
