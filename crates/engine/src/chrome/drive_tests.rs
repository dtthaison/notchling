use super::chrome_tests::{chrome_walk, go, menu_with, script, Fake, MENU_ON, OFF_ATTACH, TYPED};
use super::drive::{self, Goal, Refusal};
use super::flow::{Done, Need, Phase};
use super::menu::{self, Extension, Link};

const BOTTOM_ATTACH: &str = include_str!("../../tests/fixtures/chrome/bottom-attach.txt");
const BOTTOM_TYPED: &str = include_str!("../../tests/fixtures/chrome/bottom-typed-slash.txt");
const BOTTOM_CLOSED: &str = include_str!("../../tests/fixtures/chrome/bottom-closed-box-empty.txt");

fn without_command_row(text: &str) -> String {
    text.lines()
        .filter(|l| !l.contains(menu::COMMAND_HINT))
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn a_list_drawn_above_a_box_at_the_bottom_is_found() {
    let mut pty = Fake::new(
        &[BOTTOM_ATTACH, BOTTOM_TYPED, MENU_ON, BOTTOM_CLOSED],
        chrome_walk,
    );
    let report = drive::run(&mut pty, Goal::Read).unwrap();
    assert_eq!(report.menu.link, Link::Enabled);
    assert_eq!(pty.writes, ["/chrome", "\r", "\x1b"]);
}

#[test]
fn a_list_without_the_command_is_the_one_sure_sign_it_is_missing() {
    for typed in [
        without_command_row(BOTTOM_TYPED),
        without_command_row(TYPED),
    ] {
        let attach = if typed.starts_with("cursor 46") {
            BOTTOM_ATTACH
        } else {
            OFF_ATTACH
        };
        let mut pty = Fake::new(&[attach, &typed], |state, write| match (state, write) {
            (0, "/chrome") => 1,
            (s, _) => s,
        });
        assert_eq!(drive::run(&mut pty, Goal::Read), Err(Refusal::Unavailable));
        assert!(pty.writes.iter().any(|w| w.starts_with('\x7f')));
        assert!(!pty.writes.iter().any(|w| w == "\r"));
    }
}

#[test]
fn no_list_at_all_or_a_reworded_one_is_not_called_missing() {
    let reworded = BOTTOM_TYPED.replace(menu::COMMAND_HINT, "Something else");
    let bare: String = BOTTOM_TYPED
        .lines()
        .enumerate()
        .map(|(i, l)| if (41..=45).contains(&i) { "" } else { l })
        .collect::<Vec<_>>()
        .join("\n");
    for typed in [reworded, bare] {
        let mut pty = Fake::new(&[BOTTOM_ATTACH, &typed], |state, write| {
            match (state, write) {
                (0, "/chrome") => 1,
                (s, _) => s,
            }
        });
        assert_eq!(drive::run(&mut pty, Goal::Read), Err(Refusal::NotListed));
        assert!(pty.writes.iter().any(|w| w.starts_with('\x7f')));
        assert!(!pty.writes.iter().any(|w| w == "\r"));
    }
}

#[test]
fn a_menu_that_could_not_be_opened_says_so_and_a_missing_command_says_that() {
    let mut host = script(vec![Err(Refusal::NotListed)]);
    let (end, _) = go(&mut host, true, false);
    assert_eq!(
        end,
        Phase::NeedsYou {
            need: Need::Failed {
                text: "Couldn't open Claude Code's /chrome menu.".to_string()
            }
        }
    );
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
fn turning_on_a_session_whose_flags_say_no_chrome_never_opens_the_menu() {
    let mut host = script(vec![Ok(menu_with(Link::Enabled, Extension::Installed))]);
    host.flag = Some(false);
    let (end, _) = go(&mut host, true, false);
    assert_eq!(host.restarts, [true]);
    assert_eq!(host.goals, [Goal::Read]);
    assert_eq!(end, Phase::Done { result: Done::On });
}

#[test]
fn turning_off_a_session_whose_flags_say_chrome_never_opens_the_menu_first() {
    let mut host = script(vec![Ok(menu_with(Link::NotEnabled, Extension::Installed))]);
    host.flag = Some(true);
    let (end, _) = go(&mut host, false, false);
    assert_eq!(host.restarts, [false]);
    assert_eq!(host.goals, [Goal::Read]);
    assert_eq!(end, Phase::Done { result: Done::Off });
}

#[test]
fn turning_off_a_session_whose_flags_say_no_chrome_does_nothing() {
    let mut host = script(Vec::new());
    host.flag = Some(false);
    let (end, _) = go(&mut host, false, false);
    assert!(host.restarts.is_empty());
    assert!(host.goals.is_empty());
    assert_eq!(
        end,
        Phase::Done {
            result: Done::AlreadyOff
        }
    );
}

#[test]
fn the_flags_path_keeps_the_turn_and_background_work_checks() {
    let mut host = script(Vec::new());
    host.flag = Some(false);
    host.tasks = Some(2);
    let (end, _) = go(&mut host, true, false);
    assert_eq!(
        end,
        Phase::NeedsYou {
            need: Need::Tasks { count: 2 }
        }
    );
    assert!(host.restarts.is_empty());
    assert!(host.goals.is_empty());
    let mut waiting = script(vec![Ok(menu_with(Link::Enabled, Extension::Installed))]);
    waiting.flag = Some(false);
    waiting.turns = [true, true, false].into();
    let (end, _) = go(&mut waiting, true, false);
    assert_eq!(waiting.restarts, [true]);
    assert_eq!(end, Phase::Done { result: Done::On });
    assert!(!waiting.sleeps.is_empty());
}

#[test]
fn a_session_that_has_chrome_in_its_flags_is_still_checked_for_a_connection() {
    let mut host = script(vec![Ok(menu_with(Link::Enabled, Extension::Installed))]);
    host.flag = Some(true);
    let (end, _) = go(&mut host, true, false);
    assert!(host.restarts.is_empty());
    assert_eq!(host.goals, [Goal::Read]);
    assert_eq!(
        end,
        Phase::Done {
            result: Done::AlreadyOn
        }
    );
}
