use super::tests::{following, inline, node, peer, prompt, unit, FakeSource, Unit};
use super::{lock, PromptSource, RemotePrompts, FOLLOW_EVERY_MS};
use crate::acting_ops::Room;
use crate::acting_ops_tests::FakeRoom;
use crate::cluster_sessions::RemoteSessions;
use crate::cluster_testkit::Dir;
use crate::prompts::{fit_list, push_row, row_bytes, LIST_BYTES, ROW_BYTES};
use notchling_engine::cluster::{qualify, NodeId};
use notchling_local_api::ApiError;
use notchling_local_api::{Caller, ClientKind, Tier};
use serde_json::{json, Value};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::mpsc::channel;
use std::sync::{Arc, Mutex};
use std::time::Duration;

fn sized(id: &str, bytes: usize) -> Value {
    let mut row = prompt(id);
    row["kind"] = json!("question");
    let mut fill = bytes.saturating_sub(400);
    loop {
        row["question"] = json!({"items": [{"header": "h", "question": "q".repeat(fill),
                                            "options": ["a"], "multiSelect": false}]});
        let size = row_bytes(&row);
        if size + 8 >= bytes {
            return row;
        }
        fill += bytes - 8 - size;
    }
}

fn app() -> Caller {
    Caller {
        tier: Tier::App,
        client: ClientKind::App,
        peer: None,
        pid: None,
    }
}

fn held_listable(u: &Unit, local: Value) -> Vec<Value> {
    let merged = u.prompts.merge(local, &app()).as_array().unwrap().clone();
    let total: usize = merged.iter().map(row_bytes).sum();
    assert!(total <= LIST_BYTES, "{total}");
    for held in u.prompts.list() {
        assert!(
            merged.iter().any(|m| m["id"] == held["id"]),
            "{} is held and published but not listed",
            held["id"]
        );
    }
    let resolved: Vec<Value> = u
        .room
        .named("prompt.resolved")
        .iter()
        .map(|r| r["id"].clone())
        .collect();
    for shown in u.room.named("prompt.pending") {
        let gone = resolved.contains(&shown["id"]);
        let held = u.prompts.list().iter().any(|h| h["id"] == shown["id"]);
        assert!(
            gone || held,
            "{} published, not held, never resolved",
            shown["id"]
        );
    }
    merged
}

#[test]
fn many_large_remote_rows_are_all_listable_or_never_published() {
    let u = unit();
    let source = Arc::new(FakeSource::default());
    following(&u, &source, 1);
    for i in 0..7 {
        u.prompts.on_event(
            &node(1),
            "prompt.pending",
            &sized(&format!("big{i}"), 120 * 1024),
        );
    }
    let owner: Vec<Value> = (0..7)
        .map(|i| sized(&format!("l{i}"), 110 * 1024))
        .collect();
    source.set(&node(2), Value::Array(owner));
    u.prompts
        .sync(&[peer(1, true), peer(2, true)], true, &source, 0);
    assert!(!u.prompts.list().is_empty());
    held_listable(&u, json!([]));
}

#[test]
fn published_row_is_always_listable() {
    let u = unit();
    let source = Arc::new(FakeSource::default());
    let near = sized("near", ROW_BYTES);
    assert!(
        row_bytes(&near) < ROW_BYTES && push_row(&near).is_some(),
        "the owner sends it"
    );
    let mut rows = vec![near];
    rows.extend((0..7).map(|i| sized(&format!("p{i}"), ROW_BYTES - 1024)));
    source.set(&node(1), Value::Array(rows.clone()));
    u.prompts.sync(&[peer(1, true)], true, &source, 0);
    for (i, row) in rows.iter().enumerate() {
        let mut pushed = row.clone();
        pushed["id"] = json!(format!("pushed{i}"));
        u.prompts.on_event(&node(1), "prompt.pending", &pushed);
    }
    held_listable(&u, json!([]));
    assert!(u
        .room
        .named("prompt.pending")
        .iter()
        .all(|p| p["id"] != json!(qualify(&node(1), "near"))));
}

#[test]
fn local_rows_cannot_crowd_out_held_remote_rows() {
    let local: Vec<Value> = (0..8)
        .map(|i| json!({"id": format!("mine-{i}"), "summary": "x".repeat(100_000)}))
        .collect();
    let empty = unit();
    let alone = empty.prompts.merge(Value::Array(local.clone()), &app());
    assert_eq!(
        alone,
        Value::Array(fit_list(local.clone())),
        "local keeps it all"
    );
    assert_eq!(alone.as_array().unwrap().len(), 7);

    let u = unit();
    let source = Arc::new(FakeSource::default());
    following(&u, &source, 1);
    for id in ["r1", "r2"] {
        u.prompts
            .on_event(&node(1), "prompt.pending", &sized(id, 100 * 1024));
    }
    assert_eq!(u.prompts.list().len(), 2);
    let merged = held_listable(&u, Value::Array(local));
    assert_eq!(merged[0]["id"], "mine-0", "local rows first");
}

#[test]
fn owner_list_cut_never_resolves_a_held_prompt() {
    let u = unit();
    let source = Arc::new(FakeSource::default());
    following(&u, &source, 1);
    let held = sized("held", 60 * 1024);
    u.prompts.on_event(&node(1), "prompt.pending", &held);
    let mut owner: Vec<Value> = (0..6)
        .map(|i| sized(&format!("big{i}"), 120 * 1024))
        .collect();
    owner.push(held);
    let listed = fit_list(owner);
    assert!(listed.iter().all(|r| r["id"] != "held"), "the owner cut it");
    source.set(&node(1), Value::Array(listed));
    u.prompts
        .sync(&[peer(1, true)], true, &source, FOLLOW_EVERY_MS);
    let id = json!(qualify(&node(1), "held"));
    assert!(u.prompts.list().iter().any(|p| p["id"] == id));
    assert!(u
        .room
        .named("prompt.resolved")
        .iter()
        .all(|r| r["id"] != id));
    held_listable(&u, json!([]));
}

#[derive(Default)]
struct PanicRoom {
    inner: FakeRoom,
    panic_on: Mutex<Option<Value>>,
    always: Mutex<Option<Value>>,
}

impl Room for PanicRoom {
    fn publish(&self, topic: &str, event: &str, data: &Value) {
        if self.always.lock().unwrap().as_ref() == Some(&data["id"]) {
            panic!("publish always fails");
        }
        let armed = self.panic_on.lock().unwrap().take();
        match armed {
            Some(id) if id == data["id"] => panic!("publish failed"),
            other => *self.panic_on.lock().unwrap() = other,
        }
        self.inner.publish(topic, event, data);
    }

    fn can_answer(&self) -> bool {
        self.inner.can_answer()
    }

    fn can_follow(&self, topic: &str) -> bool {
        self.inner.can_follow(topic)
    }
}

#[test]
fn publish_panic_keeps_the_rest_of_the_batch() {
    let room = Arc::new(PanicRoom::default());
    let dir = Dir::new("prompts-panic");
    let remote = Arc::new(RemoteSessions::new(dir.path().join("mirror")));
    let prompts = RemotePrompts::with_spawn(room.clone(), remote, inline());
    let source = Arc::new(FakeSource::default());
    prompts.sync(&[peer(1, true)], true, &source, 0);
    for id in ["p1", "p2"] {
        prompts.on_event(&node(1), "prompt.pending", &prompt(id));
    }
    *room.panic_on.lock().unwrap() = Some(json!(qualify(&node(1), "p1")));
    let failed = catch_unwind(AssertUnwindSafe(|| prompts.drop_node(&node(1), "offline")));
    assert!(failed.is_err());
    prompts.stop("unfollowed");
    let resolved: Vec<Value> = room
        .inner
        .named("prompt.resolved")
        .iter()
        .map(|r| r["id"].clone())
        .collect();
    assert_eq!(
        resolved,
        vec![
            json!(qualify(&node(1), "p1")),
            json!(qualify(&node(1), "p2"))
        ]
    );
}

fn panic_rig(tag: &str) -> (Arc<PanicRoom>, Arc<RemotePrompts>, Dir) {
    let room = Arc::new(PanicRoom::default());
    let dir = Dir::new(tag);
    let remote = Arc::new(RemoteSessions::new(dir.path().join("mirror")));
    let prompts = RemotePrompts::with_spawn(room.clone(), remote, inline());
    (room, prompts, dir)
}

fn ids_of(room: &PanicRoom, event: &str) -> Vec<Value> {
    room.inner
        .named(event)
        .iter()
        .map(|r| r["id"].clone())
        .collect()
}

#[test]
fn a_publish_that_always_panics_is_dropped_and_the_rest_goes_out() {
    let (room, prompts, _dir) = panic_rig("prompts-always");
    let source = Arc::new(FakeSource::default());
    prompts.sync(&[peer(1, true)], true, &source, 0);
    let q = |id: &str| json!(qualify(&node(1), id));
    *room.always.lock().unwrap() = Some(q("p1"));
    let pend = |id: &str| {
        catch_unwind(AssertUnwindSafe(|| {
            prompts.on_event(&node(1), "prompt.pending", &prompt(id))
        }))
    };
    assert!(pend("p1").is_err(), "first try");
    assert!(pend("p2").is_err(), "the one retry");
    assert!(pend("p3").is_ok(), "then it is dropped");
    assert_eq!(ids_of(&room, "prompt.pending"), vec![q("p2"), q("p3")]);
    assert!(catch_unwind(AssertUnwindSafe(|| prompts.stop("unfollowed"))).is_err());
    assert!(catch_unwind(AssertUnwindSafe(|| prompts.stop("unfollowed"))).is_err());
    prompts.stop("unfollowed");
    assert_eq!(ids_of(&room, "prompt.resolved"), vec![q("p2"), q("p3")]);
}

#[test]
fn follower_thread_survives_a_publish_panic() {
    let (room, prompts, _dir) = panic_rig("prompts-follower");
    let source = Arc::new(FakeSource::default());
    source.set(&node(1), json!([prompt("p1"), prompt("p2")]));
    let q = |id: &str| json!(qualify(&node(1), id));
    *room.always.lock().unwrap() = Some(q("p1"));
    let status = json!({"me": node(9), "members": [
        {"id": node(9), "state": "active", "client": true},
        {"id": node(1), "state": "active", "online": true, "name": "n1"},
    ]});
    let (stop, rx) = channel();
    let looping = {
        let prompts = prompts.clone();
        std::thread::spawn(move || {
            let status = move || status.clone();
            let app = || true;
            super::follow_loop(
                &prompts,
                &status,
                &app,
                &source,
                &rx,
                Duration::from_millis(5),
            );
        })
    };
    crate::cluster_testkit::eventually("p2 goes out after p1 is dropped", || {
        ids_of(&room, "prompt.pending").contains(&q("p2"))
    });
    stop.send(()).unwrap();
    assert!(looping.join().is_ok(), "the follower thread never dies");
    assert!(prompts.list().is_empty());
    assert_eq!(ids_of(&room, "prompt.resolved"), vec![q("p2")]);
}

struct Exploding;

impl PromptSource for Exploding {
    fn follow(&self, _node: &NodeId) -> Result<(), ApiError> {
        panic!("follow failed");
    }

    fn list(&self, _node: &NodeId) -> Result<Value, ApiError> {
        Ok(json!([]))
    }
}

#[test]
fn renew_panic_frees_the_busy_slot() {
    let u = unit();
    let source = Arc::new(Exploding);
    let failed = catch_unwind(AssertUnwindSafe(|| {
        u.prompts.sync(&[peer(1, true)], true, &source, 0)
    }));
    assert!(failed.is_err());
    assert!(lock(&u.prompts.book).busy.is_empty());
}
