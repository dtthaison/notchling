use std::sync::Arc;

use notchling_engine::machines::registry::NodeSettings;
use notchling_engine::machines::step::{ClusterTokens, StepId};
use serde_json::{json, Value};

use super::cluster_feed::{can_be_master_in, cluster_id_in, token_in};
use super::cluster_feed::{DaemonTokens, NOT_RUNNING};
use super::model::MachinesModel;
use super::model_tests::{lock, mac, scripted_world};

fn ask(model: &mut MachinesModel, nodes: &[(&str, &str)]) {
    let nodes: Vec<(String, String)> = nodes
        .iter()
        .map(|(n, id)| (n.to_string(), id.to_string()))
        .collect();
    let answer = model
        .cluster_ask_with(|name| {
            nodes
                .iter()
                .find(|(n, _)| n == name)
                .map(|(_, id)| id.clone())
        })
        .run();
    model.set_masters(answer);
}

struct FromStatus(Value);

impl ClusterTokens for FromStatus {
    fn join_token(&self) -> Result<String, String> {
        Err("not asked".into())
    }
    fn cluster_id(&self) -> Result<Option<String>, String> {
        Ok(cluster_id_in(&self.0))
    }
    fn can_be_master(&self, _machine: &str) -> Option<bool> {
        None
    }
    fn can_be_master_of(&self, _machine: &str, node: Option<&str>) -> Option<bool> {
        can_be_master_in(&self.0, node?)
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

    assert!(model.set_node("m1", true), "turning it on asks for a check");
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

    assert!(
        !model.set_node("m1", false),
        "turning it off checks nothing"
    );
    assert!(
        !model.set_node("nobody", true),
        "an unknown machine checks nothing"
    );
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

    ask(&mut model, &[("m1", NODE_B)]);
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
    ask(&mut model, &[("m1", NODE_B)]);
    assert_eq!(
        model.can_be_master_line(&m1).as_deref(),
        Some("Can Be Master: Yes")
    );
}

#[test]
fn a_machine_with_no_node_id_is_never_matched_by_name() {
    let w = scripted_world(vec![mac("m1")]);
    let mut model = w.model;
    model.parts.cluster = Arc::new(FromStatus(status()));
    model.set_node("m1", true);
    ask(&mut model, &[]);
    let m1 = model.machine("m1").unwrap();
    assert_eq!(
        model.can_be_master_line(&m1).as_deref(),
        Some("Can Be Master: Not in the cluster yet")
    );
}

#[test]
fn a_daemon_that_is_down_is_an_error_not_no_cluster() {
    let tokens = DaemonTokens::new(std::env::temp_dir().join("notchling-no-such-daemon.sock"));
    assert_eq!(tokens.cluster_id(), Err(NOT_RUNNING.to_string()));
    assert_eq!(tokens.cluster_id(), Err(NOT_RUNNING.to_string()));
    assert_eq!(tokens.can_be_master_of("m1", Some(NODE_A)), None);
}

#[test]
fn a_machine_is_matched_by_node_id_only() {
    let s = status();
    assert_eq!(can_be_master_in(&s, NODE_A), Some(true));
    assert_eq!(can_be_master_in(&s, NODE_B), Some(false));
    assert_eq!(can_be_master_in(&s, "cccc"), None);
    assert_eq!(
        can_be_master_in(&json!({"cluster": null, "members": []}), NODE_A),
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

struct Counting(std::sync::atomic::AtomicUsize);

impl ClusterTokens for Counting {
    fn join_token(&self) -> Result<String, String> {
        Err("not asked".into())
    }
    fn cluster_id(&self) -> Result<Option<String>, String> {
        Ok(None)
    }
    fn can_be_master(&self, _machine: &str) -> Option<bool> {
        None
    }
    fn can_be_master_all(
        &self,
        asks: &[(String, Option<String>)],
    ) -> std::collections::BTreeMap<String, Option<bool>> {
        self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        asks.iter().map(|(n, _)| (n.clone(), Some(true))).collect()
    }
}

#[test]
fn one_ask_serves_every_node_machine() {
    let w = scripted_world(vec![mac("m1"), mac("m2"), mac("m3")]);
    let mut model = w.model;
    let counting = Arc::new(Counting(Default::default()));
    model.parts.cluster = counting.clone();
    for name in ["m1", "m2", "m3"] {
        model.set_node(name, true);
    }
    ask(&mut model, &[("m1", NODE_A), ("m2", NODE_B)]);
    assert_eq!(counting.0.load(std::sync::atomic::Ordering::SeqCst), 1);
    let m3 = model.machine("m3").unwrap();
    assert_eq!(
        model.can_be_master_line(&m3).as_deref(),
        Some("Can Be Master: Yes")
    );
}
