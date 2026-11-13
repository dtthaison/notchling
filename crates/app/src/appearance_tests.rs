use serde_json::json;

use super::*;
use crate::testutil::Scratch;
use notchling_engine::settings::{spec, Fallback};
use notchling_kit::Appearance;

fn settings_in(s: &Scratch) -> Settings {
    Settings::open(&s.path().join("settings.json")).expect("open settings")
}

#[test]
fn the_key_allows_exactly_the_choices_the_sheet_offers() {
    let key = spec(KEY).expect("the key is listed");
    let offered: Vec<&str> = CHOICES.iter().map(|(value, _)| *value).collect();
    assert_eq!(key.allowed, offered.as_slice());
    assert_eq!(key.default, Fallback::Str("system"));
}

#[test]
fn every_choice_parses_and_system_is_first() {
    assert_eq!(CHOICES[0].0, "system");
    for (value, _) in CHOICES {
        let chosen = Preference::parse(value).expect("a choice parses");
        assert_eq!(chosen.name(), value);
    }
}

#[test]
fn an_unset_or_unknown_value_follows_the_system() {
    let s = Scratch::new("appearance-unset");
    let mut settings = settings_in(&s);
    assert_eq!(from_settings(&settings), Preference::System);
    settings
        .set_unchecked(KEY, json!("sepia"))
        .expect("unchecked");
    assert_eq!(from_settings(&settings), Preference::System);
}

#[test]
fn a_saved_choice_is_read_back_from_the_data_folder() {
    let s = Scratch::new("appearance-saved");
    let mut settings = settings_in(&s);
    settings.set(KEY, json!("dark")).expect("set");
    assert_eq!(read(s.path()), Some(Preference::Dark));
    settings.set(KEY, json!("light")).expect("set");
    assert_eq!(read(s.path()), Some(Preference::Light));
}

#[test]
fn a_forced_choice_wins_over_the_system_and_system_does_not() {
    assert_eq!(
        chosen(KEY, &json!("light")).map(|p| p.resolve(Appearance::Dark)),
        Some(Appearance::Light)
    );
    assert_eq!(
        chosen(KEY, &json!("system")).map(|p| p.resolve(Appearance::Dark)),
        Some(Appearance::Dark)
    );
    assert_eq!(chosen("chatHistory", &json!("dark")), None);
    assert_eq!(chosen(KEY, &json!("sepia")), None);
}
