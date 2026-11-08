//! The four values and where they are kept.

use super::mode::*;
use crate::sessions::testutil::TempDir;

#[test]
fn modes_use_the_swift_keys_and_names() {
    assert_eq!(CLAUDE.key, "limitMode");
    assert_eq!(CODEX.key, "codexLimitMode");
    assert_eq!(ANTIGRAVITY.key, "antigravityLimitMode");
    for m in ALL_MODES {
        assert_eq!(LimitMode::parse(m.as_str()), Some(m));
    }
    assert_eq!(LimitMode::NotifyAndSwitch.as_str(), "notifyAndSwitch");
    assert_eq!(LimitMode::NotifyAndSwitch.title(), "Notify and Switch");
    assert_eq!(LimitMode::parse("nonsense"), None);
}

#[test]
fn defaults_match_the_key_table() {
    let config = LimitConfig::in_memory();
    for p in PROVIDERS {
        let default = crate::settings::spec(p.key)
            .and_then(|s| s.default_value())
            .and_then(|v| v.as_str().map(String::from));
        assert_eq!(default.as_deref(), Some(p.default.as_str()), "{}", p.key);
        assert_eq!(config.mode(&p), p.default);
    }
}

#[test]
fn the_switch_point_is_held_to_80_to_99() {
    let config = LimitConfig::in_memory();
    assert_eq!(config.switch_at(), 95);
    config.set_switch_at(80).unwrap();
    assert_eq!(config.switch_at(), 80);
    assert!(config.set_switch_at(79).is_err());
    assert!(config.set_switch_at(100).is_err());
    assert_eq!(config.switch_at(), 80);
}

#[test]
fn antigravity_never_switches() {
    let config = LimitConfig::in_memory();
    assert!(config
        .set_mode(&ANTIGRAVITY, LimitMode::NotifyAndSwitch)
        .is_err());
    assert!(config.set_mode(&CODEX, LimitMode::NotifyAndSwitch).is_ok());
    assert_eq!(config.mode(&CODEX), LimitMode::NotifyAndSwitch);
}

#[test]
fn choices_survive_a_restart_and_a_bad_file_is_never_written_over() {
    let tmp = TempDir::new("limit-config");
    let file = tmp.path("daemon/limits.json");
    let config = LimitConfig::open(&file);
    config.set_mode(&CLAUDE, LimitMode::Off).unwrap();
    config.set_switch_at(85).unwrap();
    let again = LimitConfig::open(&file);
    assert_eq!(again.mode(&CLAUDE), LimitMode::Off);
    assert_eq!(again.switch_at(), 85);

    // A stored mode the provider doesn't allow reads as its default.
    std::fs::write(
        &file,
        r#"{"version":1,"values":{"antigravityLimitMode":"notifyAndSwitch","limitSwitchAt":"x"}}"#,
    )
    .unwrap();
    let odd = LimitConfig::open(&file);
    assert_eq!(odd.mode(&ANTIGRAVITY), LimitMode::Notify);
    assert_eq!(odd.switch_at(), 95);

    std::fs::write(&file, "{ not json").unwrap();
    let bad = LimitConfig::open(&file);
    assert!(bad.set_mode(&CLAUDE, LimitMode::Off).is_err());
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "{ not json");
    assert_eq!(bad.mode(&CLAUDE), CLAUDE.default);
}
