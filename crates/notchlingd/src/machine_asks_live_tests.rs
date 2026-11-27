use super::machine_asks_live::{build, machine_runner, MemberTokens};
use crate::acting_ops_tests::FakeRoom;
use crate::cluster_asks::Admin;
use crate::daemon_wiring::Wired;
use crate::layout::Layout;
use notchling_engine::machines::runner::{argv, Call, CommandResult, ScriptedRunner, REFUSED};
use notchling_engine::machines::step::ClusterTokens;
use notchling_engine::machines::tailscale::Tailscale;
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

struct Root(PathBuf);

impl Drop for Root {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn fixture(tag: &str) -> (Root, Layout) {
    let root = std::env::temp_dir().join(format!("nl-live-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let layout = Layout::resolve(None, Some(root.as_os_str()), || None).unwrap();
    assert!(layout.fixture);
    (Root(root), layout)
}

struct Status(Mutex<Value>);

impl Admin for Status {
    fn status(&self) -> Value {
        self.0.lock().unwrap().clone()
    }
    fn own_organization(&self) -> Option<String> {
        None
    }
    fn organization_of(&self, _machine: &str) -> Option<String> {
        None
    }
    fn host_of(&self, _machine: &str) -> Option<String> {
        None
    }
    fn matches(&self, _machine: &str) -> usize {
        1
    }
    fn node_of(&self, _machine: &str) -> Option<String> {
        None
    }
    fn secret_organization(&self, _name: &str) -> Option<String> {
        None
    }
}

fn tokens(status: Value, tailscale: &Arc<ScriptedRunner>) -> MemberTokens {
    MemberTokens::new(
        Arc::new(Status(Mutex::new(status))),
        Tailscale::new(tailscale.clone(), "/fake/tailscale"),
    )
}

const STATUS: &str = r#"{"BackendState":"Running","Self":{"HostName":"mac","TailscaleIPs":["100.64.0.10"],"Online":true},"CurrentTailnet":{"Name":"person@example.com","MagicDNSSuffix":"tail0a1b2.ts.net"},"Peer":{}}"#;

#[test]
fn a_fixture_daemon_runs_no_ssh_and_no_tailscale() {
    let (_root, layout) = fixture("runner");
    let ran = machine_runner(&layout).run(&Call::secs(argv(&["/usr/bin/true"]), 5));
    assert_eq!(ran.code, REFUSED, "{}", ran.err);
    let sessions = Arc::new(Mutex::new(json!([])));
    let wired = Wired::build(&layout, sessions, Arc::new(FakeRoom::default()), (0, 0));
    let machines = build(&wired, &layout);
    let ran = machines
        .engine
        .runner()
        .run(&Call::secs(argv(&["/usr/bin/true"]), 5));
    assert_eq!(ran.code, REFUSED, "{}", ran.err);
    assert!(machines.tailscale.networks().is_err());
}

#[test]
fn the_daemons_tokens_read_the_cluster_and_never_mint_a_join_token() {
    let tail = Arc::new(ScriptedRunner::new());
    tail.on(
        |a| a.get(1).map(String::as_str) == Some("status"),
        CommandResult::ok(STATUS),
    );
    let status = json!({
        "cluster": {"id": "c1", "home": "aaaa"},
        "members": [
            {"id": "aaaa", "state": "active", "canBeMaster": true,
             "addrs": ["192.168.1.5", "100.64.0.20"], "port": 47823},
            {"id": "bbbb", "state": "active", "canBeMaster": false},
        ],
    });
    let t = tokens(status, &tail);
    assert_eq!(t.cluster_id(), Ok(Some("c1".to_string())));
    assert_eq!(t.primary_addr(), Some(("100.64.0.20".to_string(), 47823)));
    assert_eq!(t.can_be_master_of("w", Some("aaaa")), Some(true));
    assert_eq!(t.can_be_master_of("w", Some("bbbb")), Some(false));
    assert_eq!(t.can_be_master_of("w", Some("cccc")), None);
    assert_eq!(t.can_be_master_of("w", None), None);
    assert_eq!(t.tailnet(), Some("person@example.com".to_string()));
    assert!(t.status_json().is_some());
    assert!(t.join_token().is_err());
    assert_eq!(t.invite("n", "w", false, false), Ok(None));
}

#[test]
fn tokens_with_no_cluster_say_so_and_a_failing_tailscale_gives_no_tailnet() {
    let tail = Arc::new(ScriptedRunner::new());
    let t = tokens(
        json!({"cluster": null, "joining": null, "members": []}),
        &tail,
    );
    assert_eq!(t.cluster_id(), Ok(None));
    assert_eq!(t.primary_addr(), None);
    assert_eq!(t.tailnet(), None);
}
