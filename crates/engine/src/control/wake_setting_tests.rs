use super::*;
use crate::sessions::testutil::TempDir;
use crate::settings::Settings;
use serde_json::json;

#[test]
fn nothing_stored_means_continue_as_the_swift_app_did() {
    let tmp = TempDir::new("wake-setting-default");
    let settings = Settings::open(&tmp.path("settings.json")).unwrap();
    assert!(enabled(&settings));
}

#[test]
fn the_stored_choice_is_what_is_read() {
    let tmp = TempDir::new("wake-setting-stored");
    let mut settings = Settings::open(&tmp.path("settings.json")).unwrap();
    settings.set(KEY, json!(false)).unwrap();
    assert!(!enabled(&settings));
    settings.set(KEY, json!(true)).unwrap();
    assert!(enabled(&settings));
}

#[test]
fn the_key_is_the_one_settings_knows() {
    assert!(crate::settings::KEYS.iter().any(|k| k.name == KEY));
}

#[test]
fn a_resumer_waits_from_a_wake_until_its_window_has_passed() {
    let mut resumer = WakeResumer::new();
    assert!(!resumer.waiting());
    resumer.did_wake(0);
    assert!(resumer.waiting());
    assert!(resumer
        .check(&[], SETTLE_MS, true, &|_| None)
        .is_empty());
    assert!(resumer.waiting(), "the window is still open");
    assert!(resumer
        .check(&[], WINDOW_MS + 1, true, &|_| None)
        .is_empty());
    assert!(!resumer.waiting(), "a late look closes it");
}
