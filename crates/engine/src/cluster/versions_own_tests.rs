use serde_json::json;

use super::testkit::versions;
use super::versions::{compat, Compat, Purpose, Versions};
use super::Tool;
use crate::versions as vs;

#[test]
fn an_older_report_has_no_own_logins() {
    let old = json!({
        "notchling": "0.1.0",
        "proto": 1,
        "features": ["dispatch"],
        "schema": 1,
        "os": "macos",
        "arch": "arm64",
        "tools": {"claude": "2.1.324"},
        "logins": {"codex": true},
    });
    let parsed: Versions = serde_json::from_value(old).expect("an older report");
    assert!(parsed.own_logins.is_empty());
    assert_eq!(parsed.logins.get(&vs::Tool::Codex), Some(&true));
}

#[test]
fn own_logins_cross_the_wire_and_an_empty_map_is_not_sent() {
    let mut node = versions("macos");
    assert!(serde_json::to_value(&node)
        .expect("json")
        .get("own_logins")
        .is_none());
    node.own_logins.insert(vs::Tool::Claude, true);
    let wire = serde_json::to_value(&node).expect("json");
    assert_eq!(wire["own_logins"], json!({"claude": true}));
    let back: Versions = serde_json::from_value(wire).expect("round trip");
    assert_eq!(back, node);
}

#[test]
fn a_report_with_a_field_this_build_does_not_know_still_reads() {
    let mut wire = serde_json::to_value(versions("linux")).expect("json");
    wire["something_newer"] = json!({"x": 1});
    assert!(serde_json::from_value::<Versions>(wire).is_ok());
}

#[test]
fn own_logins_never_change_a_dispatch_decision() {
    let mac = versions("macos");
    let mut node = versions("macos");
    let plain = compat(&mac, &node, "m1", Purpose::Dispatch(Tool::Claude));
    node.own_logins.insert(vs::Tool::Claude, true);
    assert_eq!(
        compat(&mac, &node, "m1", Purpose::Dispatch(Tool::Claude)),
        plain
    );
    node.own_logins.insert(vs::Tool::Claude, false);
    assert_eq!(
        compat(&mac, &node, "m1", Purpose::Dispatch(Tool::Claude)),
        Compat::Ok
    );
}
