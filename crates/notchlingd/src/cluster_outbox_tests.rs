use super::{OutboxFollower, OutboxSource, RemoteOutbox, OUTBOX_LIST, REFRESH_MS, RETRY_MS};
use crate::acting::Acting;
use crate::acting_ops_tests::{app, FakeRoom};
use crate::cluster_node::{Hooks, LocalCalls};
use crate::cluster_sessions::Peer;
use crate::cluster_testkit::{eventually, pair, Opts};
use crate::op_group::OpGroup;
use crate::outbox_ops::{OutboxOps, RemoteRows};
use notchling_cluster_net::MemNet;
use notchling_engine::cluster::wire::Origin;
use notchling_engine::cluster::NodeId;
use notchling_engine::outbox::OutboxStore;
use notchling_engine::sessions::Roots;
use notchling_local_api::{ops, ApiError};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};

fn owner() -> NodeId {
    NodeId::parse("fedcba9876543210").unwrap()
}

fn peer(online: bool) -> Vec<Peer> {
    vec![Peer {
        id: owner(),
        name: "worker".into(),
        online,
    }]
}

fn row(id: &str, state: &str) -> Value {
    json!({"id": id, "session": "ab12cd34", "tool": "claude", "kind": "prompt",
        "text": "words", "state": state, "position": 1})
}

fn qualified(row: &Value) -> Value {
    let mut shown = row.clone();
    shown["session"] = json!(format!("{}/ab12cd34", owner()));
    shown
}

struct Rows(Mutex<Vec<Value>>, Mutex<u32>);

impl Rows {
    fn new(rows: Vec<Value>) -> Rows {
        Rows(Mutex::new(rows), Mutex::new(0))
    }

    fn set(&self, rows: Vec<Value>) {
        *self.0.lock().unwrap() = rows;
    }

    fn asked(&self) -> u32 {
        *self.1.lock().unwrap()
    }
}

impl OutboxSource for Rows {
    fn list(&self, node: &NodeId) -> Result<Value, ApiError> {
        assert_eq!(*node, owner());
        *self.1.lock().unwrap() += 1;
        Ok(Value::Array(self.0.lock().unwrap().clone()))
    }
}

fn following_room() -> Arc<FakeRoom> {
    let room = Arc::new(FakeRoom::default());
    room.app.store(true, Ordering::SeqCst);
    room.following.lock().unwrap().push("outbox".into());
    room
}

fn follower(room: &Arc<FakeRoom>) -> RemoteOutbox {
    RemoteOutbox::new(room.clone(), Arc::new(|| true))
}

#[test]
fn the_first_list_publishes_every_row_with_the_session_qualified() {
    let room = following_room();
    let remote = follower(&room);
    let a = row("o1", "queued");
    let b = row("o2", "waiting");
    let source = Rows::new(vec![a.clone(), b.clone()]);
    remote.sync(&peer(true), &source, 0);
    assert_eq!(
        room.named("outbox.changed"),
        vec![json!({"row": qualified(&a)}), json!({"row": qualified(&b)})]
    );
    assert!(room.named("outbox.removed").is_empty());
}

#[test]
fn an_unchanged_list_publishes_nothing_and_waits_for_the_refresh() {
    let room = following_room();
    let remote = follower(&room);
    let source = Rows::new(vec![row("o1", "queued")]);
    remote.sync(&peer(true), &source, 0);
    remote.sync(&peer(true), &source, REFRESH_MS - 1);
    assert_eq!(source.asked(), 1, "not asked again before the refresh");
    remote.sync(&peer(true), &source, REFRESH_MS);
    assert_eq!(source.asked(), 2);
    assert_eq!(room.named("outbox.changed").len(), 1);
}

#[test]
fn a_changed_row_and_a_vanished_row_are_published() {
    let room = following_room();
    let remote = follower(&room);
    let source = Rows::new(vec![row("o1", "queued"), row("o2", "queued")]);
    remote.sync(&peer(true), &source, 0);
    let moved = row("o1", "sending");
    source.set(vec![moved.clone()]);
    remote.sync(&peer(true), &source, REFRESH_MS);
    let changed = room.named("outbox.changed");
    assert_eq!(changed.len(), 3);
    assert_eq!(changed[2], json!({"row": qualified(&moved)}));
    assert_eq!(
        room.named("outbox.removed"),
        vec![json!({"id": "o2", "session": format!("{}/ab12cd34", owner()), "why": "removed"})]
    );
}

#[test]
fn a_peer_change_event_pulls_the_list_at_once() {
    let room = following_room();
    let remote = follower(&room);
    let source = Rows::new(vec![row("o1", "queued")]);
    remote.sync(&peer(true), &source, 0);
    source.set(vec![row("o1", "queued"), row("o2", "queued")]);
    remote.peer_event(&owner(), "outbox.changed", &json!({"from": owner()}));
    remote.sync(&peer(true), &source, 1);
    assert_eq!(source.asked(), 2);
    assert_eq!(room.named("outbox.changed").len(), 2);
}

#[test]
fn a_removed_event_keeps_its_why_and_qualifies_the_session() {
    let room = following_room();
    let remote = follower(&room);
    let source = Rows::new(vec![row("o1", "sending")]);
    remote.sync(&peer(true), &source, 0);
    source.set(Vec::new());
    remote.peer_event(
        &owner(),
        "outbox.removed",
        &json!({"id": "o1", "session": "ab12cd34", "why": "sent", "from": owner()}),
    );
    assert_eq!(
        room.named("outbox.removed"),
        vec![json!({"id": "o1", "session": format!("{}/ab12cd34", owner()), "why": "sent"})]
    );
    remote.sync(&peer(true), &source, 1);
    assert_eq!(
        room.named("outbox.removed").len(),
        1,
        "the pull that follows does not say it twice"
    );
}

#[test]
fn nothing_is_pulled_while_no_app_follows_the_topic() {
    let room = following_room();
    let remote = follower(&room);
    let source = Rows::new(vec![row("o1", "queued")]);
    room.following.lock().unwrap().clear();
    remote.sync(&peer(true), &source, 0);
    remote.peer_event(&owner(), "outbox.changed", &json!({"from": owner()}));
    remote.sync(&peer(true), &source, 1);
    assert_eq!(source.asked(), 0);
    assert!(room.events.lock().unwrap().is_empty());
    room.following.lock().unwrap().push("outbox".into());
    remote.sync(&peer(true), &source, 2);
    assert_eq!(room.named("outbox.changed").len(), 1);
}

#[test]
fn a_node_that_may_not_follow_pulls_and_publishes_nothing() {
    let room = following_room();
    let remote = RemoteOutbox::new(room.clone(), Arc::new(|| false));
    let source = Rows::new(vec![row("o1", "queued")]);
    remote.sync(&peer(true), &source, 0);
    remote.peer_event(
        &owner(),
        "outbox.removed",
        &json!({"id": "o1", "session": "ab12cd34", "from": owner()}),
    );
    assert_eq!(source.asked(), 0);
    assert!(room.events.lock().unwrap().is_empty());
}

#[test]
fn an_offline_or_unknown_peer_is_not_asked() {
    let room = following_room();
    let remote = follower(&room);
    let source = Rows::new(vec![row("o1", "queued")]);
    remote.sync(&peer(false), &source, 0);
    remote.sync(&[], &source, 1);
    assert_eq!(source.asked(), 0);
}

#[test]
fn rows_without_an_id_or_session_are_dropped() {
    let room = following_room();
    let remote = follower(&room);
    let source = Rows::new(vec![
        json!({"session": "ab12cd34"}),
        json!({"id": "o9"}),
        json!("junk"),
        row("o1", "queued"),
    ]);
    remote.sync(&peer(true), &source, 0);
    assert_eq!(room.named("outbox.changed").len(), 1);
}

fn row_at(id: &str, session: &str, queued: i64, position: u64) -> Value {
    json!({"id": id, "session": session, "tool": "claude", "kind": "prompt",
        "text": "words", "state": "queued", "position": position, "queuedMs": queued})
}

fn ids(events: &[Value]) -> Vec<String> {
    events
        .iter()
        .map(|e| {
            e["row"]["id"]
                .as_str()
                .or_else(|| e["id"].as_str())
                .unwrap_or_default()
                .to_string()
        })
        .collect()
}

#[test]
fn rows_are_published_in_the_queue_order_the_owner_listed_them() {
    let room = following_room();
    let remote = follower(&room);
    let source = Rows::new(vec![
        row_at("zz", "ab12cd34", 1_000, 1),
        row_at("mm", "ab12cd34", 2_000, 2),
        row_at("aa", "ab12cd34", 3_000, 3),
    ]);
    remote.sync(&peer(true), &source, 0);
    assert_eq!(ids(&room.named("outbox.changed")), ["zz", "mm", "aa"]);
    let held: Vec<String> = ids(&remote.rows(None));
    assert_eq!(held, ["zz", "mm", "aa"]);
}

#[test]
fn vanished_rows_are_published_in_the_order_they_were_known() {
    let room = following_room();
    let remote = follower(&room);
    let source = Rows::new(vec![
        row_at("zz", "ab12cd34", 1_000, 1),
        row_at("mm", "ab12cd34", 2_000, 2),
        row_at("aa", "ab12cd34", 3_000, 3),
    ]);
    remote.sync(&peer(true), &source, 0);
    source.set(Vec::new());
    remote.sync(&peer(true), &source, REFRESH_MS);
    assert_eq!(ids(&room.named("outbox.removed")), ["zz", "mm", "aa"]);
}

#[test]
fn the_known_rows_answer_a_list_for_all_or_for_one_remote_session() {
    let room = following_room();
    let remote = follower(&room);
    let at = format!("{}/ab12cd34", owner());
    let other = format!("{}/ef56ab78", owner());
    let source = Rows::new(vec![
        row_at("o1", "ab12cd34", 1_000, 1),
        row_at("o2", "ef56ab78", 2_000, 1),
        row_at("o3", "ab12cd34", 3_000, 2),
    ]);
    assert!(
        remote.rows(None).is_empty(),
        "nothing is known before the first list"
    );
    remote.sync(&peer(true), &source, 0);
    let all = remote.rows(None);
    assert_eq!(ids(&all), ["o1", "o2", "o3"]);
    assert!(all.iter().all(|r| r["session"]
        .as_str()
        .is_some_and(|s| s.starts_with(&format!("{}/", owner())))));
    assert_eq!(ids(&remote.rows(Some(&at))), ["o1", "o3"]);
    assert_eq!(ids(&remote.rows(Some(&other))), ["o2"]);
    assert!(
        remote.rows(Some("ab12cd34")).is_empty(),
        "a bare id is local"
    );
    assert!(remote.rows(Some("nobody/none")).is_empty());
}

#[test]
fn the_known_rows_follow_a_removal_and_stop_when_no_app_follows() {
    let room = following_room();
    let remote = follower(&room);
    let source = Rows::new(vec![row("o1", "queued"), row("o2", "queued")]);
    remote.sync(&peer(true), &source, 0);
    remote.peer_event(
        &owner(),
        "outbox.removed",
        &json!({"id": "o1", "session": "ab12cd34", "why": "sent", "from": owner()}),
    );
    assert_eq!(ids(&remote.rows(None)), ["o2"]);
    room.following.lock().unwrap().clear();
    assert!(remote.rows(None).is_empty());
    let off = RemoteOutbox::new(room.clone(), Arc::new(|| false));
    assert!(off.rows(None).is_empty());
}

#[test]
fn a_second_client_listing_sees_the_rows_the_follower_already_knows() {
    let room = following_room();
    let remote = follower(&room);
    let source = Rows::new(vec![row("o1", "queued")]);
    remote.sync(&peer(true), &source, 0);
    remote.sync(&peer(true), &source, 1);
    assert_eq!(
        room.named("outbox.changed").len(),
        1,
        "the follower says a row once"
    );
    let listed = remote.rows(None);
    assert_eq!(listed, vec![qualified(&row("o1", "queued"))]);
}

#[test]
fn a_reconnecting_app_is_listed_the_remote_rows_the_follower_already_told() {
    let room = following_room();
    let remote = Arc::new(follower(&room));
    let source = Rows::new(vec![row("o1", "queued"), row("o2", "waiting")]);
    remote.sync(&peer(true), &source, 0);
    remote.sync(&peer(true), &source, 1);
    let told = room.named("outbox.changed").len();
    let outbox = OutboxOps::new(
        Arc::new(OutboxStore::in_memory_for_tests()),
        Arc::new(Mutex::new(json!([]))),
        Roots {
            main_dir: PathBuf::from("/nonexistent/claude"),
            lanes_file: PathBuf::from("/nonexistent/lanes.json"),
        },
        Arc::new(Acting::in_memory(true)),
        room.clone(),
        None,
    );
    outbox.set_remote(remote.clone());
    let listed = outbox
        .call(&app(), ops::OUTBOX_LIST, &json!({}))
        .expect("a list");
    assert_eq!(
        listed,
        json!([
            qualified(&row("o1", "queued")),
            qualified(&row("o2", "waiting"))
        ])
    );
    assert_eq!(
        room.named("outbox.changed").len(),
        told,
        "listing publishes nothing"
    );
}

struct Racing {
    rows: Mutex<Vec<Value>>,
    during: Mutex<Option<Box<dyn FnOnce() + Send>>>,
    asked: Mutex<u32>,
}

impl Racing {
    fn new(rows: Vec<Value>) -> Arc<Racing> {
        Arc::new(Racing {
            rows: Mutex::new(rows),
            during: Mutex::new(None),
            asked: Mutex::new(0),
        })
    }
}

impl OutboxSource for Racing {
    fn list(&self, node: &NodeId) -> Result<Value, ApiError> {
        assert_eq!(*node, owner());
        *self.asked.lock().unwrap() += 1;
        let snapshot = self.rows.lock().unwrap().clone();
        let hook = self.during.lock().unwrap().take();
        if let Some(hook) = hook {
            hook();
        }
        Ok(Value::Array(snapshot))
    }
}

#[test]
fn a_list_that_started_before_a_sent_removal_cannot_bring_the_row_back() {
    let room = following_room();
    let remote = Arc::new(follower(&room));
    let source = Racing::new(vec![row("o1", "sending")]);
    remote.sync(&peer(true), source.as_ref(), 0);
    assert_eq!(room.named("outbox.changed").len(), 1);

    remote.peer_event(&owner(), "outbox.changed", &json!({"from": owner()}));
    let (hearing, owner_rows) = (remote.clone(), source.clone());
    *source.during.lock().unwrap() = Some(Box::new(move || {
        *owner_rows.rows.lock().unwrap() = Vec::new();
        hearing.peer_event(
            &owner(),
            "outbox.removed",
            &json!({"id": "o1", "session": "ab12cd34", "why": "sent", "from": owner()}),
        );
    }));
    remote.sync(&peer(true), source.as_ref(), 1);
    assert_eq!(
        room.named("outbox.changed").len(),
        1,
        "the stale list is not applied"
    );
    assert_eq!(
        room.named("outbox.removed"),
        vec![json!({"id": "o1", "session": format!("{}/ab12cd34", owner()), "why": "sent"})]
    );

    remote.sync(&peer(true), source.as_ref(), 2);
    assert_eq!(*source.asked.lock().unwrap(), 3, "the node is listed again");
    assert_eq!(room.named("outbox.changed").len(), 1);
    assert_eq!(
        room.named("outbox.removed").len(),
        1,
        "the sent removal is never followed by a plain one"
    );
    assert!(remote.rows(None).is_empty());
}

#[test]
fn a_list_that_started_before_a_nudge_is_asked_again_at_once() {
    let room = following_room();
    let remote = Arc::new(follower(&room));
    let source = Racing::new(vec![row("o1", "queued")]);
    remote.sync(&peer(true), source.as_ref(), 0);
    let (hearing, owner_rows) = (remote.clone(), source.clone());
    remote.peer_event(&owner(), "outbox.changed", &json!({"from": owner()}));
    *source.during.lock().unwrap() = Some(Box::new(move || {
        owner_rows.rows.lock().unwrap().push(row("o2", "queued"));
        hearing.peer_event(&owner(), "outbox.changed", &json!({"from": owner()}));
    }));
    remote.sync(&peer(true), source.as_ref(), 1);
    assert_eq!(room.named("outbox.changed").len(), 1);
    remote.sync(&peer(true), source.as_ref(), 2);
    assert_eq!(ids(&room.named("outbox.changed")), ["o1", "o2"]);
}

struct Flaky {
    failures: Mutex<u32>,
    junk: bool,
    rows: Vec<Value>,
    asked: Mutex<u32>,
}

impl Flaky {
    fn new(failures: u32, junk: bool) -> Flaky {
        Flaky {
            failures: Mutex::new(failures),
            junk,
            rows: vec![row("o1", "queued")],
            asked: Mutex::new(0),
        }
    }

    fn asked(&self) -> u32 {
        *self.asked.lock().unwrap()
    }
}

impl OutboxSource for Flaky {
    fn list(&self, _: &NodeId) -> Result<Value, ApiError> {
        *self.asked.lock().unwrap() += 1;
        let mut left = self.failures.lock().unwrap();
        if *left > 0 {
            *left -= 1;
            return if self.junk {
                Ok(json!({"not": "a list"}))
            } else {
                Err(ApiError::internal("the node is busy"))
            };
        }
        Ok(Value::Array(self.rows.clone()))
    }
}

#[test]
fn a_failed_list_is_tried_again_soon_not_after_the_full_refresh() {
    const { assert!(RETRY_MS < REFRESH_MS) };
    for junk in [false, true] {
        let room = following_room();
        let remote = follower(&room);
        let source = Flaky::new(1, junk);
        remote.sync(&peer(true), &source, 0);
        assert_eq!(source.asked(), 1);
        assert!(room.events.lock().unwrap().is_empty());
        remote.sync(&peer(true), &source, RETRY_MS - 1);
        assert_eq!(source.asked(), 1, "not before the retry wait");
        remote.sync(&peer(true), &source, RETRY_MS);
        assert_eq!(source.asked(), 2);
        assert_eq!(room.named("outbox.changed").len(), 1);
        remote.sync(&peer(true), &source, RETRY_MS + REFRESH_MS - 1);
        assert_eq!(source.asked(), 2, "a good list waits the full refresh");
        remote.sync(&peer(true), &source, RETRY_MS + REFRESH_MS);
        assert_eq!(source.asked(), 3);
    }
}

#[test]
fn a_failed_list_keeps_the_known_rows() {
    let room = following_room();
    let remote = follower(&room);
    let source = Rows::new(vec![row("o1", "queued")]);
    remote.sync(&peer(true), &source, 0);
    let failing = Flaky::new(1, false);
    remote.sync(&peer(true), &failing, REFRESH_MS);
    assert_eq!(ids(&remote.rows(None)), ["o1"]);
    assert!(room.named("outbox.removed").is_empty());
}

struct Lists;

impl LocalCalls for Lists {
    fn call(&self, op: &str, _args: &Value, _origin: &Origin) -> Result<Value, ApiError> {
        match op {
            OUTBOX_LIST => Ok(json!([row("o1", "queued")])),
            other => Ok(json!({"ran": other})),
        }
    }
}

struct Hear(Arc<RemoteOutbox>);

impl Hooks for Hear {
    fn peer_outbox(&self, from: &NodeId, name: &str, data: &Value) {
        self.0.peer_event(from, name, data);
    }
}

#[test]
fn a_remote_nodes_outbox_topic_reaches_the_local_app() {
    let net = MemNet::new();
    let room = following_room();
    let remote = Arc::new(follower(&room));
    let mut home = Opts::named("home");
    home.client = true;
    home.room = Some(room.clone());
    home.hooks = Some(Arc::new(Hear(remote.clone())));
    let mut worker = Opts::named("worker");
    worker.local = Some(Arc::new(Lists));
    let (a, b) = pair(&net, home, worker);
    let followed = OutboxFollower::spawn(a.node.clone(), remote.clone());
    let at = format!("{}/ab12cd34", b.id());
    let shown = json!({"row": {
        "id": "o1", "session": at, "tool": "claude",
        "kind": "prompt", "text": "words", "state": "queued", "position": 1,
    }});
    eventually("the baseline row reaches the app", || {
        room.named("outbox.changed").contains(&shown)
    });
    b.node.tell(
        "outbox",
        "outbox.removed",
        json!({"id": "o1", "session": "ab12cd34", "why": "sent", "text": "never shown"}),
    );
    eventually("the removal reaches the app with its why", || {
        room.named("outbox.removed")
            .contains(&json!({"id": "o1", "session": at, "why": "sent"}))
    });
    followed.close();
    assert!(room
        .events
        .lock()
        .unwrap()
        .iter()
        .all(|(_, data)| !data.to_string().contains("never shown")));
}
