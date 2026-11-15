use super::{
    peers_of, tail, LocalRows, NodeSource, Peer, RemoteSessions, RowSource, SessionCalls,
    NODE_FOLLOW, NODE_SESSIONS, NODE_TAIL, STALE_MS, TAIL_MAX,
};
use crate::acting_ops::Room;
use crate::acting_ops_tests::FakeRoom;
use crate::cluster_node::{Hooks, LocalCalls};
use crate::cluster_testkit::{eventually, pair, Dir, Opts};
use notchling_cluster_net::MemNet;
use notchling_engine::bridge::net::crypto::base64_decode;
use notchling_engine::cluster::wire::{Origin, OriginTier};
use notchling_engine::cluster::NodeId;
use notchling_local_api::{ApiError, ErrorCode};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

fn node(n: u8) -> NodeId {
    NodeId::parse(&format!("{n:016x}")).unwrap()
}

fn peer(n: u8, name: &str, online: bool) -> Peer {
    Peer {
        id: node(n),
        name: name.to_string(),
        online,
    }
}

#[derive(Default)]
pub(crate) struct Rows {
    pub by_node: Mutex<BTreeMap<NodeId, Result<Value, ApiError>>>,
    pub asked: AtomicUsize,
}

impl Rows {
    pub(crate) fn set(&self, node: &NodeId, rows: Value) {
        self.by_node.lock().unwrap().insert(node.clone(), Ok(rows));
    }
}

impl RowSource for Rows {
    fn rows(&self, node: &NodeId) -> Result<Value, ApiError> {
        self.asked.fetch_add(1, Ordering::SeqCst);
        self.by_node
            .lock()
            .unwrap()
            .get(node)
            .cloned()
            .unwrap_or_else(|| Err(ApiError::new(ErrorCode::Failed, "no rows")))
    }
}

fn remote(dir: &Dir) -> RemoteSessions {
    RemoteSessions::new(dir.path().join("mirror"))
}

#[test]
fn remote_rows_are_qualified_with_node_field() {
    let dir = Dir::new("rows");
    let sessions = remote(&dir);
    let rows = Rows::default();
    rows.set(
        &node(1),
        json!([{"id": "s1", "name": "build", "host": "Background", "transcriptPath": "/w/s1.jsonl",
                "target": {"job": "ab12"}}]),
    );
    sessions.sync(&[peer(1, "w1", true)], &rows, 0);
    let merged = sessions.merge(&json!([{"id": "local", "host": "Background"}]));
    let list = merged.as_array().unwrap();
    assert_eq!(list.len(), 2);
    assert_eq!(list[0]["id"], "local");
    let row = &list[1];
    assert_eq!(row["id"], json!(format!("{}/s1", node(1))));
    assert_eq!(row["host"], json!(node(1)));
    assert_eq!(
        row["node"],
        json!({"id": node(1), "name": "w1", "online": true})
    );
    assert_eq!(row["transcriptPath"], Value::Null, "no mirror file yet");
    assert_eq!(row["name"], "build");
    assert_eq!(row["target"], json!({"job": "ab12"}));
}

#[test]
fn a_mirror_file_becomes_the_rows_transcript_path() {
    let dir = Dir::new("rows-mirror");
    let sessions = remote(&dir);
    let rows = Rows::default();
    rows.set(
        &node(1),
        json!([{"id": "s1", "transcriptPath": "/w/s1.jsonl"}]),
    );
    sessions.sync(&[peer(1, "w1", true)], &rows, 0);
    let file = dir
        .path()
        .join("mirror")
        .join(node(1).as_str())
        .join("s1.jsonl");
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::write(&file, b"x").unwrap();
    let row = sessions.rows().remove(0);
    assert_eq!(row["transcriptPath"], json!(file.to_string_lossy()));
}

#[test]
fn offline_node_rows_say_offline() {
    let dir = Dir::new("rows-offline");
    let sessions = remote(&dir);
    let rows = Rows::default();
    rows.set(&node(1), json!([{"id": "s1"}]));
    sessions.sync(&[peer(1, "w1", true)], &rows, 0);
    assert_eq!(sessions.rows()[0]["node"]["online"], true);
    sessions.sync(&[peer(1, "w1", false)], &rows, 1_000);
    let list = sessions.rows();
    assert_eq!(list.len(), 1, "the last rows stay");
    assert_eq!(list[0]["node"]["online"], false);
    assert_eq!(list[0]["id"], json!(format!("{}/s1", node(1))));
}

#[test]
fn a_node_that_is_no_longer_a_member_leaves_every_list() {
    let dir = Dir::new("rows-gone");
    let sessions = remote(&dir);
    let rows = Rows::default();
    rows.set(&node(1), json!([{"id": "s1"}]));
    rows.set(&node(2), json!([{"id": "s2"}]));
    sessions.sync(&[peer(1, "w1", true), peer(2, "w2", true)], &rows, 0);
    assert_eq!(sessions.rows().len(), 2);
    sessions.sync(&[peer(2, "w2", true)], &rows, 1_000);
    let list = sessions.rows();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0]["node"]["name"], "w2");
}

#[test]
fn rows_are_asked_on_link_on_an_event_and_when_stale() {
    let dir = Dir::new("rows-asked");
    let sessions = remote(&dir);
    let rows = Rows::default();
    rows.set(&node(1), json!([{"id": "s1"}]));
    let online = [peer(1, "w1", true)];
    sessions.sync(&online, &rows, 0);
    sessions.sync(&online, &rows, 1_000);
    assert_eq!(rows.asked.load(Ordering::SeqCst), 1, "asked once on link");
    sessions.mark_dirty(&node(1));
    sessions.sync(&online, &rows, 2_000);
    assert_eq!(rows.asked.load(Ordering::SeqCst), 2, "asked on its event");
    sessions.sync(&online, &rows, 2_000 + STALE_MS);
    assert_eq!(
        rows.asked.load(Ordering::SeqCst),
        3,
        "asked again when stale"
    );
    sessions.sync(&[peer(1, "w1", false)], &rows, 40_000);
    sessions.sync(&online, &rows, 41_000);
    assert_eq!(rows.asked.load(Ordering::SeqCst), 4, "asked on a new link");
}

#[test]
fn a_failed_ask_keeps_the_last_rows_and_retries_soon() {
    let dir = Dir::new("rows-fail");
    let sessions = remote(&dir);
    let rows = Rows::default();
    rows.set(&node(1), json!([{"id": "s1"}]));
    let online = [peer(1, "w1", true)];
    sessions.sync(&online, &rows, 0);
    rows.by_node
        .lock()
        .unwrap()
        .insert(node(1), Err(ApiError::new(ErrorCode::Failed, "boom")));
    sessions.mark_dirty(&node(1));
    sessions.sync(&online, &rows, 1_000);
    assert_eq!(sessions.rows().len(), 1, "the last rows stay");
    let asked = rows.asked.load(Ordering::SeqCst);
    sessions.sync(&online, &rows, 2_000);
    assert_eq!(rows.asked.load(Ordering::SeqCst), asked, "not at once");
    sessions.sync(&online, &rows, 20_000);
    assert!(rows.asked.load(Ordering::SeqCst) > asked, "but soon");
}

#[test]
fn rows_without_an_id_are_dropped() {
    let dir = Dir::new("rows-junk");
    let sessions = remote(&dir);
    let rows = Rows::default();
    rows.set(
        &node(1),
        json!([{"id": "s1"}, {"name": "no id"}, 7, {"id": 3}, {"id": ""}]),
    );
    sessions.sync(&[peer(1, "w1", true)], &rows, 0);
    assert_eq!(sessions.rows().len(), 1);
    rows.set(&node(1), json!({"not": "a list"}));
    sessions.mark_dirty(&node(1));
    sessions.sync(&[peer(1, "w1", true)], &rows, 1_000);
    assert_eq!(
        sessions.rows().len(),
        1,
        "an answer that isn't a list is no answer"
    );
}

#[test]
fn peers_come_from_the_status() {
    let status = json!({
        "me": node(1),
        "members": [
            {"id": node(1), "name": "me", "state": "active", "online": true},
            {"id": node(2), "name": "w2", "state": "active", "online": true},
            {"id": node(3), "name": "w3", "state": "pending", "online": false},
            {"id": node(4), "name": "w4", "state": "removed", "online": false},
            {"id": node(5), "name": "w5", "state": "active", "online": false},
        ]
    });
    let peers = peers_of(&status);
    assert_eq!(peers, vec![peer(2, "w2", true), peer(5, "w5", false)]);
    assert!(peers_of(&json!({"cluster": null, "members": []})).is_empty());
}

fn transcript(dir: &Dir, bytes: usize) -> std::path::PathBuf {
    let path = dir.path().join("t.jsonl");
    let body: Vec<u8> = (0..bytes).map(|i| b'a' + (i % 26) as u8).collect();
    std::fs::write(&path, body).unwrap();
    path
}

fn row_for(path: &std::path::Path) -> Vec<Value> {
    vec![
        json!({"id": "s1", "transcriptPath": path.to_string_lossy()}),
        json!({"id": "s2", "transcriptPath": null}),
        json!({"id": "s3"}),
    ]
}

fn decoded(out: &Value) -> Vec<u8> {
    base64_decode(out["data"].as_str().unwrap()).unwrap()
}

#[test]
fn tail_serves_only_known_transcripts() {
    let dir = Dir::new("tail-known");
    let path = transcript(&dir, 100);
    let secret = dir.path().join("secret.txt");
    std::fs::write(&secret, b"never").unwrap();
    let rows = row_for(&path);
    let out = tail(&rows, &json!({"id": "s1", "from": 10})).unwrap();
    assert_eq!(decoded(&out), std::fs::read(&path).unwrap()[10..].to_vec());
    assert_eq!(out["next"], 100);
    assert_eq!(out["size"], 100);
    assert_eq!(out["reset"], false);
    for args in [
        json!({"id": "nope", "from": 0}),
        json!({"id": "s2", "from": 0}),
        json!({"id": "s3", "from": 0}),
        json!({"from": 0}),
    ] {
        assert!(tail(&rows, &args).is_err(), "{args}");
    }
    let sneaky = json!({"id": "nope", "from": 0, "path": secret.to_string_lossy(),
        "transcriptPath": secret.to_string_lossy()});
    assert!(
        tail(&rows, &sneaky).is_err(),
        "a path from the caller is never read"
    );
}

#[test]
fn tail_is_capped_at_512k() {
    let dir = Dir::new("tail-cap");
    let path = transcript(&dir, TAIL_MAX + 100_000);
    let rows = row_for(&path);
    let out = tail(&rows, &json!({"id": "s1", "from": 0})).unwrap();
    assert_eq!(decoded(&out).len(), TAIL_MAX);
    assert_eq!(out["next"], TAIL_MAX as u64);
    assert_eq!(out["size"], (TAIL_MAX + 100_000) as u64);
    let rest = tail(&rows, &json!({"id": "s1", "from": TAIL_MAX})).unwrap();
    assert_eq!(decoded(&rest).len(), 100_000);
    assert_eq!(rest["next"], rest["size"]);
    let line = serde_json::to_vec(&out).unwrap().len();
    assert!(line < notchling_engine::cluster::wire::MAX_LINE, "{line}");
}

#[test]
fn tail_resets_when_the_file_is_shorter_or_replaced() {
    let dir = Dir::new("tail-reset");
    let path = transcript(&dir, 100);
    let rows = row_for(&path);
    let first = tail(&rows, &json!({"id": "s1", "from": 0})).unwrap();
    let generation = first["gen"].as_str().unwrap().to_string();
    let same = tail(&rows, &json!({"id": "s1", "from": 100, "gen": generation})).unwrap();
    assert_eq!(same["reset"], false);
    assert_eq!(decoded(&same).len(), 0);
    let shorter = tail(&rows, &json!({"id": "s1", "from": 500})).unwrap();
    assert_eq!(shorter["reset"], true);
    assert_eq!(decoded(&shorter).len(), 100);
    let other = dir.path().join("other");
    std::fs::write(&other, vec![b'z'; 300]).unwrap();
    std::fs::rename(&other, &path).unwrap();
    let replaced = tail(&rows, &json!({"id": "s1", "from": 100, "gen": generation})).unwrap();
    assert_eq!(replaced["reset"], true, "a replaced file starts over");
    assert_eq!(decoded(&replaced), vec![b'z'; 300]);
}

struct FixedRows(Vec<Value>);

impl LocalRows for FixedRows {
    fn rows(&self) -> Result<Vec<Value>, ApiError> {
        Ok(self.0.clone())
    }
}

struct Passed;

impl LocalCalls for Passed {
    fn call(&self, op: &str, _args: &Value, _origin: &Origin) -> Result<Value, ApiError> {
        Ok(json!({"passed": op}))
    }
}

fn origin_of(n: u8) -> Origin {
    Origin {
        node: node(n),
        tier: OriginTier::Node,
    }
}

#[test]
fn the_owner_answers_node_ops_before_the_daemon_does() {
    let dir = Dir::new("calls");
    let path = transcript(&dir, 10);
    let room = Arc::new(FakeRoom::default());
    let calls = SessionCalls::new(
        Arc::new(Passed),
        Arc::new(FixedRows(row_for(&path))),
        room.clone(),
    );
    let listed = calls
        .call(NODE_SESSIONS, &json!({}), &origin_of(1))
        .unwrap();
    assert_eq!(listed.as_array().unwrap().len(), 3);
    let tailed = calls
        .call(NODE_TAIL, &json!({"id": "s1", "from": 0}), &origin_of(1))
        .unwrap();
    assert_eq!(tailed["size"], 10);
    assert_eq!(
        calls
            .call("session.reply", &json!({}), &origin_of(1))
            .unwrap(),
        json!({"passed": "session.reply"})
    );
    assert!(!room.can_answer_prompts());
    let followed = calls
        .call(NODE_FOLLOW, &json!({"topic": "prompts"}), &origin_of(1))
        .unwrap();
    assert_eq!(followed["following"], true);
    assert!(room.can_answer_prompts());
    assert!(calls
        .call(NODE_FOLLOW, &json!({"topic": "vault"}), &origin_of(1))
        .is_err());
}

struct Recorded(Mutex<Vec<NodeId>>);

impl Hooks for Recorded {
    fn peer_sessions(&self, from: &NodeId) {
        self.0.lock().unwrap().push(from.clone());
    }
}

struct Listing;

impl LocalCalls for Listing {
    fn call(&self, op: &str, _args: &Value, _origin: &Origin) -> Result<Value, ApiError> {
        match op {
            NODE_SESSIONS => {
                Ok(json!([{"id": "s1", "name": "remote one", "transcriptPath": null}]))
            }
            other => Ok(json!({"ran": other})),
        }
    }
}

#[test]
fn a_linked_node_lists_its_sessions_and_announces_changes() {
    let net = MemNet::new();
    let heard = Arc::new(Recorded(Mutex::new(Vec::new())));
    let mut home = Opts::named("home");
    home.client = true;
    home.hooks = Some(heard.clone());
    let mut worker = Opts::named("worker");
    worker.local = Some(Arc::new(Listing));
    let (a, b) = pair(&net, home, worker);
    let dir = Dir::new("e2e");
    let sessions = RemoteSessions::new(dir.path().join("mirror"));
    let status = a.node.status();
    sessions.sync(&peers_of(&status), &NodeSource::new(a.node.clone()), 0);
    let list = sessions.rows();
    assert_eq!(list.len(), 1, "{list:?}");
    assert_eq!(list[0]["id"], json!(format!("{}/s1", b.id())));
    assert_eq!(list[0]["node"]["name"], "worker");
    b.node.tell("sessions", "sessions.changed", json!({}));
    eventually("the home hears the change", || {
        heard.0.lock().unwrap().contains(&b.id())
    });
    assert!(
        a.room.named("sessions.changed").is_empty(),
        "a peer's event is never replayed to the local room"
    );
}
