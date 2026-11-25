use super::*;
use crate::sessions::agent::sample;
use crate::sessions::testutil::TempDir;
use crate::sessions::{Agent, AgentGroup, AgentTarget};
use crate::settings::Settings;
use serde_json::json;

fn agent(group: AgentGroup) -> Agent {
    let mut a = sample(AgentTarget::Job("job-7".into()), None);
    a.group = group;
    a
}

fn setting(mode: Mode, display: bool) -> Setting {
    Setting { mode, display }
}

#[test]
fn a_working_session_is_busy_and_waiting_or_done_ones_are_not() {
    let none = Work::default();
    assert!(busy(&[agent(AgentGroup::Working)], &none));
    assert!(busy(
        &[agent(AgentGroup::Completed), agent(AgentGroup::Working)],
        &none
    ));
    assert!(!busy(&[], &none));
    assert!(!busy(
        &[agent(AgentGroup::Completed), agent(AgentGroup::NeedsInput)],
        &none
    ));
}

#[test]
fn a_running_codex_turn_or_agy_session_is_busy_with_no_claude_session() {
    let codex = Work {
        codex_turns: 1,
        ..Work::default()
    };
    let agy = Work {
        agy_sessions: 2,
        ..Work::default()
    };
    assert!(busy(&[], &codex));
    assert!(busy(&[agent(AgentGroup::Completed)], &agy));
}

#[test]
fn off_never_holds_whatever_is_going_on() {
    assert_eq!(want(setting(Mode::Off, false), || true), None);
    assert_eq!(want(setting(Mode::Off, true), || true), None);
}

#[test]
fn always_on_holds_even_when_nothing_works() {
    assert_eq!(want(setting(Mode::On, false), || false), Some(Kind::System));
    assert_eq!(want(setting(Mode::On, true), || false), Some(Kind::Display));
}

#[test]
fn while_working_holds_only_while_busy() {
    assert_eq!(
        want(setting(Mode::WhileWorking, false), || true),
        Some(Kind::System)
    );
    assert_eq!(
        want(setting(Mode::WhileWorking, true), || true),
        Some(Kind::Display)
    );
    assert_eq!(want(setting(Mode::WhileWorking, false), || false), None);
    assert_eq!(want(setting(Mode::WhileWorking, true), || false), None);
}

#[test]
fn the_work_is_asked_only_when_the_mode_depends_on_it() {
    let ask = |mode| {
        let mut asked = false;
        want(setting(mode, false), || {
            asked = true;
            true
        });
        asked
    };
    assert!(!ask(Mode::Off));
    assert!(!ask(Mode::On));
    assert!(ask(Mode::WhileWorking));
}

#[test]
fn modes_are_the_words_the_settings_use() {
    for mode in [Mode::Off, Mode::On, Mode::WhileWorking] {
        assert_eq!(Mode::parse(mode.as_str()), Some(mode));
    }
    assert_eq!(Mode::parse("on"), Some(Mode::On));
    assert_eq!(Mode::parse("whileWorking"), Some(Mode::WhileWorking));
    assert_eq!(Mode::parse("always"), None);
    assert_eq!(Mode::parse(""), None);
}

fn stored(values: &[(&str, serde_json::Value)]) -> (TempDir, Settings) {
    let dir = TempDir::new("awake-settings");
    let mut settings = Settings::open(&dir.path("settings.json")).unwrap();
    for (key, value) in values {
        settings.set(key, value.clone()).unwrap();
    }
    (dir, settings)
}

#[test]
fn nothing_stored_means_off_and_the_display_left_alone() {
    let (_dir, settings) = stored(&[]);
    assert_eq!(read(&settings), Setting::default());
    assert_eq!(read(&settings), setting(Mode::Off, false));
}

#[test]
fn the_stored_mode_and_display_are_read() {
    let (_dir, settings) = stored(&[
        ("mode", json!("whileWorking")),
        ("keepDisplayOn", json!(true)),
    ]);
    assert_eq!(read(&settings), setting(Mode::WhileWorking, true));
    let (_dir, settings) = stored(&[("mode", json!("on"))]);
    assert_eq!(read(&settings), setting(Mode::On, false));
}

#[test]
fn a_mode_nobody_knows_holds_nothing() {
    let (_dir, settings) = stored(&[]);
    let mut odd = settings;
    odd.set_unchecked("mode", json!("forever")).unwrap();
    assert_eq!(read(&odd).mode, Mode::Off);
}
