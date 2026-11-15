use crate::cluster_node::{ClusterNode, LocalCalls};
use crate::cluster_testkit::{pair, Opts};
use notchling_cluster_net::MemNet;
use notchling_engine::cluster::wire::{Origin, OriginTier};
use notchling_engine::cluster::{JobSpec, LocalCmd, Tool};
use notchling_local_api::ApiError;
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};

#[derive(Default)]
struct Probe {
    node: Mutex<Option<Arc<ClusterNode>>>,
    saw: Mutex<Vec<bool>>,
}

impl LocalCalls for Probe {
    fn call(&self, op: &str, args: &Value, _origin: &Origin) -> Result<Value, ApiError> {
        if op != "grant.attach" {
            return Ok(json!({}));
        }
        let job = args["job"]["id"].as_str().unwrap_or("").to_string();
        let node = self.node.lock().unwrap().clone();
        let seen = node
            .and_then(|n| n.state())
            .is_some_and(|s| s.jobs.contains_key(&job));
        self.saw.lock().unwrap().push(seen);
        Ok(json!({"waiting": "the vault is elsewhere"}))
    }
}

#[test]
fn a_grant_attach_reads_a_state_that_already_holds_its_job() {
    let net = MemNet::new();
    let probe = Arc::new(Probe::default());
    let mut home = Opts::named("home");
    home.client = true;
    home.os = "macos";
    home.local = Some(probe.clone());
    let (a, b) = pair(&net, home, Opts::named("worker"));
    *probe.node.lock().unwrap() = Some(a.node.clone());
    let allow = LocalCmd::SetAllow {
        node: b.id(),
        names: Some(vec!["gh".to_string()]),
        any: None,
    };
    a.node.local(allow).unwrap();
    for n in 0..25 {
        let spec = JobSpec {
            node: b.id(),
            tool: Tool::Shell,
            cwd: "/srv".to_string(),
            args: json!({"command": "true"}),
            secrets: vec!["gh".to_string()],
        };
        let cmd = LocalCmd::Dispatch {
            key: format!("k{n}"),
            spec,
            tier: OriginTier::App,
        };
        a.node.local(cmd).unwrap();
    }
    let saw = probe.saw.lock().unwrap().clone();
    assert!(saw.len() >= 25);
    assert!(saw.iter().all(|seen| *seen), "an attach saw an old state");
}
