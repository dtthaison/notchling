//! The bytes a reply is made of and the pauses between them, as the Swift app has them
//! (`ReplySender.swift`, `ReplySender+Dialog.swift`, `ConsoleCore/MessageSending.swift`). Each
//! value is pinned by a test against the Swift source; change them only with a new recording
//! (docs/recording.md).

/// Escape: interrupts a working turn. Never part of a reply.
pub const ESC: &str = "\x1b";
pub const ENTER: &str = "\r";
pub const TAB: &str = "\t";
pub const UP: &str = "\x1b[A";
pub const DOWN: &str = "\x1b[B";
pub const RIGHT: &str = "\x1b[C";
pub const LEFT: &str = "\x1b[D";
/// The review screen's "Submit answers" is its first option.
pub const SUBMIT: &str = "1";

/// A bracketed paste starts and ends with these.
pub const PASTE_START: &str = "\x1b[200~";
pub const PASTE_END: &str = "\x1b[201~";

/// Ctrl+E (end of line), Ctrl+U (delete to its start), Backspace (join the line above).
pub const CLEAR_LINE: &str = "\x05\x15\x7f";
/// How many lines the clear keys reach.
pub const CLEAR_LINES: usize = 20;

/// `ReplySender.clearInput`: empties the input box, which keeps a draft across attaches. Per line
/// Ctrl+E, Ctrl+U, Backspace, 20 times, then Ctrl+E Ctrl+U for the top line. Never Ctrl+K (in a
/// burst it lands as a literal vertical tab, which keeps Enter from submitting), never Esc or
/// Ctrl+C (they interrupt a busy session).
pub fn clear_input() -> String {
    let mut keys = CLEAR_LINE.repeat(CLEAR_LINES);
    keys.push_str("\x05\x15");
    keys
}

/// The text with what could end a paste taken out: `\r\n` becomes `\n`, and ESC and its one-byte
/// C1 form (CSI, U+009B) are dropped, so a pasted `ESC[201~` can't end the paste early and have the
/// rest typed as keys (Enter submits, digits answer a dialog, Esc interrupts). Other characters are
/// paste content, as in `TerminalKeys.bracketed`.
pub fn paste_safe(text: &str) -> String {
    text.replace("\r\n", "\n")
        .chars()
        .filter(|&c| c != '\x1b' && c != '\u{9b}')
        .collect()
}

/// `text` as one bracketed paste, so its newlines don't submit early.
pub fn bracketed(text: &str) -> String {
    format!("{PASTE_START}{}{PASTE_END}", paste_safe(text))
}

/// Words only a question or permission dialog shows (`ReplySender.dialogMarkers`, checked on
/// 2.1.318). A reply typed into one of those answers it. One list, the screen reader's (E3a).
pub use crate::terminal::ask_dialog::DIALOG_MARKERS;

/// How many drawn lines from the bottom the markers are looked for in.
pub const DIALOG_LINES: usize = 6;

/// `ReplySender.dialogOpen`: a marker in the last six drawn (not blank) lines. A short session
/// leaves blank rows under its prompt or dialog, so blank ones don't count.
pub fn dialog_open(lines: &[String]) -> bool {
    let drawn: Vec<&String> = lines.iter().filter(|l| !l.trim().is_empty()).collect();
    let start = drawn.len().saturating_sub(DIALOG_LINES);
    drawn[start..]
        .iter()
        .any(|line| DIALOG_MARKERS.iter().any(|m| line.contains(m)))
}

/// The pauses and waits, in milliseconds.
pub mod timing {
    /// The attach terminal's size (`ReplySender.attached`'s `winsize`).
    pub const ROWS: u16 = 50;
    pub const COLS: u16 = 200;

    /// After each chunk of keys (`ReplySender.type`, the answer's page keys, the Left walk back).
    pub const KEY_PAUSE_MS: u64 = 150;

    /// Before typing: the screen drew and then nothing came for this long...
    pub const ATTACH_QUIET_MS: u64 = 500;
    /// ...within this long, or the session didn't open (`Failure.timeout`).
    pub const ATTACH_TIMEOUT_MS: u64 = 15_000;
    /// A working session's screen keeps redrawing, so interrupt and take-back go ahead after this...
    pub const SETTLE_MS: u64 = 1_200;
    /// ...waiting at most the settle plus this.
    pub const SETTLE_SLACK_MS: u64 = 1_000;

    /// After the last key, before detaching.
    pub const DETACH_QUIET_MS: u64 = 400;
    pub const DETACH_TIMEOUT_MS: u64 = 3_000;

    /// Between the take-back's keys (`takeBackQueued`).
    pub const TAKE_BACK_PAUSE_MS: u64 = 300;
    /// How long a take-back looks for `popAll` in the transcript after Up...
    pub const POP_ALL_WAIT_MS: u64 = 3_000;
    /// ...reading the screen between looks for this quiet, or this long.
    pub const POP_ALL_QUIET_MS: u64 = 300;
    pub const POP_ALL_STEP_MS: u64 = 500;
    /// A `popAll` counts from this long before Up was pressed.
    pub const POP_ALL_SLACK_MS: u64 = 1_000;

    /// `ReplySender.wait`: read until quiet for this long, or this long, then look again.
    pub const WAIT_QUIET_MS: u64 = 300;
    pub const WAIT_STEP_MS: u64 = 900;
    /// After a page's keys, the next question (or the review screen) must show within this long.
    pub const PAGE_WAIT_MS: u64 = 4_000;
    /// After the review's `1`, the dialog should close within this long.
    pub const SUBMIT_WAIT_MS: u64 = 4_000;
    /// Each Left of the walk back to the first question waits for quiet this long, or this long.
    pub const WALK_BACK_QUIET_MS: u64 = 400;
    pub const WALK_BACK_TIMEOUT_MS: u64 = 1_500;
}
