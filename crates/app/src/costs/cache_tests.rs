use super::cache::{Cache, Ttl};

#[test]
fn without_keys_the_defaults_are_one_hour_and_five_minutes() {
    let c = Cache::parse("{}");
    assert_eq!(c.main, Ttl::OneHour);
    assert_eq!(c.helpers, Ttl::FiveMinutes);
    assert!(!c.set_main && !c.set_helpers);
}

#[test]
fn the_two_keys_are_read_from_settings() {
    let c = Cache::parse(r#"{"promptCacheTtl": "5m", "subagentPromptCacheTtl": "1h"}"#);
    assert_eq!(c.main, Ttl::FiveMinutes);
    assert_eq!(c.helpers, Ttl::OneHour);
    assert!(c.set_main && c.set_helpers);
}

#[test]
fn another_value_is_kept_as_it_is() {
    let c = Cache::parse(r#"{"promptCacheTtl": "2h"}"#);
    assert_eq!(c.main, Ttl::Other("2h".to_owned()));
}

#[test]
fn a_cache_control_key_is_not_a_cache_time() {
    let c = Cache::parse(r#"{"cache_control": {"ttl": "2h"}}"#);
    assert_eq!(c, Cache::default());
}

#[test]
fn text_that_is_not_json_gives_the_defaults() {
    assert_eq!(Cache::parse("not json"), Cache::default());
}

#[test]
fn a_folder_with_no_settings_gives_the_defaults() {
    let dir = std::env::temp_dir().join("notchling-cache-test-none");
    assert_eq!(Cache::read(&dir), Cache::default());
}

#[test]
fn a_fixture_folder_is_read_from_its_own_settings() {
    let dir = std::env::temp_dir().join(format!("notchling-cache-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("settings.json"),
        r#"{"subagentPromptCacheTtl": "1h"}"#,
    )
    .unwrap();
    let c = Cache::read(&dir);
    std::fs::remove_dir_all(&dir).ok();
    assert_eq!(c.helpers, Ttl::OneHour);
    assert!(c.set_helpers);
}
