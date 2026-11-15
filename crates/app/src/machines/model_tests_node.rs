use std::sync::Arc;

use notchling_engine::machines::registry::NodeSettings;
use notchling_engine::machines::step::{ClusterTokens, StepId};
use serde_json::{json, Value};

use super::cluster_feed::{can_be_master_in, cluster_id_in, token_in};
use super::model_tests::{lock, mac, scripted_world};

struct FromStatus(Value);

impl ClusterTokens for FromStatus {
    fn join_token(&self) -> Result<String, String> {
        Err("not asked".into())
    }
    fn cluster_id(&self) -> Option<String> {
        cluster_id_in(&self.0)
    }
    fn can_be_master(&self, machine: &str) -> Option<bool> {
        can_be_master_in(&self.0, machine, None)
    }
    fn can_be_master_of(&self, machine: &str, node: Option<&str>) -> Option<bool> {
        can_be_master_in(&self.0, machine, node)
    }
}

const NODE_A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const NODE_B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

fn status() -> Value {
    json!({
        "cluster": {"id": "c1", "name": "home", "term": 1, "home": NODE_A},
        "me": NODE_A,
        "members": [
            {"id": NODE_A, "name": "laptop", "role": "home", "canBeMaster": true},
            {"id": NODE_B, "name": "m1", "role": "worker", "canBeMaster": false},
        ],
    })
}

#[test]
fn node_toggle_sets_settings() {
    let w = scripted_world(vec![mac("m1"), mac("m2")]);
    let mut model = w.model;
    let m1 = model.machine("m1").unwrap();
    assert!(!model.node_on(&m1));
    assert!(!model
        .engine()
        .steps(&m1)
        .iter()
        .any(|s| s.id == StepId::NODE_JOINED));

    model.set_node("m1", true);
    assert_eq!(
        lock(&w.registry.list)[0].node,
        Some(NodeSettings::default())
    );
    assert_eq!(lock(&w.registry.list)[1].node, None);
    let m1 = model.machine("m1").unwrap();
    assert!(model.node_on(&m1));
    let ids: Vec<StepId> = model
        .engine()
        .steps(&m1)
        .into_iter()
        .map(|s| s.id)
        .collect();
    for id in [
        StepId::NODE_TAILSCALE,
        StepId::NODE_BINARY,
        StepId::NODE_SERVICE,
        StepId::NODE_JOINED,
    ] {
        assert!(ids.contains(&id), "{}", id.raw());
    }
    assert!(model.can_be_master_line(&m1).is_some());

    model.set_node("m1", false);
    assert_eq!(lock(&w.registry.list)[0].node, None);
    let m1 = model.machine("m1").unwrap();
    assert!(!model.node_on(&m1));
    assert!(model.can_be_master_line(&m1).is_none());
}

#[test]
fn can_be_master_line_reads_cluster_status() {
    let w = scripted_world(vec![mac("m1"), mac("m2")]);
    let mut model = w.model;
    model.parts.cluster = Arc::new(FromStatus(status()));
    model.set_node("m1", true);
    model.set_node("m2", true);
    let m1 = model.machine("m1").unwrap();
    let m2 = model.machine("m2").unwrap();
    assert_eq!(
        model.can_be_master_line(&m1).as_deref(),
        Some("Can Be Master: Not in the cluster yet")
    );

    model.ask_cluster();
    assert_eq!(
        model.can_be_master_line(&m1).as_deref(),
        Some("Can Be Master: No")
    );
    assert_eq!(
        model.can_be_master_line(&m2).as_deref(),
        Some("Can Be Master: Not in the cluster yet")
    );

    model.parts.cluster = Arc::new(FromStatus(json!({
        "cluster": {"id": "c1"},
        "members": [{"id": NODE_B, "name": "m1", "canBeMaster": true}],
    })));
    model.ask_cluster();
    assert_eq!(
        model.can_be_master_line(&m1).as_deref(),
        Some("Can Be Master: Yes")
    );
}

#[test]
fn a_machine_is_matched_by_node_id_before_name() {
    let s = status();
    assert_eq!(can_be_master_in(&s, "m1", None), Some(false));
    assert_eq!(can_be_master_in(&s, "m1", Some(NODE_A)), Some(true));
    assert_eq!(can_be_master_in(&s, "other", Some(NODE_B)), Some(false));
    assert_eq!(can_be_master_in(&s, "other", None), None);
    assert_eq!(can_be_master_in(&s, "m1", Some("cccc")), Some(false));
    assert_eq!(
        can_be_master_in(&json!({"cluster": null, "members": []}), "m1", None),
        None
    );
}

#[test]
fn status_and_token_replies_are_read_plainly() {
    assert_eq!(cluster_id_in(&status()).as_deref(), Some("c1"));
    assert_eq!(
        cluster_id_in(&json!({"cluster": null, "members": []})),
        None
    );
    assert_eq!(
        token_in(&json!({"token": "nlj1.x"})).as_deref(),
        Some("nlj1.x")
    );
    assert_eq!(token_in(&json!({"token": ""})), None);
    assert_eq!(token_in(&json!({})), None);
}
