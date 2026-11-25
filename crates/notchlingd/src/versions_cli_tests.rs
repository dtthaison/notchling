use super::*;
use crate::acting_ops_tests::FakeRoom;
use crate::consent_ops::ConsentOps;
use crate::consent_testkit::{agent_caller, consent_at};
use crate::inbox_ops::InboxOps;
use crate::limits_ops_tests::TempDir;
use crate::op_group::OpGroup;
use crate::settings_cli;
use crate::settings_ops::{SettingsOps, SwitchStates};
use notchling_engine::consent::SETTING;
use notchling_events::MemorySink;
use std::sync::Arc;

fn scanned(list: &[&str]) -> Scanned {
    let raw: Vec<String> = list.iter().map(|s| s.to_string()).collect();
    scan(&raw, USAGE, &["--to", "--worker"]).unwrap()
}

fn words(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| s.to_string()).collect()
}

#[test]
fn a_mac_update_asks_versions_request() {
    let (op, body) = request(&scanned(&["claude", "--reason", "fixes", "--key", "k"])).unwrap();
    assert_eq!(op, ops::VERSIONS_REQUEST);
    assert_eq!(
        body,
        json!({"tool": "claude", "consent": {"reason": "fixes", "key": "k"}})
    );
    let (_, back) = request(&scanned(&["claude", "--to", "2.1.323", "--dry-run"])).unwrap();
    assert_eq!(back["to"], "2.1.323");
    assert_eq!(back["dryRun"], true);
}

#[test]
fn a_worker_update_is_handed_to_machines_request() {
    let (op, body) = request(&scanned(&["codex", "--worker", "build", "--json"])).unwrap();
    assert_eq!(op, ops::MACHINES_REQUEST);
    assert_eq!(body["action"], "update");
    assert_eq!(body["args"], json!({"machine": "build", "tool": "codex"}));
    assert!(body.get("tool").is_none());
}

#[test]
fn an_update_needs_exactly_one_tool() {
    assert!(request(&scanned(&[])).is_err());
    assert!(request(&scanned(&["claude", "codex"])).is_err());
}

#[test]
fn check_updates_maps_onto_the_settings_commands() {
    assert_eq!(
        settings_args(&words(&["on"])).unwrap(),
        words(&["set", "checkForUpdates", "on"])
    );
    assert_eq!(
        settings_args(&words(&["off", "--reason", "quiet", "--no-wait"])).unwrap(),
        words(&[
            "set",
            "checkForUpdates",
            "off",
            "--reason",
            "quiet",
            "--no-wait"
        ])
    );
    assert_eq!(
        settings_args(&words(&["--json"])).unwrap(),
        words(&["get", "checkForUpdates", "--json"])
    );
    assert!(settings_args(&words(&["maybe"])).is_err());
    assert!(settings_args(&words(&["on", "off"])).is_err());
    let parsed = settings_cli::parse(&settings_args(&words(&["on"])).unwrap()).unwrap();
    assert_eq!(
        parsed.sub,
        settings_cli::Sub::Set("checkForUpdates".into(), "on".into())
    );
}

#[test]
fn check_updates_on_opens_a_setting_request_on_check_for_updates() {
    let tmp = TempDir::new("check-updates");
    let room = Arc::new(FakeRoom::default());
    let inbox = InboxOps::new(tmp.path("inbox"), room.clone());
    let consent: Arc<ConsentOps> = consent_at(
        room.clone(),
        inbox,
        Arc::new(MemorySink::new()),
        Arc::new(crate::acting_ops::now_ms),
    );
    let states: SwitchStates = Arc::new(|_: &str| None);
    let settings = SettingsOps::new(tmp.path("data"), room, states).with_consent(consent.clone());
    consent.register(SETTING, settings.writer());
    let forwarded = settings_args(&words(&["on"])).unwrap();
    let parsed = settings_cli::parse(&forwarded).unwrap();
    let settings_cli::Sub::Set(key, value) = parsed.sub else {
        panic!("check-updates on is a settings set");
    };
    let reply = settings
        .call(
            &agent_caller(),
            ops::SETTINGS_SET,
            &json!({"key": key, "value": value, "wait": 0, "consent": {}}),
        )
        .unwrap();
    assert_eq!(reply["state"], "waiting", "{reply}");
    let id = reply["request"]["id"].as_str().unwrap();
    let row = consent.get_row(id).unwrap();
    assert_eq!(row.kind, SETTING);
    assert_eq!(row.target, "checkForUpdates");
}
