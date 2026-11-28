use super::*;
use crate::settings::{spec, Access, Fallback};
use serde_json::json;
use std::path::PathBuf;

struct Dir(PathBuf);

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn dir(tag: &str) -> Dir {
    let path = std::env::temp_dir().join(format!("nl-keep-finished-{}-{tag}", std::process::id()));
    let _ = std::fs::remove_dir_all(&path);
    std::fs::create_dir_all(&path).expect("make temp dir");
    Dir(path)
}

#[test]
fn parse_default_and_round_trip() {
    assert_eq!(KeepFinished::default(), KeepFinished::Days1);
    assert_eq!(KeepFinished::Days1.window_ms(), 24 * 3600 * 1000);
    assert_eq!(KeepFinished::Days7.window_ms(), 7 * 24 * 3600 * 1000);
    assert_eq!(KeepFinished::Days30.window_ms(), 30 * 24 * 3600 * 1000);
    for choice in KeepFinished::ALL {
        assert_eq!(KeepFinished::parse(choice.as_str()), Some(choice));
        assert_eq!(KeepFinished::from_setting(choice.as_str()), choice);
    }
    assert_eq!(KeepFinished::parse("14d"), None);
    assert_eq!(KeepFinished::parse(""), None);
    assert_eq!(KeepFinished::from_setting("14d"), KeepFinished::Days1);
    assert_eq!(KeepFinished::from_setting(""), KeepFinished::Days1);
}

#[test]
fn key_spec_matches_choices_and_is_free() {
    let key = spec(KEEP_FINISHED_KEY).expect("the keepFinishedSessions key");
    assert_eq!(key.default, Fallback::Str("1d"));
    assert_eq!(key.default_value(), Some(json!("1d")));
    let allowed: Vec<&str> = KeepFinished::ALL.iter().map(|c| c.as_str()).collect();
    assert_eq!(key.allowed, allowed.as_slice());
    assert_eq!(key.access, Access::Free);
    assert!(!key.from_awake);
    assert_eq!(key.from_text("7D"), json!("7d"));
    assert_eq!(key.from_text("30D"), json!("30d"));
    assert!(key.accepts(&json!("7d")));
    assert!(!key.accepts(&json!("14d")));
    assert!(!key.accepts(&json!(7)));
}

#[test]
fn missing_store_and_round_trip_through_settings() {
    let d = dir("round-trip");
    let missing = d.0.join("absent");
    assert_eq!(KeepFinished::read(&missing), Some(KeepFinished::Days1));
    assert_eq!(KeepFinished::window_ms_in(&missing), DAY_MS);
    assert!(!missing.exists());

    assert_eq!(KeepFinished::read(&d.0), Some(KeepFinished::Days1));
    assert_eq!(KeepFinished::window_ms_in(&d.0), DAY_MS);

    let mut settings = Settings::open(&d.0.join("settings.json")).expect("open settings");
    for choice in [
        KeepFinished::Days7,
        KeepFinished::Days30,
        KeepFinished::Days1,
    ] {
        settings
            .set(KEEP_FINISHED_KEY, json!(choice.as_str()))
            .expect("set choice");
        assert_eq!(KeepFinished::from_settings(&settings), choice);
        assert_eq!(KeepFinished::read(&d.0), Some(choice));
        assert_eq!(KeepFinished::window_ms_in(&d.0), choice.window_ms());
    }
}

#[test]
fn unknown_or_corrupt_store_falls_back_to_one_day() {
    let d = dir("bad");
    let mut settings = Settings::open(&d.0.join("settings.json")).expect("open settings");
    assert!(settings.set(KEEP_FINISHED_KEY, json!("90d")).is_err());
    settings
        .set_unchecked(KEEP_FINISHED_KEY, json!("90d"))
        .expect("write unknown");
    assert_eq!(KeepFinished::read(&d.0), Some(KeepFinished::Days1));
    assert_eq!(KeepFinished::window_ms_in(&d.0), DAY_MS);

    let broken = dir("corrupt");
    std::fs::write(broken.0.join("notchling.db"), "not sqlite").expect("write");
    assert_eq!(KeepFinished::read(&broken.0), None);
    assert_eq!(KeepFinished::window_ms_in(&broken.0), DAY_MS);
}

#[test]
fn chosen_matches_only_valid_values_for_this_key() {
    assert_eq!(
        KeepFinished::chosen(KEEP_FINISHED_KEY, &json!("7d")),
        Some(KeepFinished::Days7)
    );
    assert_eq!(
        KeepFinished::chosen(KEEP_FINISHED_KEY, &json!("30d")),
        Some(KeepFinished::Days30)
    );
    assert_eq!(KeepFinished::chosen(KEEP_FINISHED_KEY, &json!("14d")), None);
    assert_eq!(KeepFinished::chosen(KEEP_FINISHED_KEY, &json!(7)), None);
    assert_eq!(KeepFinished::chosen("chatHistory", &json!("7d")), None);
}
