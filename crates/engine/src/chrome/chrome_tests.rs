use super::command::{self, Command};
use super::drive::{self, Goal, Reconnected, Refusal};
use super::flow::{self, Done, Host, Need, Options, Phase, Step, Wait};
use super::menu::{self, Extension, Link};
use super::status::{self, Basis};
use crate::reply::exec::Pty;
use crate::sessions::testutil::TempDir;
use std::collections::VecDeque;
use std::time::Duration;

const MENU_ON: &str = include_str!("../../tests/fixtures/chrome/menu-on.txt");
const MENU_OFF: &str = include_str!("../../tests/fixtures/chrome/menu-off.txt");
const MENU_OFF_RECONNECT: &str = include_str!("../../tests/fixtures/chrome/menu-off-reconnect.txt");
const OFF_ATTACH: &str = include_str!("../../tests/fixtures/chrome/off-attach.txt");
const ON_ATTACH: &str = include_str!("../../tests/fixtures/chrome/on-attach.txt");
const TYPED: &str = include_str!("../../tests/fixtures/chrome/typed-slash.txt");
const CLOSED: &str = include_str!("../../tests/fixtures/chrome/closed-box-empty.txt");
const BOX_TEXT: &str = include_str!("../../tests/fixtures/input-box/typed.txt");
const WORKING: &str = include_str!("../../tests/fixtures/input-box/working-after-send.txt");
const PERMISSION: &str = include_str!("../../tests/fixtures/permission-made-up/bash.txt");

struct Shot {
    lines: Vec<String>,
    cursor: Option<(usize, usize)>,
}

fn shot(text: &str) -> Shot {
    if !text.starts_with("cursor ") {
        return Shot {
            lines: text.lines().map(String::from).collect(),
            cursor: None,
        };
    }
    let mut rows = text.lines();
    let head: Vec<&str> = rows.next().unwrap().split(' ').collect();
    let cursor = (head[1].parse().unwrap(), head[2].parse().unwrap());
    Shot {
        lines: rows.map(String::from).collect(),
        cursor: (head[3] == "true").then_some(cursor),
    }
}

struct Fake {
    state: usize,
    shots: Vec<Shot>,
    next: fn(usize, &str) -> usize,
    writes: Vec<String>,
    clock: u64,
}

impl Fake {
    fn new(shots: &[&str], next: fn(usize, &str) -> usize) -> Fake {
        Fake {
            state: 0,
            shots: shots.iter().map(|t| shot(t)).collect(),
            next,
            writes: Vec::new(),
            clock: 0,
        }
    }
}

impl Pty for Fake {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<()> {
        let text = String::from_utf8_lossy(bytes).into_owned();
        self.state = (self.next)(self.state, &text);
        self.writes.push(text);
        Ok(())
    }
    fn wait_quiet(&mut self, _quiet: Duration, timeout: Duration) -> bool {
        self.clock += u64::try_from(timeout.as_millis()).unwrap_or(0).min(300);
        true
    }
    fn screen(&mut self) -> Vec<String> {
        self.shots[self.state].lines.clone()
    }
    fn cursor(&mut self) -> Option<(usize, usize)> {
        self.shots[self.state].cursor
    }
    fn sleep(&mut self, duration: Duration) {
        self.clock += u64::try_from(duration.as_millis()).unwrap_or(0);
    }
    fn now_ms(&self) -> u64 {
        self.clock
    }
}

fn chrome_walk(state: usize, write: &str) -> usize {
    match (state, write) {
        (0, "/chrome") => 1,
        (1, "\r") => 2,
        (2, "\x1b") => 3,
        (s, _) => s,
    }
}

fn menu_lines(text: &str) -> Vec<String> {
    shot(text).lines
}

#[test]
fn the_recorded_menus_read_the_same_way() {
    let on = menu::read(&menu_lines(MENU_ON)).unwrap();
    assert_eq!(on.link, Link::Enabled);
    assert_eq!(on.extension, Extension::Installed);
    assert_eq!(on.browser.as_deref(), Some("Demo MacBook"));
    assert_eq!(on.by_default, Some(false));
    assert_eq!(
        on.options,
        [
            "Choose browser",
            "Manage permissions",
            "Reconnect extension",
            "Turn on by default"
        ]
    );
    assert_eq!(on.selected, 0);
    assert_eq!(on.notice, None);
    let off = menu::read(&menu_lines(MENU_OFF)).unwrap();
    assert_eq!(off.link, Link::NotEnabled);
    assert_eq!(off.browser, None);
    assert_eq!(off.options.len(), 3);
    assert_eq!(off.index_of(menu::RECONNECT), Some(1));
    let refused = menu::read(&menu_lines(MENU_OFF_RECONNECT)).unwrap();
    assert_eq!(refused.link, Link::NotEnabled);
    assert!(refused
        .notice
        .unwrap()
        .starts_with("Claude in Chrome is off in this session, so reconnecting can't add"));
}

#[test]
fn a_menu_is_only_read_where_it_is_open() {
    assert!(menu::read(&menu_lines(CLOSED)).is_none());
    assert!(menu::read(&menu_lines(TYPED)).is_none());
    assert!(menu::read(&menu_lines(OFF_ATTACH)).is_none());
}

#[test]
fn the_other_statuses_come_from_claude_codes_own_words() {
    let made_up = |from: &str, to: &str| menu::read(&menu_lines(&MENU_ON.replace(from, to)));
    assert_eq!(
        made_up("✔ enabled", "⚠ not connected").unwrap().link,
        Link::NotConnected
    );
    assert_eq!(
        made_up("✔ enabled", "⠋ connecting…").unwrap().link,
        Link::Connecting
    );
    assert_eq!(
        made_up("✔ enabled", "◯ turned off in /mcp").unwrap().link,
        Link::TurnedOffInMcp
    );
    assert_eq!(
        made_up("✔ installed", "⚠ not detected").unwrap().extension,
        Extension::NotDetected
    );
    assert_eq!(
        made_up("✔ enabled", "⚠ something new").unwrap().link,
        Link::Other("something new".to_string())
    );
}

#[test]
fn reading_the_menu_types_the_command_opens_it_and_closes_it_again() {
    let mut pty = Fake::new(&[ON_ATTACH, TYPED, MENU_ON, CLOSED], chrome_walk);
    let report = drive::run(&mut pty, Goal::Read).unwrap();
    assert_eq!(report.menu.link, Link::Enabled);
    assert_eq!(report.reconnect, Reconnected::Skipped);
    assert_eq!(pty.writes, ["/chrome", "\r", "\x1b"]);
    let mut off = Fake::new(&[OFF_ATTACH, TYPED, MENU_OFF, CLOSED], chrome_walk);
    let report = drive::run(&mut off, Goal::Read).unwrap();
    assert_eq!(report.menu.link, Link::NotEnabled);
    assert_eq!(off.writes, ["/chrome", "\r", "\x1b"]);
}

#[test]
fn nothing_is_typed_when_the_box_holds_text() {
    let mut pty = Fake::new(&[BOX_TEXT], chrome_walk);
    assert_eq!(drive::run(&mut pty, Goal::Read), Err(Refusal::BoxHasText));
    assert!(pty.writes.is_empty());
}

#[test]
fn nothing_is_typed_while_a_turn_is_running() {
    let mut pty = Fake::new(&[WORKING], chrome_walk);
    assert_eq!(drive::run(&mut pty, Goal::Read), Err(Refusal::Working));
    assert!(pty.writes.is_empty());
}

#[test]
fn nothing_is_typed_over_a_dialog_or_an_open_menu() {
    let mut dialog = Fake::new(&[PERMISSION], chrome_walk);
    assert_eq!(drive::run(&mut dialog, Goal::Read), Err(Refusal::Dialog));
    assert!(dialog.writes.is_empty());
    let mut open = Fake::new(&[MENU_ON], chrome_walk);
    assert_eq!(drive::run(&mut open, Goal::Read), Err(Refusal::Dialog));
    assert!(open.writes.is_empty());
}

#[test]
fn a_claude_code_without_the_command_leaves_nothing_typed() {
    let without = TYPED.replace(menu::COMMAND_HINT, "Something else");
    let mut pty = Fake::new(&[OFF_ATTACH, &without], |state, write| {
        match (state, write) {
            (0, "/chrome") => 1,
            (s, _) => s,
        }
    });
    assert_eq!(drive::run(&mut pty, Goal::Read), Err(Refusal::Unavailable));
    assert_eq!(pty.writes[0], "/chrome");
    assert!(pty.writes.iter().any(|w| w.starts_with('\x7f')));
    assert!(!pty.writes.iter().any(|w| w == "\r"));
}

#[test]
fn reconnecting_moves_to_the_menu_entry_and_presses_enter_there() {
    let not_connected = MENU_ON.replace("✔ enabled", "⚠ not connected");
    let second = not_connected.replace(
        "  ❯ Choose browser\n    Manage permissions",
        "    Choose browser\n  ❯ Manage permissions",
    );
    let third = second.replace(
        "  ❯ Manage permissions\n    Reconnect extension",
        "    Manage permissions\n  ❯ Reconnect extension",
    );
    let shots = [ON_ATTACH, TYPED, &not_connected, &second, &third, CLOSED];
    let mut pty = Fake::new(&shots, |state, write| match (state, write) {
        (0, "/chrome") => 1,
        (1, "\r") => 2,
        (2, "\x1b[B") => 3,
        (3, "\x1b[B") => 4,
        (4, "\r") => 5,
        (s, _) => s,
    });
    let report = drive::run(&mut pty, Goal::Reconnect).unwrap();
    assert_eq!(report.reconnect, Reconnected::Done);
    assert_eq!(pty.writes, ["/chrome", "\r", "\x1b[B", "\x1b[B", "\r"]);
}

#[test]
fn a_failed_reconnect_hands_back_claude_codes_note_and_closes_the_menu() {
    let not_connected = MENU_ON.replace("✔ enabled", "⚠ not connected");
    let at = not_connected.replace(
        "  ❯ Choose browser\n    Manage permissions\n    Reconnect extension",
        "    Choose browser\n    Manage permissions\n  ❯ Reconnect extension",
    );
    let failed = at.replace(
        "    Turn on by default\n",
        "    Turn on by default\n\n  ⚠ Chrome isn't answering. Select \"Reconnect extension\" to try again.\n",
    );
    let shots = [ON_ATTACH, TYPED, &at, &failed, CLOSED];
    let mut pty = Fake::new(&shots, |state, write| match (state, write) {
        (0, "/chrome") => 1,
        (1, "\r") => 2,
        (2, "\r") => 3,
        (3, "\x1b") => 4,
        (s, _) => s,
    });
    let report = drive::run(&mut pty, Goal::Reconnect).unwrap();
    assert_eq!(
        report.reconnect,
        Reconnected::Failed(
            "Chrome isn't answering. Select \"Reconnect extension\" to try again.".to_string()
        )
    );
    assert_eq!(pty.writes.last().map(String::as_str), Some("\x1b"));
}

#[test]
fn a_connected_session_is_not_reconnected() {
    let mut pty = Fake::new(&[ON_ATTACH, TYPED, MENU_ON, CLOSED], chrome_walk);
    let report = drive::run(&mut pty, Goal::Reconnect).unwrap();
    assert_eq!(report.reconnect, Reconnected::Skipped);
    assert_eq!(pty.writes, ["/chrome", "\r", "\x1b"]);
}

#[test]
fn chrome_is_caught_with_or_without_words_after_it() {
    assert_eq!(command::parse("/chrome"), Some(Command::On));
    assert_eq!(command::parse("  /Chrome  "), Some(Command::On));
    assert_eq!(command::parse("/chrome now please"), None);
    assert_eq!(command::parse("/chrome off"), Some(Command::Off));
    assert_eq!(command::parse("/chrome OFF"), Some(Command::Off));
    assert_eq!(command::parse("/chrome-browser"), None);
    assert_eq!(command::parse("chrome"), None);
    assert_eq!(command::parse("please /chrome"), None);
    assert_eq!(command::parse("/model"), None);
}

#[test]
fn the_flag_is_read_and_set_the_way_claude_code_restarts_with_it() {
    let flags = |list: &[&str]| list.iter().map(|f| f.to_string()).collect::<Vec<_>>();
    assert_eq!(status::flag_of(&flags(&["--name", "x"])), None);
    assert_eq!(
        status::flag_of(&flags(&["--chrome", "--no-chrome"])),
        Some(false)
    );
    assert_eq!(
        status::with_flag(
            &flags(&["--name", "x", "--no-chrome", "--model", "m"]),
            true
        ),
        flags(&["--name", "x", "--model", "m", "--chrome"])
    );
    assert_eq!(
        status::with_flag(&flags(&["--chrome"]), false),
        flags(&["--no-chrome"])
    );
}

#[test]
fn what_is_known_without_asking_claude_code() {
    let flags = |list: &[&str]| list.iter().map(|f| f.to_string()).collect::<Vec<_>>();
    let on = flags(&["--chrome"]);
    let plain = flags(&["--name", "x"]);
    let found = status::detect(Some(&on), Some(false));
    assert_eq!((found.on, found.basis), (Some(true), Basis::Flag));
    let found = status::detect(Some(&plain), Some(true));
    assert_eq!((found.on, found.basis), (Some(true), Basis::Default));
    let found = status::detect(Some(&plain), None);
    assert_eq!((found.on, found.basis), (None, Basis::Unknown));
    let found = status::detect(None, Some(true));
    assert_eq!((found.on, found.basis), (None, Basis::Unknown));
}

#[test]
fn the_default_comes_from_the_root_it_is_given() {
    let root = TempDir::new("chrome-default");
    let file = root.write(".claude.json", r#"{"claudeInChromeDefaultEnabled": true}"#);
    assert_eq!(status::default_enabled(&file), Some(true));
    let none = root.write("other.json", r#"{"theme": "dark"}"#);
    assert_eq!(status::default_enabled(&none), None);
    assert_eq!(status::default_enabled(&root.path("missing.json")), None);
}

#[test]
fn the_session_file_keeps_everything_but_the_flag() {
    let root = TempDir::new("chrome-state");
    let file = root.write(
        "state.json",
        r#"{"state":"done","inFlight":{"tasks":2},"respawnFlags":["--name","demo","--no-chrome","--model","m"],"keep":[1,2]}"#,
    );
    assert_eq!(status::tasks_in_flight(&file), Some(2));
    status::rewrite_flags(&file, true).unwrap();
    let after: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
    assert_eq!(
        after["respawnFlags"],
        serde_json::json!(["--name", "demo", "--model", "m", "--chrome"])
    );
    assert_eq!(after["keep"], serde_json::json!([1, 2]));
    assert_eq!(after["state"], "done");
    assert!(!root.path("state.json.notchling-tmp").exists());
    assert_eq!(
        status::read_flags(&file)
            .unwrap()
            .last()
            .map(String::as_str),
        Some("--chrome")
    );
    let bare = root.write("bare.json", r#"{"state":"done"}"#);
    assert_eq!(
        status::rewrite_flags(&bare, true),
        Err(status::RewriteError::NoFlags)
    );
}

struct Script {
    turns: VecDeque<bool>,
    looks: VecDeque<Result<menu::Menu, Refusal>>,
    tasks: Option<u32>,
    sleeps: Vec<u64>,
    restarts: Vec<bool>,
    restart_error: Option<String>,
    goals: Vec<Goal>,
    clock: u64,
    cancel: bool,
    cancel_after_ms: Option<u64>,
}

fn menu_with(link: Link, extension: Extension) -> menu::Menu {
    let mut menu = menu::read(&menu_lines(MENU_ON)).unwrap();
    menu.link = link;
    menu.extension = extension;
    menu
}

fn script(looks: Vec<Result<menu::Menu, Refusal>>) -> Script {
    Script {
        turns: VecDeque::new(),
        looks: looks.into(),
        tasks: Some(0),
        sleeps: Vec::new(),
        restarts: Vec::new(),
        restart_error: None,
        goals: Vec::new(),
        clock: 0,
        cancel: false,
        cancel_after_ms: None,
    }
}

impl Host for Script {
    fn turn_running(&mut self) -> bool {
        self.turns.pop_front().unwrap_or(false)
    }
    fn tasks_in_flight(&mut self) -> Option<u32> {
        self.tasks
    }
    fn look(&mut self, goal: Goal) -> Result<drive::Report, Refusal> {
        self.goals.push(goal);
        let menu = self.looks.pop_front().expect("a look was scripted")?;
        let failed = goal == Goal::Reconnect && menu.link == Link::NotConnected;
        Ok(drive::Report {
            menu,
            reconnect: match (goal, failed) {
                (Goal::Reconnect, true) => Reconnected::Failed("no answer".to_string()),
                (Goal::Reconnect, false) => Reconnected::Done,
                _ => Reconnected::Skipped,
            },
        })
    }
    fn restart(&mut self, on: bool) -> Result<(), String> {
        self.restarts.push(on);
        match self.restart_error.clone() {
            Some(why) => Err(why),
            None => Ok(()),
        }
    }
    fn sleep(&mut self, ms: u64) {
        self.sleeps.push(ms);
        self.clock += ms;
    }
    fn now_ms(&mut self) -> u64 {
        self.clock
    }
    fn cancelled(&mut self) -> bool {
        self.cancel || self.cancel_after_ms.is_some_and(|t| self.clock >= t)
    }
}

fn go(host: &mut Script, on: bool, force: bool) -> (Phase, Vec<Phase>) {
    let mut seen = Vec::new();
    let end = flow::run(host, Options { on, force }, &mut |p| seen.push(p));
    (end, seen)
}

#[test]
fn a_session_with_chrome_already_on_says_so_and_does_nothing_else() {
    let mut host = script(vec![Ok(menu_with(Link::Enabled, Extension::Installed))]);
    let (end, _) = go(&mut host, true, false);
    assert_eq!(
        end,
        Phase::Done {
            result: Done::AlreadyOn
        }
    );
    assert!(host.restarts.is_empty());
    assert_eq!(host.goals, [Goal::Read]);
}

#[test]
fn a_session_without_chrome_is_restarted_with_it_and_needs_no_sheet() {
    let mut host = script(vec![
        Ok(menu_with(Link::NotEnabled, Extension::Installed)),
        Ok(menu_with(Link::Connecting, Extension::Installed)),
        Ok(menu_with(Link::Enabled, Extension::Installed)),
    ]);
    let (end, seen) = go(&mut host, true, false);
    assert_eq!(end, Phase::Done { result: Done::On });
    assert_eq!(host.restarts, [true]);
    assert!(seen.contains(&Phase::Working {
        step: Step::Restarting
    }));
    assert!(!seen.iter().any(|p| matches!(p, Phase::NeedsYou { .. })));
}

#[test]
fn turning_chrome_off_restarts_a_session_that_has_it() {
    let mut host = script(vec![
        Ok(menu_with(Link::Enabled, Extension::Installed)),
        Ok(menu_with(Link::NotEnabled, Extension::Installed)),
    ]);
    let (end, _) = go(&mut host, false, false);
    assert_eq!(end, Phase::Done { result: Done::Off });
    assert_eq!(host.restarts, [false]);
    let mut already = script(vec![Ok(menu_with(Link::NotEnabled, Extension::Installed))]);
    let (end, _) = go(&mut already, false, false);
    assert_eq!(
        end,
        Phase::Done {
            result: Done::AlreadyOff
        }
    );
    assert!(already.restarts.is_empty());
}

#[test]
fn a_running_turn_is_waited_out_before_anything_is_typed_or_restarted() {
    let mut host = script(vec![
        Ok(menu_with(Link::NotEnabled, Extension::Installed)),
        Ok(menu_with(Link::Enabled, Extension::Installed)),
    ]);
    host.turns = VecDeque::from([true, true, false]);
    let (end, seen) = go(&mut host, true, false);
    assert_eq!(end, Phase::Done { result: Done::On });
    let waits = seen
        .iter()
        .filter(|p| **p == Phase::Waiting { why: Wait::Turn })
        .count();
    assert_eq!(waits, 2);
    assert_eq!(host.goals.len(), 2);
}

#[test]
fn text_in_the_box_or_an_open_question_is_waited_out_too() {
    let mut host = script(vec![
        Err(Refusal::BoxHasText),
        Err(Refusal::Dialog),
        Err(Refusal::Working),
        Ok(menu_with(Link::Enabled, Extension::Installed)),
    ]);
    let (end, seen) = go(&mut host, true, false);
    assert_eq!(
        end,
        Phase::Done {
            result: Done::AlreadyOn
        }
    );
    assert!(seen.contains(&Phase::Waiting { why: Wait::Box }));
    assert!(seen.contains(&Phase::Waiting {
        why: Wait::Question
    }));
    assert!(seen.contains(&Phase::Waiting { why: Wait::Turn }));
    assert!(host.restarts.is_empty());
}

#[test]
fn a_box_that_never_empties_ends_as_something_for_the_person() {
    let looks = (0..400).map(|_| Err(Refusal::BoxHasText)).collect();
    let mut host = script(looks);
    let (end, _) = go(&mut host, true, false);
    assert_eq!(
        end,
        Phase::NeedsYou {
            need: Need::Busy { why: Wait::Box }
        }
    );
    assert!(host.restarts.is_empty());
}

#[test]
fn an_extension_that_is_not_connected_is_tried_once_then_handed_to_the_person() {
    let mut host = script(vec![
        Ok(menu_with(Link::NotConnected, Extension::Installed)),
        Ok(menu_with(Link::NotConnected, Extension::Installed)),
    ]);
    let (end, _) = go(&mut host, true, false);
    assert_eq!(
        end,
        Phase::NeedsYou {
            need: Need::NotConnected
        }
    );
    assert_eq!(host.goals, [Goal::Read, Goal::Reconnect]);
    assert!(host.restarts.is_empty());
}

#[test]
fn a_reconnect_that_works_ends_on() {
    let mut host = script(vec![
        Ok(menu_with(Link::NotConnected, Extension::Installed)),
        Ok(menu_with(Link::Enabled, Extension::Installed)),
        Ok(menu_with(Link::Enabled, Extension::Installed)),
    ]);
    let (end, _) = go(&mut host, true, false);
    assert_eq!(end, Phase::Done { result: Done::On });
}

#[test]
fn a_missing_extension_is_named_as_missing() {
    let looks = vec![
        Ok(menu_with(Link::NotEnabled, Extension::NotDetected)),
        Ok(menu_with(Link::NotConnected, Extension::NotDetected)),
        Ok(menu_with(Link::NotConnected, Extension::NotDetected)),
        Ok(menu_with(Link::NotConnected, Extension::NotDetected)),
        Ok(menu_with(Link::NotConnected, Extension::NotDetected)),
        Ok(menu_with(Link::NotConnected, Extension::NotDetected)),
        Ok(menu_with(Link::NotConnected, Extension::NotDetected)),
    ];
    let mut host = script(looks);
    let (end, _) = go(&mut host, true, false);
    assert_eq!(
        end,
        Phase::NeedsYou {
            need: Need::NoExtension
        }
    );
}

#[test]
fn chrome_turned_off_in_mcp_is_for_the_terminal() {
    let mut host = script(vec![Ok(menu_with(
        Link::TurnedOffInMcp,
        Extension::Installed,
    ))]);
    let (end, _) = go(&mut host, true, false);
    assert_eq!(
        end,
        Phase::NeedsYou {
            need: Need::TurnedOffInMcp
        }
    );
    assert!(host.restarts.is_empty());
}

#[test]
fn background_tasks_stop_the_restart_until_the_person_agrees() {
    let mut host = script(vec![Ok(menu_with(Link::NotEnabled, Extension::Installed))]);
    host.tasks = Some(3);
    let (end, _) = go(&mut host, true, false);
    assert_eq!(
        end,
        Phase::NeedsYou {
            need: Need::Tasks { count: 3 }
        }
    );
    assert!(host.restarts.is_empty());
    let mut agreed = script(vec![
        Ok(menu_with(Link::NotEnabled, Extension::Installed)),
        Ok(menu_with(Link::Enabled, Extension::Installed)),
    ]);
    agreed.tasks = Some(3);
    let (end, _) = go(&mut agreed, true, true);
    assert_eq!(end, Phase::Done { result: Done::On });
    assert_eq!(agreed.restarts, [true]);
}

#[test]
fn a_restart_that_fails_is_a_failure_with_its_words() {
    let mut host = script(vec![Ok(menu_with(Link::NotEnabled, Extension::Installed))]);
    host.restart_error = Some("claude respawn failed".to_string());
    let (end, _) = go(&mut host, true, false);
    assert_eq!(
        end,
        Phase::NeedsYou {
            need: Need::Failed {
                text: "claude respawn failed".to_string()
            }
        }
    );
}

#[test]
fn a_claude_code_without_chrome_commands_ends_as_unavailable() {
    let mut host = script(vec![Err(Refusal::Unavailable)]);
    let (end, _) = go(&mut host, true, false);
    assert_eq!(
        end,
        Phase::NeedsYou {
            need: Need::Unavailable
        }
    );
}

#[test]
fn a_session_that_started_without_the_flag_after_a_restart_is_a_failure() {
    let mut host = script(vec![
        Ok(menu_with(Link::NotEnabled, Extension::Installed)),
        Ok(menu_with(Link::NotEnabled, Extension::Installed)),
    ]);
    let (end, _) = go(&mut host, true, false);
    assert!(matches!(
        end,
        Phase::NeedsYou {
            need: Need::Failed { .. }
        }
    ));
}

#[test]
fn the_phase_goes_over_the_wire_in_plain_words() {
    let json = serde_json::to_value(Phase::NeedsYou {
        need: Need::Tasks { count: 2 },
    })
    .unwrap();
    assert_eq!(
        json,
        serde_json::json!({"phase": "needs_you", "need": {"need": "tasks", "count": 2}})
    );
    let json = serde_json::to_value(Phase::Waiting { why: Wait::Turn }).unwrap();
    assert_eq!(json, serde_json::json!({"phase": "waiting", "why": "turn"}));
    assert!(Phase::Done { result: Done::On }.is_over());
    assert!(!Phase::Waiting { why: Wait::Box }.is_over());
}

#[test]
fn a_menu_is_closed_when_the_reconnect_entry_is_missing() {
    let not_connected = MENU_ON
        .replace("✔ enabled", "⚠ not connected")
        .replace("    Reconnect extension\n", "");
    let shots = [ON_ATTACH, TYPED, &not_connected, CLOSED];
    let mut pty = Fake::new(&shots, chrome_walk);
    assert_eq!(
        drive::run(&mut pty, Goal::Reconnect),
        Err(Refusal::Unavailable)
    );
    assert_eq!(pty.writes, ["/chrome", "\r", "\x1b"]);
    assert_eq!(pty.state, 3);
}

#[test]
fn a_menu_is_closed_when_moving_to_the_entry_never_gets_there() {
    let not_connected = MENU_ON.replace("✔ enabled", "⚠ not connected");
    let shots = [ON_ATTACH, TYPED, &not_connected, CLOSED];
    let mut pty = Fake::new(&shots, |state, write| match (state, write) {
        (0, "/chrome") => 1,
        (1, "\r") => 2,
        (2, "\x1b") => 3,
        (s, _) => s,
    });
    assert_eq!(drive::run(&mut pty, Goal::Reconnect), Err(Refusal::Stuck));
    assert_eq!(pty.writes.last().map(String::as_str), Some("\x1b"));
}

#[test]
fn enter_is_never_pressed_when_the_box_holds_more_than_the_command() {
    let mixed = TYPED.replacen("/chrome\n", "hello /chrome\n", 1);
    let mut pty = Fake::new(&[ON_ATTACH, &mixed], |state, write| match (state, write) {
        (0, "/chrome") => 1,
        (s, _) => s,
    });
    assert_eq!(drive::run(&mut pty, Goal::Read), Err(Refusal::BoxHasText));
    assert!(!pty.writes.iter().any(|w| w == "\r"));
    assert!(!pty.writes.iter().any(|w| w.starts_with('\x7f')));
}

#[test]
fn a_line_above_the_box_does_not_count_as_the_commands_list() {
    let lines: Vec<&str> = TYPED.lines().collect();
    let rule = lines.iter().position(|l| l.starts_with('─')).unwrap();
    let mut above: Vec<String> = lines.iter().map(|l| l.to_string()).collect();
    above.truncate(rule + 3);
    above[2] = "❯ ".to_string() + menu::COMMAND_HINT;
    let screen = above.join("\n");
    let mut pty = Fake::new(&[ON_ATTACH, &screen], |state, write| match (state, write) {
        (0, "/chrome") => 1,
        (s, _) => s,
    });
    assert_eq!(drive::run(&mut pty, Goal::Read), Err(Refusal::Unavailable));
    assert!(!pty.writes.iter().any(|w| w == "\r"));
}

#[test]
fn a_cancelled_task_stops_before_it_does_anything() {
    let mut host = script(vec![Ok(menu_with(Link::NotEnabled, Extension::Installed))]);
    host.cancel = true;
    let (end, _) = go(&mut host, true, false);
    assert_eq!(end, Phase::Cancelled);
    assert!(host.goals.is_empty());
    assert!(host.restarts.is_empty());
    assert!(end.is_over());
}

#[test]
fn a_waiting_task_that_is_cancelled_never_restarts_the_session() {
    let mut host = script(vec![Ok(menu_with(Link::NotEnabled, Extension::Installed))]);
    host.turns = VecDeque::from([true, true, true, true, true]);
    host.cancel_after_ms = Some(flow::POLL_MS);
    let (end, seen) = go(&mut host, true, false);
    assert_eq!(end, Phase::Cancelled);
    assert!(seen.contains(&Phase::Waiting { why: Wait::Turn }));
    assert!(host.restarts.is_empty());
    assert!(host.goals.is_empty());
}

#[test]
fn a_restarted_session_that_comes_up_not_connected_is_reconnected() {
    let mut host = script(vec![
        Ok(menu_with(Link::NotEnabled, Extension::Installed)),
        Ok(menu_with(Link::NotConnected, Extension::Installed)),
        Ok(menu_with(Link::NotConnected, Extension::Installed)),
        Ok(menu_with(Link::NotConnected, Extension::Installed)),
        Ok(menu_with(Link::Enabled, Extension::Installed)),
        Ok(menu_with(Link::Enabled, Extension::Installed)),
    ]);
    let (end, _) = go(&mut host, true, false);
    assert_eq!(end, Phase::Done { result: Done::On });
    assert_eq!(host.restarts, [true]);
    assert!(host.goals.contains(&Goal::Reconnect));
}

#[test]
fn a_session_that_never_opened_after_a_restart_is_not_called_not_connected() {
    let mut looks = vec![Ok(menu_with(Link::NotEnabled, Extension::Installed))];
    for _ in 0..flow::CONNECT_LOOKS {
        looks.push(Err(Refusal::Starting));
    }
    let mut host = script(looks);
    let (end, _) = go(&mut host, true, false);
    assert_eq!(
        end,
        Phase::NeedsYou {
            need: Need::Failed {
                text: Refusal::Starting.to_string()
            }
        }
    );
}

#[test]
fn a_cancelled_phase_reads_back_from_json() {
    let json = serde_json::to_value(Phase::Cancelled).unwrap();
    assert_eq!(json, serde_json::json!({"phase": "cancelled"}));
    let back: Phase = serde_json::from_value(json).unwrap();
    assert_eq!(back, Phase::Cancelled);
}

#[test]
fn prose_after_the_command_is_a_normal_message() {
    assert_eq!(command::parse("/chrome doesn't connect, why?"), None);
    assert_eq!(command::parse("/chrome off now"), None);
    assert_eq!(command::parse("/chrome on"), None);
    assert_eq!(command::parse("/chrome  off "), Some(Command::Off));
    assert!(!command::is_chrome("/chrome please fix it"));
}

#[test]
fn work_in_flight_counts_tasks_monitors_and_queued_prompts_and_fails_closed() {
    let root = TempDir::new("chrome-work");
    let file = |body: &str| root.write("state.json", body);
    let counted = |body: &str| status::tasks_in_flight(&file(body));
    assert_eq!(counted(r#"{"inFlight":{"tasks":0,"queued":0}}"#), Some(0));
    assert_eq!(counted(r#"{"inFlight":{"tasks":2,"queued":1}}"#), Some(3));
    assert_eq!(
        counted(r#"{"inFlight":{"tasks":1,"drainableMonitors":1,"queued":0}}"#),
        Some(1)
    );
    assert_eq!(
        counted(r#"{"inFlight":{"tasks":0,"drainableMonitors":2}}"#),
        Some(2)
    );
    assert_eq!(counted(r#"{"state":"done"}"#), None);
    assert_eq!(counted("not json"), None);
    assert_eq!(counted(r#"{"inFlight":3}"#), None);
    assert_eq!(status::tasks_in_flight(&root.path("missing.json")), None);
}

#[test]
fn not_knowing_about_background_work_asks_before_restarting() {
    let mut host = script(vec![Ok(menu_with(Link::NotEnabled, Extension::Installed))]);
    host.tasks = None;
    let (end, _) = go(&mut host, true, false);
    assert_eq!(
        end,
        Phase::NeedsYou {
            need: Need::TasksUnknown
        }
    );
    assert!(host.restarts.is_empty());
    let mut agreed = script(vec![
        Ok(menu_with(Link::NotEnabled, Extension::Installed)),
        Ok(menu_with(Link::Enabled, Extension::Installed)),
    ]);
    agreed.tasks = None;
    let (end, _) = go(&mut agreed, true, true);
    assert_eq!(end, Phase::Done { result: Done::On });
    assert_eq!(agreed.restarts, [true]);
}

#[test]
fn a_box_or_question_wait_backs_off_and_stops_at_thirty_seconds() {
    let mut looks: Vec<Result<menu::Menu, Refusal>> =
        (0..7).map(|_| Err(Refusal::BoxHasText)).collect();
    looks.push(Ok(menu_with(Link::Enabled, Extension::Installed)));
    let mut host = script(looks);
    let (end, _) = go(&mut host, true, false);
    assert_eq!(
        end,
        Phase::Done {
            result: Done::AlreadyOn
        }
    );
    assert_eq!(
        host.sleeps,
        [3_000, 6_000, 12_000, 24_000, 30_000, 30_000, 30_000]
    );
    let mut turn = script(vec![Ok(menu_with(Link::Enabled, Extension::Installed))]);
    turn.turns = VecDeque::from([true, true, true, true]);
    let _ = go(&mut turn, true, false);
    assert!(turn.sleeps.iter().all(|ms| *ms == flow::POLL_MS));
}

#[test]
fn the_flag_edit_changes_only_the_flags_text() {
    let root = TempDir::new("chrome-text");
    let original = "{\n  \"z\": 1.50,\n  \"n\": 1e3,\n  \"nest\": {\"respawnFlags\": [\"keep\"]},\n  \"respawnFlags\" : [ \"--name\" , \"a\\\"b\" ],\n  \"inFlight\": {\"tasks\": 0}\n}\n";
    let file = root.write("state.json", original);
    let flags: Vec<String> = ["--name", "a\"b", "--chrome"]
        .iter()
        .map(|f| f.to_string())
        .collect();
    status::set_flags(&file, &flags).unwrap();
    let after = std::fs::read_to_string(&file).unwrap();
    assert_eq!(
        after,
        original.replace(
            "[ \"--name\" , \"a\\\"b\" ]",
            "[\"--name\",\"a\\\"b\",\"--chrome\"]"
        )
    );
    assert_eq!(status::read_flags(&file).unwrap(), flags);
}

#[test]
fn a_linked_state_file_is_left_alone_and_the_copy_is_private() {
    use std::os::unix::fs::PermissionsExt;
    let root = TempDir::new("chrome-link");
    let real = root.write("real.json", r#"{"respawnFlags":["--no-chrome"]}"#);
    let link = root.path("state.json");
    std::os::unix::fs::symlink(&real, &link).unwrap();
    let flags = vec!["--chrome".to_string()];
    assert!(status::set_flags(&link, &flags).is_err());
    assert_eq!(status::read_flags(&real).unwrap(), ["--no-chrome"]);
    std::fs::set_permissions(&real, std::fs::Permissions::from_mode(0o640)).unwrap();
    status::set_flags(&real, &flags).unwrap();
    let mode = std::fs::metadata(&real).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o640);
    assert!(!root.path("real.json.notchling-tmp").exists());
}

#[test]
fn a_pending_change_is_written_read_and_cleared() {
    let root = TempDir::new("chrome-pending");
    let state = root.write("state.json", r#"{"respawnFlags":[]}"#);
    assert_eq!(status::read_pending(&state), None);
    let pending = status::Pending {
        on: true,
        before: vec!["--no-chrome".to_string()],
        wanted: vec!["--chrome".to_string()],
    };
    status::write_pending(&state, &pending).unwrap();
    assert_eq!(status::read_pending(&state), Some(pending));
    status::clear_pending(&state);
    assert_eq!(status::read_pending(&state), None);
}

#[test]
fn the_permission_mode_is_read_from_the_transcript_and_kept_in_the_flags() {
    let root = TempDir::new("chrome-mode");
    let transcript = root.write(
        "t.jsonl",
        concat!(
            r#"{"type":"permission-mode","permissionMode":"plan","sessionId":"s"}"#,
            "\n",
            r#"{"type":"user","message":"hi"}"#,
            "\n",
            r#"{"type":"permission-mode","permissionMode":"acceptEdits","sessionId":"s"}"#,
            "\n",
            r#"{"type":"user","message":"again"}"#,
            "\n"
        ),
    );
    assert_eq!(
        status::permission_mode(&transcript).as_deref(),
        Some("acceptEdits")
    );
    let odd = root.write(
        "u.jsonl",
        "{\"type\":\"permission-mode\",\"permissionMode\":\"bypassPermissions\"}\n",
    );
    assert_eq!(status::permission_mode(&odd), None);
    let flags: Vec<String> = ["--permission-mode", "auto", "--model", "m", "--chrome"]
        .iter()
        .map(|f| f.to_string())
        .collect();
    assert_eq!(
        status::with_mode(&flags, "plan"),
        ["--model", "m", "--chrome", "--permission-mode", "plan"]
    );
}
