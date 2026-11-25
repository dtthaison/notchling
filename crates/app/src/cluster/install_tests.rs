use notchling_local_api::ops;
use serde_json::{json, Value};

use super::install::{applied_note, plan_of, Plan};
use super::model::ClusterModel;
use super::renders::{status, ME, STANDBY, WORKER};
use crate::welcome::facts::HookFacts;

fn set_member(status: &mut Value, id: &str, key: &str, value: Value) {
    let members = status["members"].as_array_mut().expect("members");
    let member = members.iter_mut().find(|m| m["id"] == id).expect("member");
    member[key] = value;
}

fn reply(on: bool, applied: bool, changes: Value, skills: Value) -> Value {
    json!({
        "on": on,
        "applied": applied,
        "message": "install is off: the Swift app owns hooks and skills until the maintainer turns this on",
        "plan": {
            "action": "install",
            "changed": true,
            "blocked": null,
            "hooks": {
                "file": "/home/x/.claude/settings.json",
                "state": "present",
                "read_only": false,
                "refused": null,
                "changed": true,
                "changes": changes,
                "notes": [],
                "present": [],
            },
            "skills": skills,
        },
    })
}

fn facts(value: &Value) -> HookFacts {
    HookFacts::parse(value).expect("a plan")
}

fn skill(name: &str, action: &str) -> Value {
    json!({"name": name, "tool": "claude", "path": "/x", "action": action, "reason": null, "error": null})
}

#[test]
fn headless_standby_row_offers_install_hook() {
    let model = model_with_headless_standby();
    let row = model.row(STANDBY).expect("standby");
    assert!(row.headless);
    assert!(row.offers_install_hook());
    let plan = model.install_plan(STANDBY).expect("offered");
    assert_eq!(plan.op, ops::INSTALL_PLAN);
    assert_eq!(plan.args, json!({"node": STANDBY}));
    let apply = model.install_apply(STANDBY).expect("offered");
    assert_eq!(apply.op, ops::INSTALL_APPLY);
    assert_eq!(apply.args, json!({"node": STANDBY}));
}

fn model_with_headless_standby() -> ClusterModel {
    let mut status = status();
    set_member(&mut status, STANDBY, "headless", json!(true));
    ClusterModel::from_status(&status, ME)
}

#[test]
fn install_hook_is_for_a_headless_online_node_from_the_primary() {
    let model = model_with_headless_standby();
    assert_eq!(model.install_plan(ME), None);
    assert_eq!(model.install_plan(WORKER), None);
    assert_eq!(model.install_apply(WORKER), None);
    assert_eq!(model.install_plan("0000000000000000"), None);

    let mut offline = status();
    set_member(&mut offline, STANDBY, "headless", json!(true));
    set_member(&mut offline, STANDBY, "online", json!(false));
    let model = ClusterModel::from_status(&offline, ME);
    assert_eq!(model.install_plan(STANDBY), None);

    let mut with_app = status();
    set_member(&mut with_app, STANDBY, "headless", json!(false));
    let model = ClusterModel::from_status(&with_app, ME);
    assert_eq!(model.install_plan(STANDBY), None);

    let mut elsewhere = status();
    set_member(&mut elsewhere, STANDBY, "headless", json!(true));
    let model = ClusterModel::from_status(&elsewhere, STANDBY);
    assert_eq!(model.install_plan(ME), None);
    assert_eq!(model.install_plan(WORKER), None);
}

#[test]
fn install_switch_toggle_keeps_acting_and_bridge() {
    let mut status = status();
    set_member(
        &mut status,
        STANDBY,
        "desired",
        json!({"acting": true, "bridge": false, "install": false}),
    );
    let model = ClusterModel::from_status(&status, ME);
    let on = model.toggle_install(STANDBY).expect("offered");
    assert_eq!(on.op, ops::CLUSTER_MEMBER_SET);
    assert_eq!(
        on.args["desired"],
        json!({"acting": true, "bridge": false, "install": true})
    );
    assert_eq!(model.toggle_install(ME), None);
}

#[test]
fn the_plan_is_shown_before_anything_is_applied() {
    let value = reply(
        true,
        false,
        json!([
            "add PreToolUse (*): 'notchling' hook --fence hook (timeout 30 s)",
            "add PostToolUse (Artifact): 'notchling' --hook hook (timeout 10 s)"
        ]),
        json!([
            skill("notchling-pages", "write"),
            skill("notchling-vault", "unchanged")
        ]),
    );
    match plan_of("mac-mini", &facts(&value)) {
        Plan::Confirm { title, lines } => {
            assert_eq!(title, "Install the Hook on mac-mini?");
            assert_eq!(
                lines[0],
                "This changes Claude Code's settings on mac-mini, not on this Mac:"
            );
            assert!(lines.iter().any(|l| l.contains("hook --fence")));
            assert!(lines.contains(&"Skills: notchling-pages".to_owned()));
            assert!(!lines.iter().any(|l| l.contains("notchling-vault")));
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn install_off_is_said_and_not_offered() {
    let value = reply(false, false, json!(["add PreToolUse (*): x"]), json!([]));
    match plan_of("mac-mini", &facts(&value)) {
        Plan::Say { text, problem } => {
            assert_eq!(
                text,
                "Install is off for mac-mini. Turn Install On in its menu, then try again."
            );
            assert!(problem);
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn nothing_to_change_and_refused_files_are_said() {
    let nothing = reply(
        true,
        false,
        json!([]),
        json!([skill("notchling-pages", "unchanged")]),
    );
    match plan_of("mac-mini", &facts(&nothing)) {
        Plan::Say { text, problem } => {
            assert_eq!(text, "The hook is already installed on mac-mini.");
            assert!(!problem);
        }
        other => panic!("{other:?}"),
    }
    let mut refused = reply(true, false, json!([]), json!([]));
    refused["plan"]["hooks"]["refused"] = json!("settings.json isn't JSON");
    match plan_of("mac-mini", &facts(&refused)) {
        Plan::Say { text, problem } => {
            assert_eq!(
                text,
                "The settings on mac-mini can't be edited: settings.json isn't JSON"
            );
            assert!(problem);
        }
        other => panic!("{other:?}"),
    }
    let mut blocked = reply(true, false, json!(["add PreToolUse (*): x"]), json!([]));
    blocked["plan"]["blocked"] = json!("the Swift app's hook is still in settings.json");
    match plan_of("mac-mini", &facts(&blocked)) {
        Plan::Say { text, problem } => {
            assert!(text.contains("the Swift app's hook is still in settings.json"));
            assert!(problem);
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn the_answer_to_apply_is_a_line() {
    let done = reply(true, true, json!(["add PreToolUse (*): x"]), json!([]));
    assert_eq!(
        applied_note("mac-mini", &facts(&done)),
        ("Installed the hook on mac-mini.".to_owned(), false)
    );
    let off = reply(false, false, json!(["add PreToolUse (*): x"]), json!([]));
    assert_eq!(
        applied_note("mac-mini", &facts(&off)),
        ("Nothing was written on mac-mini.".to_owned(), true)
    );
}
