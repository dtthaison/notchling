use serde_json::json;

use super::*;
use crate::menu::{AwakeMode, Command, MenuState};
use crate::testutil::Scratch;
use notchling_engine::settings::Settings;

fn state(awake: AwakeMode, keep_display_on: bool) -> MenuState {
    MenuState {
        awake,
        keep_display_on,
        ..MenuState::default()
    }
}

#[test]
fn the_menu_words_are_the_words_the_daemon_reads() {
    for mode in AwakeMode::ALL {
        assert_eq!(from_word(word(mode)), Some(mode), "{mode:?}");
        assert!(
            notchling_engine::awake::Mode::parse(word(mode)).is_some(),
            "the daemon must know {}",
            word(mode)
        );
    }
    assert_eq!(word(AwakeMode::WhileWorking), "whileWorking");
    assert_eq!(from_word("forever"), None);
}

#[test]
fn nothing_stored_is_off_with_the_display_alone() {
    let s = Scratch::new("keep-awake-empty");
    assert_eq!(read(s.path()), state(AwakeMode::Off, false));
}

#[test]
fn the_stored_choice_is_what_the_menu_starts_with() {
    let s = Scratch::new("keep-awake-stored");
    let mut store = Settings::open(&s.path().join("settings.json")).unwrap();
    store.set("mode", json!("whileWorking")).unwrap();
    store.set("keepDisplayOn", json!(true)).unwrap();
    assert_eq!(read(s.path()), state(AwakeMode::WhileWorking, true));
}

#[test]
fn picking_a_mode_names_the_setting_to_write() {
    let now = state(AwakeMode::Off, false);
    assert_eq!(
        wanted(Command::SetAwake(AwakeMode::On), &now),
        Some((MODE, json!("on")))
    );
    assert_eq!(
        wanted(Command::SetAwake(AwakeMode::WhileWorking), &now),
        Some((MODE, json!("whileWorking")))
    );
}

#[test]
fn toggling_the_display_writes_the_opposite_of_what_shows() {
    assert_eq!(
        wanted(Command::ToggleDisplay, &state(AwakeMode::On, false)),
        Some((DISPLAY, json!(true)))
    );
    assert_eq!(
        wanted(Command::ToggleDisplay, &state(AwakeMode::On, true)),
        Some((DISPLAY, json!(false)))
    );
}

#[test]
fn no_other_command_writes_a_setting() {
    let now = state(AwakeMode::Off, false);
    for command in [
        Command::Accounts,
        Command::OpenChat,
        Command::Settings,
        Command::About,
        Command::Welcome,
        Command::Quit,
    ] {
        assert_eq!(wanted(command, &now), None, "{command:?}");
    }
}

#[test]
fn a_changed_setting_moves_the_menu_and_an_unchanged_one_does_not() {
    let now = state(AwakeMode::Off, false);
    assert_eq!(
        with(&now, MODE, &json!("on")),
        Some(state(AwakeMode::On, false))
    );
    assert_eq!(
        with(&now, DISPLAY, &json!(true)),
        Some(state(AwakeMode::Off, true))
    );
    assert_eq!(with(&now, MODE, &json!("off")), None);
    assert_eq!(with(&now, DISPLAY, &json!(false)), None);
}

#[test]
fn another_key_or_a_value_that_is_not_one_leaves_the_menu_alone() {
    let now = state(AwakeMode::On, true);
    assert_eq!(with(&now, "appearance", &json!("dark")), None);
    assert_eq!(with(&now, MODE, &json!("forever")), None);
    assert_eq!(with(&now, MODE, &json!(3)), None);
    assert_eq!(with(&now, DISPLAY, &json!("yes")), None);
}

#[test]
fn the_account_row_is_kept_when_the_setting_moves() {
    let now = MenuState {
        account: Some("me@example.com".to_owned()),
        ..state(AwakeMode::Off, false)
    };
    let next = with(&now, MODE, &json!("on")).unwrap();
    assert_eq!(next.account.as_deref(), Some("me@example.com"));
}

#[test]
fn writing_a_choice_lands_where_the_daemon_reads_it() {
    let s = Scratch::new("keep-awake-write");
    write(s.path(), MODE, &json!("whileWorking")).unwrap();
    write(s.path(), DISPLAY, &json!(true)).unwrap();
    assert_eq!(read(s.path()), state(AwakeMode::WhileWorking, true));
    let daemon_view =
        notchling_engine::awake::read(&Settings::open(&s.path().join("settings.json")).unwrap());
    assert_eq!(
        daemon_view.mode,
        notchling_engine::awake::Mode::WhileWorking
    );
    assert!(daemon_view.display);
}

#[test]
fn a_write_the_store_refuses_is_an_error_and_not_a_silent_loss() {
    let s = Scratch::new("keep-awake-refused");
    assert!(write(s.path(), MODE, &json!("forever")).is_err());
    assert!(write(s.path(), "nonsense", &json!(true)).is_err());
}
