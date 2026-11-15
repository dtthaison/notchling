use notchling_locale as t;
use serde_json::{json, Value};

use super::skills_rows::*;

fn skill(name: &str, tool: &str, action: &str, reason: Option<&str>) -> Value {
    json!({"name": name, "tool": tool, "action": action, "reason": reason,
        "tool_missing": reason == Some("the tool is not installed here"), "path": "/x", "error": null})
}

fn reply(on: bool, skills: Vec<Value>) -> Result<Value, String> {
    Ok(json!({"on": on, "applied": false, "plan": {"skills": skills}}))
}

#[test]
fn each_plan_action_has_one_state_and_a_removal_has_none() {
    use SkillState::*;
    assert_eq!(SkillState::of_plan("unchanged", false), Some(Installed));
    assert_eq!(SkillState::of_plan("update", false), Some(OutOfDate));
    assert_eq!(SkillState::of_plan("write", false), Some(NotInstalled));
    assert_eq!(SkillState::of_plan("skip", true), Some(ToolMissing));
    assert_eq!(SkillState::of_plan("skip", false), Some(LeftAlone));
    assert_eq!(SkillState::of_plan("remove", false), None);
    assert_eq!(SkillState::of_plan("absent", false), None);
    assert_eq!(SkillState::Installed.word(), t::settings::skill_installed());
    assert_eq!(SkillState::OutOfDate.word(), "Out of date");
    assert_eq!(SkillState::NotInstalled.word(), "Not installed");
    assert_eq!(SkillState::ToolMissing.word(), "Tool not installed");
}

#[test]
fn skills_group_under_their_tool_in_a_fixed_order() {
    let shown = SkillsShown::from_reply(reply(
        true,
        vec![
            skill("notchling-delegate", "antigravity", "write", None),
            skill("notchling-delegate", "claude", "unchanged", None),
            skill("notchling-pages", "claude", "update", None),
            skill(
                "notchling-delegate",
                "codex",
                "skip",
                Some("the tool is not installed here"),
            ),
            skill("notchling-odd", "mystery", "write", None),
            skill("notchling-removal", "claude", "remove", None),
        ],
    ));
    let SkillsShown::Found { tools, install_on } = &shown else {
        panic!("{shown:?}");
    };
    assert!(install_on);
    let order: Vec<_> = tools.iter().map(|g| g.tool.id()).collect();
    assert_eq!(order, ["claude", "codex", "antigravity"]);
    let claude: Vec<_> = tools[0]
        .lines
        .iter()
        .map(|l| (l.name.as_str(), l.state))
        .collect();
    assert_eq!(
        claude,
        [
            ("notchling-delegate", SkillState::Installed),
            ("notchling-pages", SkillState::OutOfDate)
        ]
    );
    assert_eq!(tools[1].lines[0].state, SkillState::ToolMissing);
    assert_eq!(
        shown.names(),
        ["notchling-delegate", "notchling-pages"],
        "each name once, for the reinstall"
    );
    assert!(shown.can_reinstall());
    assert_eq!(shown.status(), None);
}

#[test]
fn a_tool_with_no_skill_in_the_plan_is_not_listed() {
    let shown = SkillsShown::from_reply(reply(
        true,
        vec![skill("notchling-pages", "claude", "write", None)],
    ));
    let SkillsShown::Found { tools, .. } = shown else {
        panic!();
    };
    assert_eq!(tools.len(), 1);
}

#[test]
fn with_the_switch_off_the_button_is_off_and_the_page_says_why() {
    let shown = SkillsShown::from_reply(reply(
        false,
        vec![skill("notchling-pages", "claude", "write", None)],
    ));
    assert!(!shown.can_reinstall());
    assert_eq!(shown.switch_note(), Some(t::settings::skills_switch_off()));
}

#[test]
fn nothing_to_list_means_no_button() {
    assert!(!SkillsShown::from_reply(reply(true, vec![])).can_reinstall());
    assert!(!SkillsShown::Checking.can_reinstall());
    assert!(SkillsShown::Checking.names().is_empty());
}

#[test]
fn failures_are_lines_and_a_detached_daemon_is_its_own_state() {
    assert_eq!(
        SkillsShown::from_reply(Err(crate::acting::DETACHED_MESSAGE.to_owned())),
        SkillsShown::Detached
    );
    assert_eq!(
        SkillsShown::Detached.status().as_deref(),
        Some(t::settings::skills_detached())
    );
    assert_eq!(
        SkillsShown::from_reply(Err("no path".into())),
        SkillsShown::Failed("no path".into())
    );
    assert_eq!(
        SkillsShown::from_reply(Ok(json!({"hello": 1}))),
        SkillsShown::Failed(t::settings::reply_unreadable().to_owned())
    );
    assert_eq!(
        SkillsShown::Checking.status().as_deref(),
        Some(t::settings::skills_checking())
    );
}

#[test]
fn a_reinstall_that_wrote_says_so_and_a_refused_one_says_the_switch_is_off() {
    assert_eq!(
        reinstall_result(Ok(json!({"on": true, "applied": true}))),
        Ok(t::settings::skills_reinstalled())
    );
    assert_eq!(
        reinstall_result(Ok(json!({"on": false, "applied": false}))),
        Err(t::settings::skills_switch_off().to_owned())
    );
    assert_eq!(
        reinstall_result(Err("a skill failed".into())),
        Err("a skill failed".to_owned())
    );
}
