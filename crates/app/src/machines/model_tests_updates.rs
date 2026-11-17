use std::collections::BTreeMap;
use std::sync::Arc;

use notchling_engine::machines::registry::{Machine, MachineOs, NodeSettings};
use notchling_engine::machines::step::ClusterTokens;
use serde_json::{json, Value};

use super::model_updates::UpdateAsk;

const ME: &str = "a1b2c3d4e5f60718";
const M1: &str = "1122334455667788";
const OURS: &str = "a1b2c3d";
const OTHER: &str = "9f8e7d6";

struct Status(Option<Value>);

impl ClusterTokens for Status {
    fn join_token(&self) -> Result<String, String> {
        Err("not asked".into())
    }
    fn cluster_id(&self) -> Result<Option<String>, String> {
        Ok(None)
    }
    fn can_be_master(&self, _machine: &str) -> Option<bool> {
        None
    }
    fn status_json(&self) -> Option<Value> {
        self.0.clone()
    }
}

fn member(id: &str, name: &str, build: &str) -> Value {
    json!({
        "id": id, "name": name, "role": "worker", "canBeMaster": false,
        "online": true, "state": "active",
        "versions": {
            "notchling": "0.9.0", "proto": 1, "features": [], "schema": 1,
            "os": "macos", "arch": "aarch64", "build": build,
        },
    })
}

fn status(m1_build: &str) -> Value {
    json!({
        "cluster": {"id": "c1", "name": "home", "term": 1, "home": ME},
        "me": ME,
        "members": [member(ME, "m5", OURS), member(M1, "m1", m1_build)],
        "jobs": [],
    })
}

fn machine() -> Machine {
    let mut m = Machine::new("m1", "m1.test", "worker");
    m.os = Some(MachineOs::MacOs);
    m.node = Some(NodeSettings::default());
    m
}

fn ask(status: Option<Value>, nodes: &[(&str, &str)]) -> UpdateAsk {
    let nodes: BTreeMap<String, String> = nodes
        .iter()
        .map(|(n, id)| ((*n).to_owned(), (*id).to_owned()))
        .collect();
    UpdateAsk::new(Arc::new(Status(status)), vec![machine()], nodes)
}

#[test]
fn the_machines_banner_names_the_machines_and_points_to_the_cluster_sheet() {
    let line = ask(Some(status(OTHER)), &[("m1", M1)])
        .run(Some(OURS))
        .expect("m1 runs another build");
    assert_eq!(
        line,
        "1 machine runs another Notchling build: m1. Update them from the Cluster sheet in Notchling."
    );
}

#[test]
fn the_machines_banner_is_empty_when_builds_match_or_there_is_no_cluster() {
    assert_eq!(ask(Some(status(OURS)), &[]).run(Some(OURS)), None);
    assert_eq!(ask(None, &[]).run(Some(OURS)), None);
    assert_eq!(ask(Some(status(OTHER)), &[]).run(None), None);
}
