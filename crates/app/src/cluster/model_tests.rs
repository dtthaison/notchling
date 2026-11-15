use notchling_local_api::ops;
use serde_json::{json, Value};

use super::model::{ClusterModel, Line};
use super::renders::{empty, model, status, ME, STANDBY, WORKER};
use super::wire;

fn set_member(status: &mut Value, id: &str, key: &str, value: Value) {
    let members = status["members"].as_array_mut().expect("members");
    let member = members.iter_mut().find(|m| m["id"] == id).expect("member");
    member[key] = value;
}

#[test]
fn status_maps_roles_and_online() {
    let rows = model(true).rows();
    let seen: Vec<(&str, &str, bool)> = rows
        .iter()
        .map(|r| (r.name.as_str(), r.role, r.online))
        .collect();
    assert_eq!(
        seen,
        [
            ("MacBook", "Primary", true),
            ("mac-mini", "Standby", true),
            ("build-box", "Worker", false),
        ]
    );
    for row in &rows {
        assert!(!row.role.contains("Home"));
        assert!(!row.state_word().contains("Home"));
    }
    assert_eq!(rows[0].state_word(), "Online");
    assert_eq!(rows[2].state_word(), "Offline");
}

#[test]
fn can_be_master_check_sends_member_set() {
    let model = model(true);
    let off = model.toggle_can_be_master(STANDBY).expect("standby row");
    assert_eq!(off.op, ops::CLUSTER_MEMBER_SET);
    assert_eq!(off.args, json!({"node": STANDBY, "canBeMaster": false}));
    let on = model.toggle_can_be_master(WORKER).expect("worker row");
    assert_eq!(on.args, json!({"node": WORKER, "canBeMaster": true}));
    assert!(model.row(STANDBY).expect("row").can_be_master);
    assert!(!model.row(WORKER).expect("row").can_be_master);
    assert_eq!(model.toggle_can_be_master("0000000000000000"), None);
}

#[test]
fn compat_warning_shows_on_row() {
    let model = model(true);
    let worker = model.row(WORKER).expect("worker");
    assert_eq!(worker.warnings.len(), 1, "{:?}", worker.warnings);
    assert!(
        worker.warnings[0].contains("0.166.0"),
        "{:?}",
        worker.warnings
    );
    assert!(
        worker.warnings[0].contains("0.170.0"),
        "{:?}",
        worker.warnings
    );
    assert!(model.row(STANDBY).expect("standby").warnings.is_empty());
    assert!(model.row(ME).expect("me").warnings.is_empty());
}

#[test]
fn no_cluster_offers_create() {
    let none = ClusterModel::from_status(&empty(), ME);
    assert!(none.can_create());
    assert!(none.rows().is_empty());
    assert!(!none.can_copy_token());
    assert!(!model(true).can_create());
    assert!(model(true).can_copy_token());
    assert!(!ClusterModel::default().can_create());
    assert_eq!(wire::create("  Studio  ").args, json!({"name": "Studio"}));
}

#[test]
fn this_mac_is_marked() {
    let model = model(true);
    let rows = model.rows();
    let marked: Vec<&str> = rows
        .iter()
        .filter(|r| r.me)
        .map(|r| r.id.as_str())
        .collect();
    assert_eq!(marked, [ME]);
    assert!(model.row(ME).expect("me").me);
    assert_eq!(model.me(), ME);
    assert!(!model.row(STANDBY).expect("row").me);
}

#[test]
fn acting_toggle_keeps_bridge_and_install() {
    let mut status = status();
    set_member(
        &mut status,
        WORKER,
        "desired",
        json!({"acting": false, "bridge": true, "install": true}),
    );
    let model = ClusterModel::from_status(&status, ME);
    let request = model.toggle_acting(WORKER).expect("worker row");
    assert_eq!(request.op, ops::CLUSTER_MEMBER_SET);
    assert_eq!(
        request.args,
        json!({
            "node": WORKER,
            "desired": {"acting": true, "bridge": true, "install": true},
        })
    );
    let standby = model.toggle_acting(STANDBY).expect("standby row");
    assert_eq!(
        standby.args["desired"],
        json!({"acting": true, "bridge": false, "install": false})
    );
}

#[test]
fn acting_is_not_offered_for_this_mac_or_a_client() {
    let mut status = status();
    set_member(&mut status, WORKER, "client", json!(true));
    let model = ClusterModel::from_status(&status, ME);
    assert_eq!(model.toggle_acting(ME), None);
    assert_eq!(model.toggle_acting(WORKER), None);
    assert!(!model.row(ME).expect("me").offers_acting());
    assert!(!model.row(WORKER).expect("worker").offers_acting());
    assert!(model.row(WORKER).expect("worker").offers_menu());
    assert!(model.row(STANDBY).expect("standby").offers_acting());
}

#[test]
fn only_an_active_member_has_a_menu_and_the_primary_cannot_be_removed() {
    let mut status = status();
    set_member(&mut status, WORKER, "state", json!("pending"));
    let model = ClusterModel::from_status(&status, ME);
    let pending = model.row(WORKER).expect("pending");
    assert!(!pending.offers_menu() && !pending.offers_remove());
    assert_eq!(model.remove(WORKER), None);
    assert_eq!(model.toggle_can_be_master(WORKER), None);
    assert_eq!(model.toggle_acting(WORKER), None);
    let from_the_worker = ClusterModel::from_status(&super::renders::status(), WORKER);
    assert_eq!(
        from_the_worker.remove(ME),
        None,
        "the primary is never removed"
    );
    assert!(!from_the_worker.row(ME).expect("primary").offers_remove());
    assert!(from_the_worker.row(ME).expect("primary").offers_menu());
}

#[test]
fn a_node_with_another_schema_shows_the_schema_text() {
    let mut status = status();
    let mut versions = status["members"][1]["versions"].clone();
    versions["schema"] = json!(2);
    set_member(&mut status, STANDBY, "versions", versions);
    let model = ClusterModel::from_status(&status, ME);
    let warnings = &model.row(STANDBY).expect("standby").warnings;
    assert!(
        warnings
            .iter()
            .any(|w| w == "mac-mini has cluster schema 2, this Mac 1"),
        "{warnings:?}"
    );
}

#[test]
fn a_node_with_no_tools_and_another_notchling_version_says_so() {
    let mut status = status();
    let mut versions = status["members"][0]["versions"].clone();
    versions["tools"] = json!({});
    versions["notchling"] = json!("0.8.0");
    set_member(&mut status, WORKER, "versions", versions);
    let model = ClusterModel::from_status(&status, ME);
    let warnings = &model.row(WORKER).expect("worker").warnings;
    assert_eq!(
        warnings,
        &["build-box runs Notchling 0.8.0, the Mac 0.9.0".to_owned()]
    );
}

#[test]
fn a_node_that_takes_no_dispatched_jobs_says_so() {
    let mut status = status();
    let mut versions = status["members"][1]["versions"].clone();
    versions["features"] = json!([]);
    set_member(&mut status, STANDBY, "versions", versions);
    let model = ClusterModel::from_status(&status, ME);
    let warnings = &model.row(STANDBY).expect("standby").warnings;
    assert_eq!(
        warnings,
        &["mac-mini doesn't take dispatched jobs".to_owned()]
    );
}

#[test]
fn remove_is_not_offered_for_this_mac_or_a_removed_member() {
    let mut status = status();
    set_member(&mut status, WORKER, "state", json!("removed"));
    let model = ClusterModel::from_status(&status, ME);
    assert_eq!(model.remove(ME), None);
    assert_eq!(model.remove(WORKER), None);
    assert_eq!(model.toggle_can_be_master(WORKER), None);
    assert_eq!(model.toggle_acting(WORKER), None);
    assert!(!model.row(WORKER).expect("row").offers_menu());
    assert_eq!(model.row(WORKER).expect("row").state_word(), "Removed");
    let request = model.remove(STANDBY).expect("standby row");
    assert_eq!(request.op, ops::CLUSTER_REMOVE);
    assert_eq!(request.args, json!({"node": STANDBY}));
    assert_eq!(
        model.rows().last().map(|r| r.id.as_str()),
        Some(WORKER),
        "removed members come last"
    );
}

#[test]
fn the_connection_line_says_why_the_cluster_is_not_running() {
    let mut status = status();
    assert_eq!(
        ClusterModel::from_status(&status, ME).line(),
        Line::Connected
    );
    status["listening"] = Value::Null;
    assert_eq!(
        ClusterModel::from_status(&status, ME).line(),
        Line::NotListening
    );
    status["tailscale"] = json!(false);
    let model = ClusterModel::from_status(&status, ME);
    assert_eq!(model.line().text(), "No Tailscale Address");
    status["link"] = json!(false);
    let off = ClusterModel::from_status(&status, ME);
    assert_eq!(off.line().text(), "Not Connected");
    assert!(off.can_turn_on());
    assert!(!model.can_turn_on());
    status["error"] = json!("listening on port 47823 failed: in use");
    let broken = ClusterModel::from_status(&status, ME);
    assert_eq!(
        broken.line(),
        Line::Error("listening on port 47823 failed: in use".into())
    );
    assert!(broken.line().is_problem());
}

#[test]
fn turning_on_asks_for_the_cluster_switch_only() {
    let request = wire::turn_on();
    assert_eq!(request.op, ops::SETTINGS_SET_SWITCH);
    assert_eq!(request.args, json!({"switch": "cluster", "on": true}));
}

#[test]
fn load_tools_and_os_are_read_from_the_member() {
    let model = model(true);
    let me = model.row(ME).expect("me");
    assert_eq!(me.cpu, Some(31));
    assert_eq!(me.mem, Some((18432, 32768)));
    assert_eq!(
        me.load_text().as_deref(),
        Some("CPU 31% · Memory 18 of 32 GB")
    );
    assert_eq!(me.os, "macOS aarch64");
    assert_eq!(
        me.tool_texts(),
        ["Claude Code 2.1.320", "Codex 0.170.0", "Antigravity 1.5.1"]
    );
    let worker = model.row(WORKER).expect("worker");
    assert_eq!(worker.load_text(), None);
    assert_eq!(worker.os, "Linux x86_64");
    assert_eq!(worker.tool_texts()[0], "Claude Code not installed");
}

#[test]
fn a_token_is_read_from_the_reply_and_nothing_else_is_kept() {
    assert_eq!(
        wire::token_of(&json!({"token": "nlj1.abc"})).as_deref(),
        Some("nlj1.abc")
    );
    assert_eq!(wire::token_of(&json!({"token": ""})), None);
    assert_eq!(wire::token_of(&json!({})), None);
}
