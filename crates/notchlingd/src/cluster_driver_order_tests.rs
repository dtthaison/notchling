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
    ops: Mutex<Vec<String>>,
}

impl LocalCalls for Probe {
    fn call(&self, op: &str, args: &Value, _origin: &Origin) -> Result<Value, ApiError> {
        self.ops.lock().unwrap().push(op.to_string());
        if op == "delegate.run" {
            return Ok(json!({"exit": 0, "out": "", "err": ""}));
        }
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
        let err = a.node.local(cmd).unwrap_err();
        assert!(err.message.contains("dispatch again when it is primary"));
    }
    let saw = probe.saw.lock().unwrap().clone();
    assert!(saw.len() >= 25);
    assert!(saw.iter().all(|seen| *seen), "an attach saw an old state");
}

#[test]
fn a_save_that_fails_after_a_terminal_transition_still_revokes_the_job() {
    let net = MemNet::new();
    let probe = Arc::new(Probe::default());
    let mut home = Opts::named("home");
    home.client = true;
    home.os = "macos";
    home.tools = vec![(notchling_engine::versions::Tool::Agy, "1.5.1")];
    home.local = Some(probe.clone());
    let (a, _b) = pair(&net, home, Opts::named("worker"));
    *probe.node.lock().unwrap() = Some(a.node.clone());
    let db = a.cluster_dir().join("cluster.db");
    let conn = rusqlite::Connection::open(&db).unwrap();
    conn.execute_batch(
        "CREATE TRIGGER no_done BEFORE INSERT ON journal
         WHEN NEW.command LIKE '%\"state\":\"done\"%'
         BEGIN SELECT RAISE(ABORT, 'blocked'); END;",
    )
    .unwrap();
    drop(conn);
    let spec = JobSpec {
        node: a.id(),
        tool: Tool::Agy,
        cwd: "/srv".to_string(),
        args: json!({"delegate": ["agy", "--json", "--", "hi"]}),
        secrets: Vec::new(),
    };
    let cmd = LocalCmd::Dispatch {
        key: "d1".to_string(),
        spec,
        tier: OriginTier::App,
    };
    let err = a.node.local(cmd).unwrap_err();
    assert!(err.message.contains("saving the cluster failed"), "{err:?}");
    crate::cluster_testkit::eventually("the revoke runs", || {
        probe
            .ops
            .lock()
            .unwrap()
            .iter()
            .any(|op| op == "grant.revoke_job")
    });
    assert!(a.audit_text().contains("cluster.step step=save"));
}
