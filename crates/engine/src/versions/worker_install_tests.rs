use super::worker_install::{install_plan, verdict, AGY_INSTALL_URL};
use super::{Method, Tool, ToolState, Version};

fn v(s: &str) -> Version {
    Version::parse(s).unwrap()
}

fn on(version: Option<&str>) -> ToolState {
    ToolState::Installed {
        version: version.map(v),
        method: Method::Standalone,
        path: "/home/w/.local/bin/x".into(),
    }
}

#[test]
fn codex_pins_the_installer_and_npm_to_the_macs_version() {
    let plan = install_plan(
        "m1",
        Tool::Codex,
        &ToolState::NotInstalled,
        Some(&v("0.166.0")),
    )
    .unwrap();
    assert_eq!(plan.target, Some(v("0.166.0")));
    assert!(plan.note.is_none());
    assert!(plan.script.contains("https://chatgpt.com/codex/install.sh"));
    assert!(
        plan.script.contains("sh -s -- --release '0.166.0'"),
        "{}",
        plan.script
    );
    assert!(
        plan.script
            .contains("npm install -g '@openai/codex@0.166.0'"),
        "{}",
        plan.script
    );
    assert!(!plan.script.contains("sudo"));
}

#[test]
fn codex_without_a_checked_version_installs_the_newest_and_says_so() {
    let plan = install_plan("m1", Tool::Codex, &ToolState::NotInstalled, None).unwrap();
    assert_eq!(plan.target, None);
    assert!(!plan.script.contains("--release"));
    assert!(plan.script.contains("@openai/codex@latest"));
    assert!(plan.note.unwrap().contains("newest"));
}

#[test]
fn antigravity_cannot_pin_and_the_plan_says_so() {
    let plan = install_plan("m1", Tool::Agy, &ToolState::NotInstalled, Some(&v("1.5.9"))).unwrap();
    assert_eq!(plan.target, None);
    assert!(plan.script.contains(AGY_INSTALL_URL));
    assert!(!plan.script.contains("1.5.9"));
    let note = plan.note.unwrap();
    assert!(note.contains("no exact version"), "{note}");
    assert!(note.contains("1.5.9"), "{note}");
    assert!(note.contains("m1"), "{note}");
}

#[test]
fn only_a_missing_codex_or_antigravity_is_installed() {
    let installed = install_plan("m1", Tool::Codex, &on(Some("0.118.0")), Some(&v("0.166.0")));
    assert!(installed.unwrap_err().contains("already installed"));
    for tool in [Tool::Claude, Tool::Notchling] {
        let why = install_plan("m1", tool, &ToolState::NotInstalled, Some(&v("1.0.0")));
        assert!(why.unwrap_err().contains("doesn't install"));
    }
}

#[test]
fn a_version_that_is_not_three_plain_numbers_never_reaches_a_command() {
    for bad in ["0.166.0-alpha.1", "0.166.0-rc", "1.2.3-4"] {
        let why =
            install_plan("m1", Tool::Codex, &ToolState::NotInstalled, Some(&v(bad))).unwrap_err();
        assert!(why.contains("isn't a version"), "{why}");
        assert!(why.contains("m1"), "{why}");
    }
}

#[test]
fn a_pinned_install_succeeds_only_on_the_target_version() {
    let want = v("0.166.0");
    let ok = verdict("m1", Tool::Codex, Some(&want), Some(&on(Some("0.166.0"))));
    assert_eq!(ok.unwrap(), "Codex 0.166.0 is installed on m1");
    let other = verdict("m1", Tool::Codex, Some(&want), Some(&on(Some("0.165.0"))));
    assert!(other.unwrap_err().contains("reports 0.165.0, not 0.166.0"));
    let unread = verdict("m1", Tool::Codex, Some(&want), Some(&on(None)));
    assert!(unread.unwrap_err().contains("couldn't be read"));
    let missing = verdict(
        "m1",
        Tool::Codex,
        Some(&want),
        Some(&ToolState::NotInstalled),
    );
    assert!(missing.unwrap_err().contains("still isn't found"));
    assert!(verdict("m1", Tool::Codex, Some(&want), None).is_err());
}

#[test]
fn an_unpinned_install_succeeds_when_the_tool_is_found() {
    assert!(verdict("m1", Tool::Agy, None, Some(&on(None))).is_ok());
    assert!(verdict("m1", Tool::Agy, None, Some(&ToolState::NotInstalled)).is_err());
    let text = verdict("m1", Tool::Codex, None, Some(&on(Some("0.170.0")))).unwrap();
    assert!(text.contains("0.170.0"));
}
