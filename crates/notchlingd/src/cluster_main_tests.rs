use super::{agents_of, is_home, tick, MainFollower};
use crate::cluster_sessions::LocalRows;
use crate::cluster_testkit::{eventually, node, pair, Opts};
use notchling_cluster_net::MemNet;
use notchling_engine::cluster::LocalCmd;
use notchling_local_api::{ApiError, ErrorCode};
use serde_json::{json, Value};
use std::sync::Arc;
use std::time::Duration;

struct Rows(Result<Vec<Value>, String>);

impl LocalRows for Rows {
    fn rows(&self) -> Result<Vec<Value>, ApiError> {
        self.0
            .clone()
            .map_err(|why| ApiError::new(ErrorCode::Failed, why))
    }
}

fn row(id: &str, tool: &str) -> Value {
    json!({
        "id": id, "name": id, "project": "p", "cwd": "/work/p", "group": "completed",
        "label": "Idle", "sinceMs": 1, "target": {"job": id}, "host": "Background",
        "headless": false, "tool": tool,
    })
}

fn main_ids(status: &Value) -> Vec<String> {
    status["main"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect()
}

#[test]
fn the_home_follows_the_session_list() {
    let net = MemNet::new();
    let n = node(&net, Opts::named("mainhome"));
    n.node.create("home").unwrap();
    assert!(is_home(&n.node.status()));
    let rows = Rows(Ok(vec![
        row("job-1", "claude"),
        row("codex:c1", "codex"),
        row("agy:a1", "antigravity"),
        row("shell:s1", "shell"),
    ]));
    assert!(tick(&n.node, &rows));
    let mut seen = main_ids(&n.node.status());
    seen.sort();
    assert_eq!(seen, ["codex:c1", "job-1"]);
}

#[test]
fn a_kept_session_stays_off_when_the_list_is_read_again() {
    let net = MemNet::new();
    let n = node(&net, Opts::named("mainkept"));
    n.node.create("home").unwrap();
    let rows = Rows(Ok(vec![row("job-1", "claude")]));
    tick(&n.node, &rows);
    n.node
        .local(LocalCmd::SetMain {
            session: "job-1".to_string(),
            main: false,
        })
        .unwrap();
    tick(&n.node, &rows);
    let status = n.node.status();
    assert!(main_ids(&status).is_empty());
    assert_eq!(status["kept"], json!(["job-1"]));
}

#[test]
fn a_machine_that_is_not_the_primary_or_has_no_list_does_nothing() {
    let net = MemNet::new();
    let (home, worker) = pair(&net, Opts::named("mainhome2"), Opts::named("mainworker2"));
    let rows = Rows(Ok(vec![row("job-1", "claude")]));
    assert!(!tick(&worker.node, &rows));
    assert!(main_ids(&worker.node.status()).is_empty());
    assert!(!tick(&home.node, &Rows(Err("not ready".to_string()))));
    assert!(main_ids(&home.node.status()).is_empty());
    let alone = node(&net, Opts::named("mainalone"));
    assert!(!tick(&alone.node, &rows));
}

#[test]
fn rows_that_are_not_sessions_are_skipped() {
    let agents = agents_of(vec![row("job-1", "claude"), json!({"id": 5}), json!("x")]);
    assert_eq!(agents.len(), 1);
}

#[test]
fn the_follower_polls_until_closed() {
    let net = MemNet::new();
    let n = node(&net, Opts::named("mainloop"));
    n.node.create("home").unwrap();
    let follower = MainFollower::spawn(
        n.node.clone(),
        Arc::new(Rows(Ok(vec![row("job-1", "claude")]))),
        Duration::from_millis(20),
    );
    eventually("the session is main", || {
        main_ids(&n.node.status()) == ["job-1"]
    });
    follower.close();
    follower.close();
}
