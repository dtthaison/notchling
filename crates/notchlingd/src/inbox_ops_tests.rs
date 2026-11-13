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

pub(crate) const NOW: i64 = 1_800_000_000_000;

#[derive(Default)]
pub(crate) struct Calls(pub(crate) Mutex<Vec<(String, ActionId)>>);

impl Responder for Calls {
    fn answered(&self, item: &Item, action: ActionId) -> bool {
        self.0.lock().unwrap().push((item.id.clone(), action));
        true
    }
}

struct Refuses;

impl Responder for Refuses {
    fn answered(&self, _item: &Item, _action: ActionId) -> bool {
        false
    }
}

pub(crate) fn caller(tier: Tier, client: ClientKind) -> Caller {
    Caller {
        tier,
        client,
        peer: None,
        pid: None,
    }
}

pub(crate) fn app() -> Caller {
    caller(Tier::App, ClientKind::App)
}

fn agent() -> Caller {
    caller(Tier::Agent, ClientKind::Cli)
}

pub(crate) fn item(id: &str) -> Item {
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

pub(crate) struct Rig {
    pub(crate) _tmp: TempDir,
    pub(crate) dir: std::path::PathBuf,
    pub(crate) room: Arc<FakeRoom>,
    pub(crate) clock: Arc<AtomicI64>,
    pub(crate) ops: Arc<InboxOps>,
}

pub(crate) fn rig() -> Rig {
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

pub(crate) fn answer(ops: &InboxOps, id: &str, action: &str) -> Value {
    ops.call(
        &app(),
        ops::INBOX_ANSWER,
        &json!({"id": id, "action": action}),
    )
    .unwrap()
}

pub(crate) fn changed(room: &FakeRoom) -> Vec<Value> {
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
    r.ops.flush();
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
    r.ops.flush();
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
    r.ops.flush();
    let before = r.room.events.lock().unwrap().len();
    let mut extra = item("extra");
    extra.created_ms = NOW + 10_000;
    r.ops.raise(extra, None);
    r.ops.flush();
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
    r.ops
        .resolve("a", State::Answered, Some(ActionId::Allow), None);
    r.ops.flush();
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
    let mut i = item("a");
    i.expires_ms = Some(NOW + 1_000);
    r.ops.raise(i, None);
    r.ops.tick(NOW + 999);
    r.ops.flush();
    assert_eq!(changed(&r.room).len(), 1);
    r.ops.tick(NOW + 1_000);
    r.ops.flush();
    let events = changed(&r.room);
    assert_eq!(events.len(), 2);
    assert_eq!(events[1]["item"]["state"], "expired");
    let late = answer(&r.ops, "a", "allow");
    assert_eq!(late["answered"], false);
    assert_eq!(late["state"], "expired");
}

#[test]
fn tick_prunes_old_closed_items_and_publishes_removed() {
    let r = rig();
    r.ops.raise(item("a"), None);
    r.ops
        .resolve("a", State::Answered, Some(ActionId::Allow), None);
    r.ops.tick(NOW + inbox::DONE_KEEP_MS);
    r.ops.flush();
    assert!(r.room.named("inbox.removed").is_empty());
    r.ops.tick(NOW + inbox::DONE_KEEP_MS + 1);
    r.ops.flush();
    assert_eq!(r.room.named("inbox.removed"), vec![json!({"id": "a"})]);
    let out = r.ops.call(&app(), ops::INBOX_LIST, &json!({})).unwrap();
    assert_eq!(out["items"], json!([]));
    r.ops.flush();
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
    r.ops.flush();
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
    r.ops.flush();
    let items = inbox::load(&r.dir, NOW + 5).unwrap().items();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].state, State::Answered);
    assert_eq!(items[0].answer, Some(ActionId::Allow));
}

#[test]
fn a_restart_withdraws_open_items_and_saves_that() {
    let r = rig();
    r.ops.raise(item("a"), None);
    r.ops.flush();
    let later = NOW + 5_000;
    let reopened = InboxOps::with_clock(
        r.dir.clone(),
        Arc::new(FakeRoom::default()),
        Arc::new(move || later),
    );
    reopened.start();
    let out = reopened.call(&app(), ops::INBOX_LIST, &json!({})).unwrap();
    assert_eq!(out["items"][0]["state"], "withdrawn");
    assert_eq!(out["unread"], 0);
    let aged = later + inbox::DONE_KEEP_MS + 100;
    reopened.close();
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
    ops.flush();
    assert_eq!(changed(&room).len(), 1);
    let out = answer(&ops, "a", "allow");
    assert_eq!(out["answered"], true);
    ops.flush();
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

struct GatedRoom {
    gate: Mutex<Option<std::sync::mpsc::Receiver<()>>>,
    events: Mutex<Vec<Value>>,
}

impl crate::acting_ops::Room for GatedRoom {
    fn publish(&self, _topic: &str, event: &str, data: &Value) {
        let gate = self.gate.lock().unwrap().take();
        if let Some(gate) = gate {
            let _ = gate.recv();
        }
        if event == "inbox.changed" {
            self.events.lock().unwrap().push(data["item"].clone());
        }
    }
    fn can_answer(&self) -> bool {
        true
    }
    fn can_follow(&self, _topic: &str) -> bool {
        true
    }
}

#[test]
fn a_slow_publish_blocks_no_op_and_events_keep_book_order() {
    let tmp = TempDir::new("inbox-ops-gated");
    let (release, gate) = std::sync::mpsc::channel::<()>();
    let room = Arc::new(GatedRoom {
        gate: Mutex::new(Some(gate)),
        events: Mutex::new(Vec::new()),
    });
    let ops = InboxOps::with_clock(tmp.path("data"), room.clone(), Arc::new(|| NOW));
    ops.raise(item("a"), None);
    let seen = ops
        .call(&app(), ops::INBOX_SEEN, &json!({"ids": ["a"]}))
        .unwrap();
    assert_eq!(seen["unread"], 0);
    assert_eq!(answer(&ops, "a", "allow")["answered"], true);
    assert!(room.events.lock().unwrap().is_empty());
    release.send(()).unwrap();
    ops.flush();
    let events = room.events.lock().unwrap().clone();
    let shape: Vec<(Value, Value)> = events
        .iter()
        .map(|i| (i["state"].clone(), i["seen"].clone()))
        .collect();
    assert_eq!(
        shape,
        vec![
            (json!("open"), json!(false)),
            (json!("open"), json!(true)),
            (json!("answered"), json!(true)),
        ]
    );
}

#[test]
fn racing_calls_publish_in_book_order() {
    let r = rig();
    for n in 0..40 {
        let id = format!("race{n}");
        r.ops.raise(item(&id), None);
        let seeing = {
            let ops = r.ops.clone();
            let id = id.clone();
            std::thread::spawn(move || {
                ops.call(&app(), ops::INBOX_SEEN, &json!({"ids": [id]}))
                    .unwrap();
            })
        };
        let answering = {
            let ops = r.ops.clone();
            let id = id.clone();
            std::thread::spawn(move || {
                answer(&ops, &id, "allow");
            })
        };
        seeing.join().unwrap();
        answering.join().unwrap();
    }
    r.ops.flush();
    let listed = r.ops.call(&app(), ops::INBOX_LIST, &json!({})).unwrap();
    let events = changed(&r.room);
    for item in listed["items"].as_array().unwrap() {
        let last = events
            .iter()
            .rev()
            .find(|e| e["item"]["id"] == item["id"])
            .unwrap();
        assert_eq!(&last["item"], item, "{}", item["id"]);
    }
}

#[test]
fn close_drains_the_queue_and_ends_the_writer() {
    let r = rig();
    r.ops.raise(item("a"), None);
    r.ops.close();
    assert_eq!(changed(&r.room).len(), 1);
    assert_eq!(inbox::load(&r.dir, NOW + 1).unwrap().items().len(), 1);
    assert_eq!(Arc::strong_count(&r.room), 1);
    r.ops.close();
    r.ops.flush();
}

#[test]
fn dropping_without_close_still_ends_the_writer() {
    let tmp = TempDir::new("inbox-ops-drop");
    let room = Arc::new(FakeRoom::default());
    let ops = InboxOps::with_clock(tmp.path("data"), room.clone(), Arc::new(|| NOW));
    ops.raise(item("a"), None);
    drop(ops);
    let until = Instant::now() + Duration::from_secs(5);
    while Arc::strong_count(&room) > 1 && Instant::now() < until {
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(Arc::strong_count(&room), 1);
    assert_eq!(changed(&room).len(), 1);
}

#[test]
fn a_refusing_responder_leaves_the_item_open() {
    let r = rig();
    r.ops.raise(item("a"), Some(Arc::new(Refuses)));
    let out = answer(&r.ops, "a", "allow");
    assert_eq!(out["answered"], false);
    assert_eq!(out["state"], "open");
    assert_eq!(out["answer"], Value::Null);
    r.ops.flush();
    let list = r.ops.call(&app(), ops::INBOX_LIST, &json!({})).unwrap();
    assert_eq!(list["items"][0]["state"], "open");
    assert_eq!(list["items"][0]["answer"], Value::Null);
    r.ops
        .resolve("a", State::Answered, Some(ActionId::Deny), None);
    let out = answer(&r.ops, "a", "allow");
    assert_eq!(out["answered"], false);
    assert_eq!(out["state"], "answered");
    assert_eq!(out["answer"], "deny");
}

#[test]
fn a_phone_answer_is_recorded_as_the_phone_and_runs_the_responder() {
    let r = rig();
    let calls = Arc::new(Calls::default());
    r.ops.raise(item("a"), Some(calls.clone()));
    let out = r.ops.decide("a", ActionId::Deny, "phone").unwrap();
    assert!(out.answered);
    assert_eq!(out.state, State::Answered);
    assert_eq!(out.answer, Some(ActionId::Deny));
    assert_eq!(
        *calls.0.lock().unwrap(),
        vec![("a".to_string(), ActionId::Deny)]
    );
    let stored = r.ops.items();
    assert_eq!(stored[0].answered_by.as_deref(), Some("phone"));
}

#[test]
fn the_phone_loses_when_the_mac_answered_first() {
    let r = rig();
    let calls = Arc::new(Calls::default());
    r.ops.raise(item("a"), Some(calls.clone()));
    answer(&r.ops, "a", "allow");
    let out = r.ops.decide("a", ActionId::Deny, "phone").unwrap();
    assert!(!out.answered);
    assert_eq!(out.state, State::Answered);
    assert_eq!(out.answer, Some(ActionId::Allow));
    assert_eq!(calls.0.lock().unwrap().len(), 1);
    assert_eq!(r.ops.items()[0].answered_by.as_deref(), Some("mac"));
}

#[test]
fn a_phone_answer_the_responder_refuses_leaves_the_item_open() {
    let r = rig();
    r.ops.raise(item("a"), Some(Arc::new(Refuses)));
    let out = r.ops.decide("a", ActionId::Allow, "phone").unwrap();
    assert!(!out.answered);
    assert_eq!(out.state, State::Open);
    assert!(r.ops.decide("zzz", ActionId::Allow, "phone").is_err());
}
