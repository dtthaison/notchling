use serde_json::json;

use super::*;
use crate::testutil::Scratch;
use notchling_engine::settings::{spec, Fallback};

fn settings_in(s: &Scratch) -> Settings {
    Settings::open(&s.path().join("settings.json")).expect("open settings")
}

#[test]
fn each_choice_maps_to_its_bytes() {
    let got: Vec<Option<u64>> = CHOICES.iter().map(|(value, _)| bytes(value)).collect();
    assert_eq!(
        got,
        [
            Some(2_000_000),
            Some(4_000_000),
            Some(8_000_000),
            Some(16_000_000)
        ]
    );
    assert_eq!(bytes("32mb"), None);
    assert_eq!(bytes(""), None);
}

#[test]
fn the_default_is_the_four_megabytes_chat_always_read() {
    assert_eq!(bytes("4mb"), Some(CHAT_TAIL_BYTES));
    let key = spec(KEY).expect("the key is listed");
    assert_eq!(key.default, Fallback::Str("4mb"));
}

#[test]
fn the_key_allows_exactly_the_choices_the_sheet_offers() {
    let key = spec(KEY).expect("the key is listed");
    let offered: Vec<&str> = CHOICES.iter().map(|(value, _)| *value).collect();
    assert_eq!(key.allowed, offered.as_slice());
}

#[test]
fn nothing_saved_reads_the_default() {
    let s = Scratch::new("history-empty");
    assert_eq!(read(s.path()), Some(CHAT_TAIL_BYTES));
}

#[test]
fn a_saved_choice_reads_back_as_bytes() {
    let s = Scratch::new("history-saved");
    let mut settings = settings_in(&s);
    for (value, want) in [("2mb", 2_000_000), ("16mb", 16_000_000), ("8mb", 8_000_000)] {
        settings.set(KEY, json!(value)).expect("set");
        assert_eq!(read(s.path()), Some(want));
    }
}

#[test]
fn a_value_that_is_not_a_choice_reads_the_default() {
    let s = Scratch::new("history-bad");
    let mut settings = settings_in(&s);
    assert!(settings.set(KEY, json!("1gb")).is_err());
    settings
        .set_unchecked(KEY, json!("1gb"))
        .expect("write it anyway");
    assert_eq!(read(s.path()), Some(CHAT_TAIL_BYTES));
}

#[test]
fn a_store_that_cannot_be_read_says_so_instead_of_giving_the_default() {
    let s = Scratch::new("history-broken");
    std::fs::write(s.path().join("notchling.db"), "this is not a database").expect("write");
    assert_eq!(read(s.path()), None);
}

#[test]
fn only_a_choice_for_this_key_is_a_change() {
    assert_eq!(chosen(KEY, &json!("8mb")), Some(8_000_000));
    assert_eq!(chosen(KEY, &json!("1gb")), None);
    assert_eq!(chosen(KEY, &json!(8)), None);
    assert_eq!(chosen("chatUsedSessions", &json!("8mb")), None);
}
