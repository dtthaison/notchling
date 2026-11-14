use super::log_value;
use crate::settings::KEYS;
use notchling_events::kinds::settings::{SettingKey, SettingValue};
use notchling_events::{Count, Salt};
use serde_json::json;

const SALT: Salt = Salt([5; 32]);

#[test]
fn every_key_is_a_setting_key_choice() {
    for key in KEYS {
        let choice = SettingKey::from_name(key.name);
        assert_eq!(
            choice.map(SettingKey::name),
            Some(key.name),
            "{} has no SettingKey",
            key.name
        );
    }
    for name in SettingKey::VALUES {
        if matches!(*name, "unknown" | "other") {
            continue;
        }
        assert!(
            KEYS.iter().any(|k| k.name == *name),
            "SettingKey {name} is not a settings key"
        );
    }
}

#[test]
fn every_allowed_value_is_a_setting_value() {
    for key in KEYS {
        for allowed in key.allowed {
            assert_eq!(
                SettingValue::from_name(allowed).map(SettingValue::name),
                Some(*allowed),
                "{} allows {allowed}",
                key.name
            );
        }
    }
}

#[test]
fn a_flag_is_written_as_a_flag() {
    let logged = log_value("hideNotch", &json!(true), &SALT);
    assert_eq!(logged.key, SettingKey::HideNotch);
    assert_eq!(logged.value_flag, Some(true));
    assert!(logged.value_hash.is_none() && logged.value_chars.is_none());
}

#[test]
fn a_number_is_written_as_a_number() {
    let logged = log_value("limitSwitchAt", &json!(90), &SALT);
    assert_eq!(logged.value_number, Some(Count(90)));
}

#[test]
fn a_choice_is_written_as_its_name() {
    let logged = log_value("chatHistory", &json!("8mb"), &SALT);
    assert_eq!(logged.value_choice, Some(SettingValue::Mb8));
    assert!(logged.value_hash.is_none());
}

#[test]
fn a_choice_outside_the_list_is_only_counted_and_hashed() {
    let logged = log_value("chatHistory", &json!("secret-marker-9"), &SALT);
    assert!(logged.value_choice.is_none());
    assert_eq!(logged.value_chars, Some(Count(15)));
    assert!(logged.value_hash.is_some());
}

#[test]
fn text_value_is_chars_and_hash() {
    let logged = log_value("searchShortcut", &json!("ctrl+alt+x"), &SALT);
    assert_eq!(logged.value_chars, Some(Count(10)));
    let hash = logged.value_hash.clone().unwrap();
    assert_eq!(hash.as_str().len(), 16);
    assert!(!format!("{logged:?}").contains("ctrl+alt+x"));
    let other_salt = log_value("searchShortcut", &json!("ctrl+alt+x"), &Salt([6; 32]));
    assert_ne!(other_salt.value_hash, Some(hash));
}

#[test]
fn unknown_key_is_hashed() {
    let logged = log_value("my-private-key", &json!("v"), &SALT);
    assert_eq!(logged.key, SettingKey::Unknown);
    assert_eq!(logged.key_hash.as_ref().map(|h| h.as_str().len()), Some(16));
    assert!(!format!("{logged:?}").contains("my-private-key"));
}
