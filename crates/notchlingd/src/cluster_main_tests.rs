use super::{agents_of, first_line, is_home, originator, tick, MainFollower, Originators};
use crate::cluster_sessions::LocalRows;
use crate::cluster_testkit::{eventually, node, pair, Dir, Opts};
use notchling_cluster_net::MemNet;
use notchling_engine::cluster::LocalCmd;
use notchling_local_api::{ApiError, ErrorCode};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

struct Rows {
    rows: Result<Vec<Value>, String>,
    partial: bool,
}

fn rows(list: Vec<Value>) -> Rows {
    Rows {
        rows: Ok(list),
        partial: false,
    }
}

impl LocalRows for Rows {
    fn rows(&self) -> Result<Vec<Value>, ApiError> {
        self.rows
            .clone()
            .map_err(|why| ApiError::new(ErrorCode::Failed, why))
    }

    fn complete_rows(&self) -> Result<Option<Vec<Value>>, ApiError> {
        self.rows().map(|r| (!self.partial).then_some(r))
    }
}

fn row(id: &str, tool: &str) -> Value {
    json!({
        "id": id, "name": id, "project": "p", "cwd": "/work/p", "group": "completed",
        "label": "Idle", "sinceMs": 1, "target": {"job": "ab12cd34"}, "host": "Background",
        "headless": false, "tool": tool, "transcriptPath": format!("/t/{id}.jsonl"),
    })
}

fn rollout(dir: &Dir, name: &str, originator: &str) -> PathBuf {
    let path = dir.path().join(name);
    let line = json!({"type": "session_meta", "payload": {"id": name, "originator": originator}});
    std::fs::write(&path, format!("{line}\n{{\"type\":\"other\"}}\n")).unwrap();
    path
}

fn codex_row(id: &str, path: &Path) -> Value {
    let mut value = row(id, "codex");
    value["transcriptPath"] = json!(path);
    value
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
    let dir = Dir::new("mainrollouts");
    let exec = rollout(&dir, "exec.jsonl", "codex_exec");
    let tui = rollout(&dir, "tui.jsonl", "codex_cli_rs");
    let list = rows(vec![
        row("job-1", "claude"),
        codex_row("codex:c1", &exec),
        codex_row("codex:c2", &tui),
        row("agy:a1", "antigravity"),
        row("shell:s1", "shell"),
    ]);
    assert!(tick(&n.node, &list, &Originators::default()));
    let mut seen = main_ids(&n.node.status());
    seen.sort();
    assert_eq!(seen, ["codex:c1", "job-1"]);
}

#[test]
fn a_codex_rollout_that_cannot_be_read_is_not_main() {
    let dir = Dir::new("mainunreadable");
    let missing = dir.path().join("none.jsonl");
    let torn = dir.path().join("torn.jsonl");
    std::fs::write(&torn, "{\"type\":\"session_meta\",\"payload\":{\"origin").unwrap();
    let agents = agents_of(vec![
        codex_row("codex:a", &missing),
        codex_row("codex:b", &torn),
    ]);
    assert!(Originators::default().exec_ids(&agents).is_empty());
}

#[test]
fn only_the_first_line_is_read_and_the_answer_is_kept() {
    let dir = Dir::new("mainfirst");
    let path = dir.path().join("big.jsonl");
    let line = json!({"type": "session_meta", "payload": {"originator": "codex_exec"}});
    let mut text = format!("{line}\n");
    text.push_str(&"x".repeat(200_000));
    std::fs::write(&path, text).unwrap();
    assert_eq!(
        first_line(&path).as_deref(),
        Some(line.to_string().as_str())
    );
    let huge = dir.path().join("huge.jsonl");
    std::fs::write(&huge, "y".repeat(200_000)).unwrap();
    assert_eq!(first_line(&huge), None);
    assert_eq!(originator(&line.to_string()).as_deref(), Some("codex_exec"));
    assert_eq!(originator("{\"type\":\"turn_context\"}"), None);
    let origins = Originators::default();
    let agents = agents_of(vec![codex_row("codex:big", &path)]);
    assert_eq!(origins.exec_ids(&agents).len(), 1);
    std::fs::remove_file(&path).unwrap();
    assert_eq!(origins.exec_ids(&agents).len(), 1);
    assert!(origins.exec_ids(&[]).is_empty());
}

#[test]
fn a_partial_or_unreadable_list_changes_nothing() {
    let net = MemNet::new();
    let n = node(&net, Opts::named("mainpartial"));
    n.node.create("home").unwrap();
    let origins = Originators::default();
    let list = Rows {
        rows: Ok(vec![row("job-1", "claude")]),
        partial: true,
    };
    assert!(!tick(&n.node, &list, &origins));
    assert!(main_ids(&n.node.status()).is_empty());
    let failing = Rows {
        rows: Err("not ready".to_string()),
        partial: false,
    };
    assert!(!tick(&n.node, &failing, &origins));
    assert!(tick(&n.node, &rows(vec![row("job-1", "claude")]), &origins));
    assert_eq!(main_ids(&n.node.status()), ["job-1"]);
}

#[test]
fn a_kept_session_stays_off_when_the_list_is_read_again() {
    let net = MemNet::new();
    let n = node(&net, Opts::named("mainkept"));
    n.node.create("home").unwrap();
    let list = rows(vec![row("job-1", "claude")]);
    let origins = Originators::default();
    tick(&n.node, &list, &origins);
    n.node
        .local(LocalCmd::SetMain {
            session: "job-1".to_string(),
            main: false,
        })
        .unwrap();
    tick(&n.node, &list, &origins);
    let status = n.node.status();
    assert!(main_ids(&status).is_empty());
    assert_eq!(status["kept"], json!(["job-1"]));
}

#[test]
fn a_machine_that_is_not_the_primary_does_nothing() {
    let net = MemNet::new();
    let (home, worker) = pair(&net, Opts::named("mainhome2"), Opts::named("mainworker2"));
    let list = rows(vec![row("job-1", "claude")]);
    let origins = Originators::default();
    assert!(!tick(&worker.node, &list, &origins));
    assert!(main_ids(&worker.node.status()).is_empty());
    assert!(main_ids(&home.node.status()).is_empty());
    let alone = node(&net, Opts::named("mainalone"));
    assert!(!tick(&alone.node, &list, &origins));
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
        Arc::new(rows(vec![row("job-1", "claude")])),
        Duration::from_millis(20),
    );
    eventually("the session is main", || {
        main_ids(&n.node.status()) == ["job-1"]
    });
    follower.close();
    follower.close();
}
