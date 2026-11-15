use super::{OutboxFollower, OutboxSource, RemoteOutbox, OUTBOX_LIST, REFRESH_MS};
use crate::acting_ops_tests::FakeRoom;
use crate::cluster_node::{Hooks, LocalCalls};
use crate::cluster_sessions::Peer;
use crate::cluster_testkit::{eventually, pair, Opts};
use notchling_cluster_net::MemNet;
use notchling_engine::cluster::wire::Origin;
use notchling_engine::cluster::NodeId;
use notchling_local_api::ApiError;
use serde_json::{json, Value};
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
