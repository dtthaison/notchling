pub const ESC: &str = "\x1b";
pub const ENTER: &str = "\r";
pub const TAB: &str = "\t";
pub const UP: &str = "\x1b[A";
pub const DOWN: &str = "\x1b[B";
pub const RIGHT: &str = "\x1b[C";
pub const LEFT: &str = "\x1b[D";
pub const SUBMIT: &str = "1";

pub const PASTE_START: &str = "\x1b[200~";
pub const PASTE_END: &str = "\x1b[201~";

pub const CLEAR_LINE: &str = "\x05\x15\x7f";
pub const CLEAR_LINES: usize = 20;

pub fn clear_input() -> String {
    let mut keys = CLEAR_LINE.repeat(CLEAR_LINES);
    keys.push_str("\x05\x15");
    keys
}

pub fn paste_safe(text: &str) -> String {
    text.replace("\r\n", "\n")
        .chars()
        .filter(|&c| c != '\x1b' && c != '\u{9b}')
        .collect()
}

pub fn bracketed(text: &str) -> String {
    format!("{PASTE_START}{}{PASTE_END}", paste_safe(text))
}

pub use crate::terminal::ask_dialog::DIALOG_MARKERS;

pub const DIALOG_LINES: usize = 6;

pub fn dialog_open(lines: &[String]) -> bool {
    let drawn: Vec<&String> = lines.iter().filter(|l| !l.trim().is_empty()).collect();
    let start = drawn.len().saturating_sub(DIALOG_LINES);
    drawn[start..]
        .iter()
        .any(|line| DIALOG_MARKERS.iter().any(|m| line.contains(m)))
}

pub mod timing {
    pub const ROWS: u16 = 50;
    pub const COLS: u16 = 200;

    pub const KEY_PAUSE_MS: u64 = 150;

    pub const ATTACH_QUIET_MS: u64 = 500;
    pub const ATTACH_TIMEOUT_MS: u64 = 15_000;
    pub const SETTLE_MS: u64 = 1_200;
    pub const SETTLE_SLACK_MS: u64 = 1_000;

    pub const DETACH_QUIET_MS: u64 = 400;
    pub const DETACH_TIMEOUT_MS: u64 = 3_000;

    pub const TAKE_BACK_PAUSE_MS: u64 = 300;
    pub const POP_ALL_WAIT_MS: u64 = 3_000;
    pub const POP_ALL_QUIET_MS: u64 = 300;
    pub const POP_ALL_STEP_MS: u64 = 500;
    pub const POP_ALL_SLACK_MS: u64 = 1_000;

    pub const WAIT_QUIET_MS: u64 = 300;
    pub const WAIT_STEP_MS: u64 = 900;
    pub const PAGE_WAIT_MS: u64 = 4_000;
    pub const SUBMIT_WAIT_MS: u64 = 4_000;
    pub const PERMISSION_WAIT_MS: u64 = 4_000;
    pub const PERMISSION_DRAW_WAIT_MS: u64 = 3_000;
    pub const WALK_BACK_QUIET_MS: u64 = 400;
    pub const WALK_BACK_TIMEOUT_MS: u64 = 1_500;
}
