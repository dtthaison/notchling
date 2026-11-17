use serde_json::json;

use super::*;
use crate::menu::{tray_glyph, AwakeMode, MenuState, TrayGlyph};

fn chosen(awake: AwakeMode) -> MenuState {
    MenuState {
        awake,
        ..MenuState::default()
    }
}

#[test]
fn the_glyph_is_filled_only_while_the_daemon_holds_the_assertion() {
    let on = chosen(AwakeMode::On);
    assert_eq!(tray_glyph(&on), TrayGlyph::Outline, "chosen is not held");
    let held = holding(&on, true).unwrap();
    assert_eq!(tray_glyph(&held), TrayGlyph::Filled);
    let let_go = holding(&held, false).unwrap();
    assert_eq!(tray_glyph(&let_go), TrayGlyph::Outline);
    assert_eq!(let_go.awake, AwakeMode::On, "the choice stays");
}

#[test]
fn a_report_that_changes_nothing_is_no_change() {
    let on = chosen(AwakeMode::On);
    assert_eq!(holding(&on, false), None);
    let held = holding(&on, true).unwrap();
    assert_eq!(holding(&held, true), None);
}

#[test]
fn picking_another_mode_keeps_what_is_held_until_the_daemon_says_otherwise() {
    let held = holding(&chosen(AwakeMode::On), true).unwrap();
    let off = with(&held, MODE, &json!("off")).unwrap();
    assert_eq!(off.awake, AwakeMode::Off);
    assert!(off.holding);
    let display = with(&held, DISPLAY, &json!(true)).unwrap();
    assert!(display.keep_display_on && display.holding);
}

#[test]
fn a_menu_read_from_the_store_holds_nothing_yet() {
    let s = crate::testutil::Scratch::new("keep-awake-hold-start");
    let mut store = Settings::open(&s.path().join("settings.json")).unwrap();
    store.set("mode", json!("on")).unwrap();
    assert!(!read(s.path()).holding);
}
