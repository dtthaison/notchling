use crate::acting_ops_tests::FakeRoom;
use crate::inbox_ops::{InboxOps, Responder};
use crate::limits_ops_tests::TempDir;
use crate::op_group::OpGroup;
use notchling_engine::inbox::{self, Action, ActionId, Item, Kind, Source, State};
use notchling_local_api::{ops, topics, Caller, ClientKind, ErrorCode, OpSpec, Tier};
use serde_json::{json, Value};
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const NOW: i64 = 1_800_000_000_000;

#[derive(Default)]
struct Calls(Mutex<Vec<(String, ActionId)>>);

impl Responder for Calls {
    fn answered(&self, item: &Item, action: ActionId) {
        self.0.lock().unwrap().push((item.id.clone(), action));
    }
}

fn caller(tier: Tier, client: ClientKind) -> Caller {
    Caller {
        tier,
        client,
        peer: None,
        pid: None,
    }
}

fn app() -> Caller {
    caller(Tier::App, ClientKind::App)
}

fn agent() -> Caller {
    caller(Tier::Agent, ClientKind::Cli)
}

fn item(id: &str) -> Item {
    Item {
        id: id.to_string(),
        kind: Kind::SecretApproval,
        title: format!("title {id}"),
        detail: String::new(),
        source: Source::default(),
        created_ms: NOW,
        expires_ms: None,
        actions: vec![
            Action {
                id: ActionId::Allow,
                label: "Allow".to_string(),
                primary: true,
            },
            Action {
                id: ActionId::Deny,
                label: "Deny".to_string(),
                primary: false,
            },
        ],
        state: State::Open,
        answer: None,
        answered_by: None,
        seen: false,
        open: None,
        closed_ms: None,
    }
}

struct Rig {
    _tmp: TempDir,
    dir: std::path::PathBuf,
    room: Arc<FakeRoom>,
    clock: Arc<AtomicI64>,
    ops: Arc<InboxOps>,
}

fn rig() -> Rig {
    let tmp = TempDir::new("inbox-ops");
    let dir = tmp.path("data");
    let room = Arc::new(FakeRoom::default());
    let clock = Arc::new(AtomicI64::new(NOW));
    let ops = InboxOps::with_clock(dir.clone(), room.clone(), {
        let clock = clock.clone();
        Arc::new(move || clock.load(Ordering::SeqCst))
    });
    Rig {
        _tmp: tmp,
        dir,
        room,
        clock,
        ops,
    }
}

fn answer(ops: &InboxOps, id: &str, action: &str) -> Value {
    ops.call(
        &app(),
        ops::INBOX_ANSWER,
        &json!({"id": id, "action": action}),
    )
    .unwrap()
}

fn changed(room: &FakeRoom) -> Vec<Value> {
    room.named("inbox.changed")
}

#[test]
fn list_requires_app_tier() {
    let r = rig();
    for op in [ops::INBOX_LIST, ops::INBOX_ANSWER, ops::INBOX_SEEN] {
        assert_eq!(r.ops.spec(op), Some(OpSpec::APP), "{op}");
        let e = r.ops.call(&agent(), op, &json!({})).unwrap_err();
        assert_eq!(e.code, ErrorCode::Forbidden, "{op}");
    }
    assert_eq!(r.ops.spec("inbox.nope"), None);
    assert_eq!(r.ops.topics(), &[topics::INBOX]);
    assert_eq!(r.ops.topic_tier(topics::INBOX), Some(Tier::App));
    assert_eq!(r.ops.topic_tier(topics::VAULT), None);
}

#[test]
fn list_returns_items_and_unread() {
    let r = rig();
    r.ops.raise(item("a"), None);
    let mut b = item("b");
    b.created_ms = NOW + 1;
    r.ops.raise(b, None);
    let out = r.ops.call(&app(), ops::INBOX_LIST, &json!({})).unwrap();
    assert_eq!(out["unread"], 2);
    let ids: Vec<&str> = out["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["id"].as_str().unwrap())
        .collect();
    assert_eq!(ids, ["b", "a"]);
}

#[test]
fn answer_calls_responder_once() {
    let r = rig();
    let calls = Arc::new(Calls::default());
    let id = r.ops.raise(item("a"), Some(calls.clone()));
    assert_eq!(id, "a");
    let out = answer(&r.ops, "a", "allow");
    assert_eq!(out["answered"], true);
    assert_eq!(out["state"], "answered");
    assert_eq!(out["answer"], "allow");
    assert_eq!(
        *calls.0.lock().unwrap(),
        vec![("a".to_string(), ActionId::Allow)]
    );
    assert_eq!(
        changed(&r.room).last().unwrap()["item"]["state"],
        "answered"
    );
}

#[test]
fn second_answer_is_refused_and_reports_state() {
    let r = rig();
    let calls = Arc::new(Calls::default());
    r.ops.raise(item("a"), Some(calls.clone()));
    let first = answer(&r.ops, "a", "allow");
    let second = answer(&r.ops, "a", "deny");
    assert_eq!(first["answered"], true);
    assert_eq!(second["answered"], false);
    assert_eq!(second["state"], "answered");
    assert_eq!(second["answer"], "allow");
    assert_eq!(calls.0.lock().unwrap().len(), 1);
}

#[test]
fn answer_with_a_bad_item_or_action_is_an_error() {
    let r = rig();
    r.ops.raise(item("a"), None);
    let code = |args: Value| {
        r.ops
            .call(&app(), ops::INBOX_ANSWER, &args)
            .unwrap_err()
            .code
    };
    assert_eq!(
        code(json!({"id": "nope", "action": "allow"})),
        ErrorCode::Failed
    );
    assert_eq!(
        code(json!({"id": "a", "action": "cancel"})),
        ErrorCode::BadRequest
    );
    assert_eq!(
        code(json!({"id": "a", "action": "explode"})),
        ErrorCode::BadRequest
    );
    assert_eq!(code(json!({"action": "allow"})), ErrorCode::BadRequest);
}

#[test]
fn raise_publishes_changed() {
    let r = rig();
    r.ops.raise(item("a"), None);
    let events = r.room.events.lock().unwrap().clone();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].0, "inbox.changed");
    assert_eq!(events[0].1["item"]["id"], "a");
}

#[test]
fn a_cap_victim_is_published_in_order() {
    let r = rig();
    for n in 0..inbox::MAX_OPEN {
        let mut i = item(&format!("n{n}"));
        i.kind = Kind::Notice;
        i.created_ms = NOW + n as i64;
        r.ops.raise(i, None);
    }
    let before = r.room.events.lock().unwrap().len();
    let mut extra = item("extra");
    extra.created_ms = NOW + 10_000;
    r.ops.raise(extra, None);
    let events = r.room.events.lock().unwrap().clone();
    let tail = &events[before..];
    assert_eq!(tail.len(), 2);
    assert_eq!(tail[0].0, "inbox.changed");
    assert_eq!(tail[0].1["item"]["id"], "extra");
    assert_eq!(tail[1].0, "inbox.removed");
    assert_eq!(tail[1].1["id"], "n0");
}

#[test]
fn resolve_closes_without_the_responder() {
    let r = rig();
    let calls = Arc::new(Calls::default());
    r.ops.raise(item("a"), Some(calls.clone()));
    r.ops.resolve("a", State::Answered, Some(ActionId::Allow));
    let events = changed(&r.room);
    assert_eq!(events.last().unwrap()["item"]["state"], "answered");
    assert_eq!(events.last().unwrap()["item"]["answer"], "allow");
    let second = answer(&r.ops, "a", "deny");
    assert_eq!(second["answered"], false);
    assert!(calls.0.lock().unwrap().is_empty());
}

#[test]
fn tick_expires_and_publishes() {
    let r = rig();
    let calls = Arc::new(Calls::default());
    let mut i = item("a");
    i.expires_ms = Some(NOW + 1_000);
    r.ops.raise(i, Some(calls.clone()));
    r.ops.tick(NOW + 999);
    assert_eq!(changed(&r.room).len(), 1);
    r.ops.tick(NOW + 1_000);
    let events = changed(&r.room);
    assert_eq!(events.len(), 2);
    assert_eq!(events[1]["item"]["state"], "expired");
    let late = answer(&r.ops, "a", "allow");
    assert_eq!(late["answered"], false);
    assert_eq!(late["state"], "expired");
    assert!(calls.0.lock().unwrap().is_empty());
}

#[test]
fn tick_prunes_old_closed_items_and_publishes_removed() {
    let r = rig();
    r.ops.raise(item("a"), None);
    r.ops.resolve("a", State::Answered, Some(ActionId::Allow));
    r.ops.tick(NOW + inbox::DONE_KEEP_MS);
    assert!(r.room.named("inbox.removed").is_empty());
    r.ops.tick(NOW + inbox::DONE_KEEP_MS + 1);
    assert_eq!(r.room.named("inbox.removed"), vec![json!({"id": "a"})]);
    let out = r.ops.call(&app(), ops::INBOX_LIST, &json!({})).unwrap();
    assert_eq!(out["items"], json!([]));
    let stored = inbox::load(&r.dir, NOW + inbox::DONE_KEEP_MS + 2).unwrap();
    assert!(stored.items().is_empty());
}

#[test]
fn seen_marks_items_and_returns_unread() {
    let r = rig();
    r.ops.raise(item("a"), None);
    r.ops.raise(item("b"), None);
    let out = r
        .ops
        .call(&app(), ops::INBOX_SEEN, &json!({"ids": ["a", "zzz"]}))
        .unwrap();
    assert_eq!(out["unread"], 1);
    let events = changed(&r.room);
    assert_eq!(events.last().unwrap()["item"]["id"], "a");
    assert_eq!(events.last().unwrap()["item"]["seen"], true);
    let bad = r
        .ops
        .call(&app(), ops::INBOX_SEEN, &json!({"ids": "a"}))
        .unwrap_err();
    assert_eq!(bad.code, ErrorCode::BadRequest);
}

#[test]
fn changes_are_persisted() {
    let r = rig();
    r.ops.raise(item("a"), None);
    answer(&r.ops, "a", "allow");
    let items = inbox::load(&r.dir, NOW + 5).unwrap().items();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].state, State::Answered);
    assert_eq!(items[0].answer, Some(ActionId::Allow));
}

#[test]
fn a_restart_withdraws_open_items_and_saves_that() {
    let r = rig();
    r.ops.raise(item("a"), None);
    let later = NOW + 5_000;
    let reopened = InboxOps::with_clock(
        r.dir.clone(),
        Arc::new(FakeRoom::default()),
        Arc::new(move || later),
    );
    let out = reopened.call(&app(), ops::INBOX_LIST, &json!({})).unwrap();
    assert_eq!(out["items"][0]["state"], "withdrawn");
    assert_eq!(out["unread"], 0);
    let aged = later + inbox::DONE_KEEP_MS + 100;
    let mut stored = inbox::load(&r.dir, aged).unwrap();
    assert_eq!(stored.items()[0].state, State::Withdrawn);
    assert_eq!(stored.items()[0].closed_ms, Some(later));
    stored.prune(aged);
    assert!(stored.items().is_empty());
}

#[test]
fn a_failing_save_never_fails_an_op_or_skips_the_publish() {
    let tmp = TempDir::new("inbox-ops-bad");
    tmp.write("file", "not a folder");
    let room = Arc::new(FakeRoom::default());
    let ops = InboxOps::with_clock(tmp.path("file/inside"), room.clone(), Arc::new(|| NOW));
    let calls = Arc::new(Calls::default());
    ops.raise(item("a"), Some(calls.clone()));
    assert_eq!(changed(&room).len(), 1);
    let out = answer(&ops, "a", "allow");
    assert_eq!(out["answered"], true);
    assert_eq!(changed(&room).len(), 2);
    assert_eq!(calls.0.lock().unwrap().len(), 1);
}

#[test]
fn the_loop_ticks_until_closed() {
    let r = rig();
    let mut i = item("a");
    i.expires_ms = Some(NOW + 1);
    r.ops.raise(i, None);
    r.clock.store(NOW + 10, Ordering::SeqCst);
    r.ops.spawn_loop_every(Duration::from_millis(20));
    let until = Instant::now() + Duration::from_secs(5);
    while changed(&r.room).len() < 2 && Instant::now() < until {
        std::thread::sleep(Duration::from_millis(10));
    }
    r.ops.close();
    assert_eq!(changed(&r.room).last().unwrap()["item"]["state"], "expired");
}
