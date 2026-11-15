use serde_json::Value;

use super::core_tests::Hub;
use super::{Effect, Input, LocalCmd};
use crate::cluster::join::JoinToken;
use crate::cluster::testkit::{fp, member};
use crate::cluster::NodeId;

fn pending_joiner() -> (Hub, NodeId, NodeId) {
    let mut hub = Hub::new();
    let m5 = hub.add("m5", "macos", true);
    let w1 = hub.add("w1", "linux", false);
    hub.create("m5");
    hub.local(
        &m5,
        LocalCmd::IssueToken {
            secret: "hush-secret".to_string(),
            minter: m5.clone(),
        },
    );
    hub.tick(&w1, 5_000);
    let token = JoinToken {
        cluster: "c1".to_string(),
        home: m5.clone(),
        fingerprint: fp("m5"),
        addrs: vec!["100.64.0.5".to_string()],
        port: 47823,
        secret: "hush-secret".to_string(),
    };
    hub.local(
        &w1,
        LocalCmd::JoinWith {
            token,
            me: member("w1", "linux", false),
        },
    );
    (hub, m5, w1)
}

fn link(hub: &mut Hub, a: &NodeId, b: &NodeId) {
    hub.linked.push((a.clone(), b.clone()));
    hub.run(a, Input::Linked { peer: b.clone() });
    hub.run(b, Input::Linked { peer: a.clone() });
}

#[test]
fn status_reports_joining_while_pending() {
    let (mut hub, m5, w1) = pending_joiner();
    let status = hub.core(&w1).status();
    assert_eq!(status["cluster"], Value::Null);
    let joining = &status["joining"];
    assert_eq!(joining["since"], 5_000);
    assert_eq!(joining["cluster"], "c1");
    assert_eq!(joining["primary"], serde_json::json!(m5));
    assert_eq!(joining["answered"], false);
    assert!(!status.to_string().contains("hush-secret"));
    link(&mut hub, &m5, &w1);
    let asked = hub.core(&w1).status();
    assert_eq!(asked["joining"]["answered"], true);
    assert_eq!(asked["joining"]["since"], 5_000);
    assert!(!asked.to_string().contains("hush-secret"));
}

#[test]
fn joining_ends_with_the_join_and_is_null_otherwise() {
    let (mut hub, m5, w1) = pending_joiner();
    assert_eq!(hub.core(&m5).status()["joining"], Value::Null);
    link(&mut hub, &m5, &w1);
    hub.local(
        &m5,
        LocalCmd::Approve {
            node: w1.clone(),
            client: false,
            can_be_master: false,
        },
    );
    let status = hub.core(&w1).status();
    assert_eq!(status["joining"], Value::Null);
    assert_ne!(status["cluster"], Value::Null);
}

#[test]
fn joining_change_is_published() {
    let (mut hub, m5, w1) = pending_joiner();
    link(&mut hub, &m5, &w1);
    hub.tick(&w1, 1);
    assert!(hub.seen.iter().any(|(n, e)| n == "w1"
        && matches!(e, Effect::Publish { name, data, .. }
            if name == "cluster.changed" && data["joining"]["answered"] == true)));
}
