//! The four values and where they are kept.

use super::mode::*;
use crate::sessions::testutil::TempDir;
use crate::store;

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
    let data = tmp.path("data");
    let config = LimitConfig::open(&data);
    assert!(
        !data.exists(),
        "opening with nothing to move in makes no database"
    );
    config.set_mode(&CLAUDE, LimitMode::Off).unwrap();
    config.set_switch_at(85).unwrap();
    let again = LimitConfig::open(&data);
    assert_eq!(again.mode(&CLAUDE), LimitMode::Off);
    assert_eq!(again.switch_at(), 85);

    // A stored mode the provider doesn't allow reads as its default.
    store::put_doc(
        &data,
        LIMITS_DOC,
        r#"{"version":1,"values":{"antigravityLimitMode":"notifyAndSwitch","limitSwitchAt":"x"}}"#,
    )
    .unwrap();
    let odd = LimitConfig::open(&data);
    assert_eq!(odd.mode(&ANTIGRAVITY), LimitMode::Notify);
    assert_eq!(odd.switch_at(), 95);

    store::put_doc(&data, LIMITS_DOC, "{ not json").unwrap();
    let bad = LimitConfig::open(&data);
    assert!(bad.set_mode(&CLAUDE, LimitMode::Off).is_err());
    assert_eq!(
        store::doc(&data, LIMITS_DOC).unwrap().as_deref(),
        Some("{ not json")
    );
    assert_eq!(bad.mode(&CLAUDE), CLAUDE.default);
}

#[test]
fn the_old_file_moves_in_once_and_an_unreadable_one_stays() {
    let tmp = TempDir::new("limit-config-import");
    let data = tmp.path("data");
    let old = tmp.write(
        "data/daemon/limits.json",
        r#"{"version":1,"values":{"limitMode":"off","limitSwitchAt":90}}"#,
    );
    let config = LimitConfig::open(&data);
    assert!(!old.exists(), "the old file moved in");
    assert_eq!(config.mode(&CLAUDE), LimitMode::Off);
    assert_eq!(config.switch_at(), 90);
    config.set_mode(&CODEX, LimitMode::Off).unwrap();
    let again = LimitConfig::open(&data);
    assert_eq!(
        again.mode(&CLAUDE),
        LimitMode::Off,
        "kept beside the new choice"
    );
    assert_eq!(again.mode(&CODEX), LimitMode::Off);

    let tmp = TempDir::new("limit-config-import-bad");
    let data = tmp.path("data");
    let old = tmp.write("data/daemon/limits.json", "{ not json");
    let bad = LimitConfig::open(&data);
    assert!(bad.set_mode(&CLAUDE, LimitMode::Off).is_err());
    assert_eq!(std::fs::read_to_string(&old).unwrap(), "{ not json");
    assert_eq!(bad.mode(&CLAUDE), CLAUDE.default);
}

#[test]
fn two_configs_changing_different_values_keep_both() {
    let tmp = TempDir::new("limit-config-two");
    let data = tmp.path("data");
    let (a, b) = (LimitConfig::open(&data), LimitConfig::open(&data));
    a.set_mode(&CLAUDE, LimitMode::Off).unwrap();
    b.set_switch_at(88).unwrap();
    let all = LimitConfig::open(&data);
    assert_eq!(all.mode(&CLAUDE), LimitMode::Off);
    assert_eq!(all.switch_at(), 88);
}
