use super::menu::{self, Link, Menu};
use crate::reply::exec::{wait_attached, Pty};
use crate::reply::keys::{self, timing::*, DOWN, ENTER, ESC, UP};
use crate::terminal::input_box::{self, Input};
use std::time::Duration;

pub const COMMAND: &str = "/chrome";
const UNTYPE: &str = "\x7f\x7f\x7f\x7f\x7f\x7f\x7f";
const LIST_WAIT_MS: u64 = 3_000;
const MENU_WAIT_MS: u64 = 5_000;
const CLOSE_WAIT_MS: u64 = 3_000;
const RECONNECT_WAIT_MS: u64 = 25_000;
const MOVE_MAX: usize = 8;
const LIST_ROWS_ABOVE: usize = 14;
const SPINNERS: [char; 8] = ['✻', '✽', '✶', '✳', '✢', '·', '✦', '*'];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Goal {
    Read,
    Reconnect,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Reconnected {
    Skipped,
    Done,
    Failed(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Report {
    pub menu: Menu,
    pub reconnect: Reconnected,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refusal {
    Dialog,
    Working,
    BoxHasText,
    Starting,
    NoInput,
    Unavailable,
    NotListed,
    Stuck,
    Pty(String),
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Refusal::Dialog => write!(f, "The session is showing a question or menu."),
            Refusal::Working => write!(f, "The session is working."),
            Refusal::BoxHasText => write!(f, "The session's input box holds text."),
            Refusal::Starting => write!(f, "The session didn't open in time."),
            Refusal::NoInput => write!(f, "The session's terminal isn't at its prompt."),
            Refusal::Unavailable => write!(f, "This Claude Code has no /chrome command."),
            Refusal::NotListed => write!(f, "Couldn't open Claude Code's /chrome menu."),
            Refusal::Stuck => write!(f, "The terminal didn't show the Chrome menu."),
            Refusal::Pty(why) => write!(f, "Couldn't type into the session's terminal: {why}"),
        }
    }
}

impl std::error::Error for Refusal {}

fn ms(n: u64) -> Duration {
    Duration::from_millis(n)
}

pub fn working(lines: &[String]) -> bool {
    let Some((top, _)) = input_box::span(lines) else {
        return false;
    };
    let Some(line) = lines[..top].iter().rev().find(|l| !l.trim().is_empty()) else {
        return false;
    };
    let line = line.trim_start();
    let first = line.chars().next();
    first.is_some_and(|c| SPINNERS.contains(&c))
        && line.chars().take(48).any(|c| c == '…')
        && !line.contains("done ")
}

#[derive(Debug, PartialEq, Eq)]
enum Seen {
    Menu,
    Starting,
    Dialog,
    Working,
    Box(Input),
    Nothing,
}

fn seen(pty: &mut dyn Pty) -> Seen {
    let lines = pty.screen();
    let cursor = pty.cursor();
    if menu::is_open(&lines) {
        return Seen::Menu;
    }
    if input_box::starting(&lines, cursor) {
        return Seen::Starting;
    }
    if keys::dialog_open(&lines) {
        return Seen::Dialog;
    }
    match input_box::read(&lines, cursor) {
        Some(input) if input_box::focused(&lines, cursor) => {
            if working(&lines) {
                Seen::Working
            } else {
                Seen::Box(input)
            }
        }
        _ => Seen::Nothing,
    }
}

fn stands(pty: &mut dyn Pty, first: Seen) -> Seen {
    let mut last = first;
    for _ in 0..SETTLE_LOOKS {
        pty.sleep(ms(INPUT_POLL_MS));
        let again = seen(pty);
        if again == last {
            return again;
        }
        last = again;
    }
    last
}

fn settled(pty: &mut dyn Pty) -> Seen {
    let first = seen(pty);
    stands(pty, first)
}

fn write(pty: &mut dyn Pty, keys: &str) -> Result<(), Refusal> {
    pty.write(keys.as_bytes())
        .map_err(|e| Refusal::Pty(e.to_string()))
}

fn ready(pty: &mut dyn Pty) -> Result<(), Refusal> {
    if !wait_attached(
        pty,
        ms(ATTACH_QUIET_MS),
        ms(ATTACH_TIMEOUT_MS),
        ms(ATTACH_BUSY_MS),
    ) {
        return Err(Refusal::Starting);
    }
    let started = pty.now_ms();
    let mut deadline = started + INPUT_WAIT_MS;
    loop {
        match seen(pty) {
            Seen::Menu | Seen::Dialog => return Err(Refusal::Dialog),
            Seen::Starting => deadline = deadline.max(started + BOOT_WAIT_MS),
            Seen::Working => match stands(pty, Seen::Working) {
                Seen::Working => return Err(Refusal::Working),
                Seen::Box(input) if input.empty => return Ok(()),
                _ => {}
            },
            Seen::Box(input) if input.empty => match stands(pty, Seen::Box(input)) {
                Seen::Box(input) if input.empty => return Ok(()),
                Seen::Box(_) => return Err(Refusal::BoxHasText),
                Seen::Working => return Err(Refusal::Working),
                _ => {}
            },
            Seen::Box(input) => match stands(pty, Seen::Box(input)) {
                Seen::Box(input) if !input.empty => return Err(Refusal::BoxHasText),
                Seen::Working => return Err(Refusal::Working),
                _ => {}
            },
            Seen::Nothing => {}
        }
        if pty.now_ms() >= deadline {
            return Err(Refusal::NoInput);
        }
        pty.sleep(ms(INPUT_POLL_MS));
    }
}

struct Row {
    selected: bool,
    name: String,
    line: String,
}

fn command_row(line: &str) -> Option<Row> {
    let text = line.replace('\u{a0}', " ");
    let trimmed = text.trim_start();
    let selected = trimmed.starts_with('❯');
    let body = trimmed.trim_start_matches('❯').trim_start();
    if !body.starts_with('/') {
        return None;
    }
    let name = body.split_whitespace().next()?;
    body[name.len()..].starts_with("  ").then(|| Row {
        selected,
        name: name.to_string(),
        line: text.clone(),
    })
}

fn command_rows(lines: &[String]) -> Vec<Row> {
    let Some((top, _)) = input_box::span(lines) else {
        return Vec::new();
    };
    lines[top.saturating_sub(LIST_ROWS_ABOVE)..]
        .iter()
        .filter_map(|l| command_row(l))
        .collect()
}

fn lists_command(lines: &[String]) -> bool {
    command_rows(lines)
        .iter()
        .any(|r| r.selected && r.name == COMMAND && r.line.contains(menu::COMMAND_HINT))
}

fn list_without_command(lines: &[String]) -> bool {
    let rows = command_rows(lines);
    !rows.is_empty() && rows.iter().all(|r| r.name != COMMAND)
}

fn ours_only(pty: &mut dyn Pty) -> bool {
    let lines = pty.screen();
    input_box::read(&lines, pty.cursor()).is_some_and(|input| {
        input.held.len() == 1 && input.held[0].replace('\u{a0}', " ").trim() == "❯ /chrome"
    })
}

fn untype(pty: &mut dyn Pty) {
    pty.sleep(ms(KEY_PAUSE_MS));
    let _ = write(pty, ESC);
    pty.sleep(ms(KEY_PAUSE_MS));
    if ours_only(pty) {
        let _ = write(pty, UNTYPE);
        pty.wait_quiet(ms(WAIT_QUIET_MS), ms(WAIT_STEP_MS));
    }
}

fn clean_up(pty: &mut dyn Pty) {
    if menu::is_open(&pty.screen()) {
        let _ = close(pty);
    }
    if ours_only(pty) {
        untype(pty);
    }
}

fn type_command(pty: &mut dyn Pty) -> Result<(), Refusal> {
    write(pty, COMMAND)?;
    let deadline = pty.now_ms() + LIST_WAIT_MS;
    loop {
        pty.wait_quiet(ms(WAIT_QUIET_MS), ms(WAIT_STEP_MS));
        let lines = pty.screen();
        if lists_command(&lines) {
            return Ok(());
        }
        if pty.now_ms() >= deadline {
            break;
        }
    }
    if ours_only(pty) {
        return Err(if list_without_command(&pty.screen()) {
            Refusal::Unavailable
        } else {
            Refusal::NotListed
        });
    }
    Err(Refusal::BoxHasText)
}

fn open(pty: &mut dyn Pty) -> Result<Menu, Refusal> {
    if !ours_only(pty) {
        return Err(Refusal::BoxHasText);
    }
    write(pty, ENTER)?;
    let deadline = pty.now_ms() + MENU_WAIT_MS;
    loop {
        pty.wait_quiet(ms(WAIT_QUIET_MS), ms(WAIT_STEP_MS));
        let lines = pty.screen();
        if let Some(menu) = menu::read(&lines) {
            return Ok(menu);
        }
        if pty.now_ms() >= deadline {
            return Err(Refusal::Stuck);
        }
    }
}

fn close(pty: &mut dyn Pty) -> Result<(), Refusal> {
    write(pty, ESC)?;
    let start = pty.now_ms();
    let deadline = start + CLOSE_WAIT_MS;
    let mut again = false;
    loop {
        pty.wait_quiet(ms(WAIT_QUIET_MS), ms(WAIT_STEP_MS));
        let lines = pty.screen();
        if !menu::is_open(&lines) {
            return match settled(pty) {
                Seen::Box(input) if input.empty => Ok(()),
                Seen::Box(_) => Err(Refusal::BoxHasText),
                _ => Err(Refusal::Stuck),
            };
        }
        let now = pty.now_ms();
        if now >= deadline {
            return Err(Refusal::Stuck);
        }
        if !again && now >= start + CLOSE_WAIT_MS / 2 {
            write(pty, ESC)?;
            again = true;
        }
    }
}

fn move_to(pty: &mut dyn Pty, mut menu: Menu, label: &str) -> Result<Menu, Refusal> {
    let target = menu.index_of(label).ok_or(Refusal::Unavailable)?;
    for _ in 0..MOVE_MAX {
        if menu.selected == target {
            return Ok(menu);
        }
        write(pty, if target > menu.selected { DOWN } else { UP })?;
        pty.wait_quiet(ms(WAIT_QUIET_MS), ms(WAIT_STEP_MS));
        let lines = pty.screen();
        menu = menu::read(&lines).ok_or(Refusal::Stuck)?;
    }
    Err(Refusal::Stuck)
}

fn reconnect(pty: &mut dyn Pty, menu: Menu) -> Result<Reconnected, Refusal> {
    let menu = move_to(pty, menu, menu::RECONNECT)?;
    debug_assert_eq!(
        menu.options.get(menu.selected).map(String::as_str),
        Some(menu::RECONNECT)
    );
    write(pty, ENTER)?;
    let deadline = pty.now_ms() + RECONNECT_WAIT_MS;
    loop {
        pty.wait_quiet(ms(WAIT_QUIET_MS), ms(WAIT_STEP_MS));
        let lines = pty.screen();
        match menu::read(&lines) {
            None => {
                return match settled(pty) {
                    Seen::Box(input) if input.empty => Ok(Reconnected::Done),
                    Seen::Box(_) => Err(Refusal::BoxHasText),
                    _ => Err(Refusal::Stuck),
                };
            }
            Some(now) => {
                let waiting = now
                    .notice
                    .as_deref()
                    .is_none_or(|n| n.starts_with("Reconnecting"));
                if !waiting {
                    let note = now.notice.unwrap_or_default();
                    close(pty)?;
                    return Ok(Reconnected::Failed(note));
                }
            }
        }
        if pty.now_ms() >= deadline {
            close(pty)?;
            return Ok(Reconnected::Failed(String::new()));
        }
    }
}

pub fn run(pty: &mut dyn Pty, goal: Goal) -> Result<Report, Refusal> {
    ready(pty)?;
    let result = drive(pty, goal);
    if result.is_err() {
        clean_up(pty);
    }
    result
}

fn drive(pty: &mut dyn Pty, goal: Goal) -> Result<Report, Refusal> {
    type_command(pty)?;
    let menu = open(pty)?;
    let wants_reconnect = goal == Goal::Reconnect && matches!(menu.link, Link::NotConnected);
    if !wants_reconnect {
        close(pty)?;
        return Ok(Report {
            menu,
            reconnect: Reconnected::Skipped,
        });
    }
    let shown = menu.clone();
    let reconnect = reconnect(pty, menu)?;
    Ok(Report {
        menu: shown,
        reconnect,
    })
}
