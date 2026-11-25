use super::*;
use crate::acting_ops_tests::FakeRoom;
use crate::consent_testkit::{agent_caller, verified};
use crate::inbox_ops::InboxOps;
use crate::late_emit::Events;
use crate::limits_ops_tests::TempDir;
use crate::session_children::FREE_PER_HOUR;
use notchling_engine::consent::ConsentStore;
use notchling_engine::inbox::{ActionId, Item};
use notchling_events::{MemorySink, Salt};
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::Mutex;
use std::time::Duration;

const TEXT: &str = "please run the billing tests, then push";

#[derive(Default)]
struct FakeDir {
    sessions: Mutex<HashMap<String, Target>>,
    orgs: Mutex<HashMap<String, String>>,
    trusted: Mutex<HashSet<String>>,
    gone: Mutex<HashSet<String>>,
}

impl Directory for FakeDir {
    fn find(&self, id: &str) -> Option<Target> {
        self.sessions.lock().unwrap().get(id).cloned()
    }
    fn organization(&self, folder: &str) -> String {
        self.orgs
            .lock()
            .unwrap()
            .get(folder)
            .cloned()
            .unwrap_or_else(|| "Personal".to_string())
    }
    fn trusted(&self, _tool: &str, folder: &str) -> bool {
        self.trusted.lock().unwrap().contains(folder)
    }
    fn folder_exists(&self, folder: &str) -> bool {
        folder.starts_with("/work") && !self.gone.lock().unwrap().contains(folder)
    }
}

struct Rig {
    _tmp: TempDir,
    inbox: Arc<InboxOps>,
    sink: Arc<MemorySink>,
    room: Arc<FakeRoom>,
    consent: Arc<ConsentOps>,
    asks: Arc<SessionAsks>,
    dir: Arc<FakeDir>,
    calls: Arc<Mutex<Vec<(String, Value)>>>,
    acting: Arc<AtomicBool>,
    clock: Arc<AtomicI64>,
    fail_new_once: Arc<AtomicBool>,
    dialog: Arc<Mutex<Value>>,
    question: Arc<Mutex<Value>>,
}

impl Drop for Rig {
    fn drop(&mut self) {
        self.consent.close();
    }
}

fn target(id: &str, folder: &str) -> Target {
    Target {
        id: id.to_string(),
        tool: "claude",
        folder: folder.to_string(),
        name: format!("name of {id}"),
        remote: false,
        also: Vec::new(),
    }
}

fn kept(by: &str) -> Child {
    Child {
        started_by: by.to_string(),
        tool: "claude".to_string(),
        folder: "/work/a".to_string(),
        started_ms: 10,
        stopped_ms: None,
    }
}

fn rig(tag: &str) -> Rig {
    rig_with(tag, Arc::new(Children::in_memory()))
}

fn rig_with(tag: &str, children: Arc<Children>) -> Rig {
    let tmp = TempDir::new(&format!("session-asks-{tag}"));
    let room = Arc::new(FakeRoom::default());
    let inbox = InboxOps::new(tmp.path("inbox"), room.clone());
    let sink = Arc::new(MemorySink::new());
    let consent = ConsentOps::with_clock(
        Arc::new(ConsentStore::in_memory_for_tests()),
        room.clone(),
        Some(inbox.clone()),
        Some(Events::fixed(sink.clone(), Salt([3; 32]))),
        Arc::new(now_ms),
    );
    let mut me = verified("parent");
    me.organization = Some("Personal".to_string());
    let who = Arc::new(Mutex::new(me));
    let seen = who.clone();
    consent.bind_identity(Arc::new(move |_| seen.lock().unwrap().clone()));
    consent.start();
    let dir = Arc::new(FakeDir::default());
    for (id, folder) in [
        ("parent", "/work/a"),
        ("kid", "/work/a"),
        ("grand", "/work/a"),
        ("other", "/work/a"),
        ("abroad", "/work/b"),
    ] {
        dir.sessions
            .lock()
            .unwrap()
            .insert(id.to_string(), target(id, folder));
    }
    dir.orgs
        .lock()
        .unwrap()
        .insert("/work/b".to_string(), "Work".to_string());
    let calls: Arc<Mutex<Vec<(String, Value)>>> = Arc::new(Mutex::new(Vec::new()));
    let fail_new_once = Arc::new(AtomicBool::new(false));
    let (log, fail) = (calls.clone(), fail_new_once.clone());
    let dialog = Arc::new(Mutex::new(Value::Null));
    let question = Arc::new(Mutex::new(json!({"items": []})));
    let (seen_dialog, seen_question) = (dialog.clone(), question.clone());
    let run: Runner = Arc::new(move |op: &str, args: &Value| {
        log.lock().unwrap().push((op.to_string(), args.clone()));
        if op == ops::SESSION_NEW && fail.swap(false, Ordering::SeqCst) {
            return Err(ApiError::new(ErrorCode::Failed, "not trusted"));
        }
        Ok(match op {
            ops::SESSION_NEW => json!({"jobId": "job-new"}),
            ops::SESSION_DIALOG => json!({"dialog": seen_dialog.lock().unwrap().clone()}),
            ops::SESSION_QUESTION => seen_question.lock().unwrap().clone(),
            _ => json!({"ok": true}),
        })
    });
    let acting = Arc::new(AtomicBool::new(true));
    let clock = Arc::new(AtomicI64::new(1_000));
    let (flag, ticks) = (acting.clone(), clock.clone());
    let asks = SessionAsks::with_clock(
        consent.clone(),
        dir.clone(),
        run,
        children,
        Arc::new(move || flag.load(Ordering::SeqCst)),
        Arc::new(move || ticks.load(Ordering::SeqCst)),
    );
    Rig {
        _tmp: tmp,
        inbox,
        sink,
        room,
        consent,
        asks,
        dir,
        calls,
        acting,
        clock,
        fail_new_once,
        dialog,
        question,
    }
}

impl Rig {
    fn call(&self, op: &str, args: Value) -> Result<Value, ApiError> {
        self.asks.call(&agent_caller(), op, &args)
    }

    fn ops_run(&self) -> Vec<String> {
        self.calls
            .lock()
            .unwrap()
            .iter()
            .map(|(op, _)| op.clone())
            .collect()
    }

    fn cards(&self) -> usize {
        self.consent.store().open_rows().unwrap().len()
    }

    fn card_of(&self, reply: &Value) -> String {
        assert_eq!(reply["state"], "waiting", "{reply}");
        reply["request"]["id"].as_str().unwrap().to_string()
    }

    fn allow(&self, id: &str, by: &str) -> Consent {
        let said = self.consent.decide(id, true, None, by, None).unwrap();
        assert_eq!(said["answered"], json!(true));
        self.consent.wait_for(id, Duration::from_secs(5)).unwrap()
    }

    fn item(&self, id: &str) -> Item {
        self.inbox.flush();
        self.inbox
            .items()
            .into_iter()
            .find(|i| notchling_engine::consent::consent_of(&i.id) == Some(id))
            .expect("an inbox item")
    }

    fn make_child(&self, id: &str, by: &str) {
        self.asks.children().record(id, kept(by));
    }
}

fn reason(e: &ApiError) -> String {
    e.data
        .as_ref()
        .and_then(|d| d["reason"].as_str())
        .unwrap_or_default()
        .to_string()
}

fn field(e: &ApiError, key: &str) -> Value {
    e.data
        .as_ref()
        .map(|d| d[key].clone())
        .unwrap_or(Value::Null)
}

#[test]
fn a_start_opens_a_card_and_allowing_it_records_the_child_and_sends_the_first_message() {
    let r = rig("start");
    r.dir.trusted.lock().unwrap().insert("/work/a".to_string());
    let reply = r
        .call(
            ops::SESSION_START,
            json!({"folder": "/work/a", "message": TEXT, "model": "sonnet"}),
        )
        .unwrap();
    let id = r.card_of(&reply);
    assert_eq!(reply["request"]["kind"], "session");
    assert!(r.ops_run().is_empty(), "asking runs nothing");
    let lines: Vec<&str> = reply["request"]["facts"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|f| f["line"].as_str())
        .collect();
    assert_eq!(
        lines,
        [
            "consent-session-start",
            "consent-session-model",
            "consent-session-child"
        ]
    );
    assert_eq!(reply["request"]["facts"][1]["args"]["model"], "sonnet");
    let waiting = r.consent.store().get(&id).unwrap().unwrap();
    assert_eq!(waiting.change["message"], TEXT);
    assert_eq!(waiting.change["organization"], "Personal");
    let done = r.allow(&id, "mac");
    assert_eq!(done.state.as_str(), "applied");
    let calls = r.calls.lock().unwrap().clone();
    assert_eq!(calls[0].0, ops::SESSION_NEW);
    assert_eq!(calls[0].1["folder"], "/work/a");
    assert_eq!(calls[0].1["flags"], json!(["--model", "sonnet"]));
    assert_eq!(calls[1].0, ops::SESSION_SEND);
    assert_eq!(calls[1].1, json!({"id": "job-new", "text": TEXT}));
    let child = r.asks.children().get("job-new").expect("recorded");
    assert_eq!(child.started_by, "parent");
    assert_eq!(done.change.get("message"), None, "the text goes at the end");
}

#[test]
fn an_untrusted_folder_is_trusted_by_the_allowed_start_only() {
    let r = rig("trusts");
    let reply = r
        .call(ops::SESSION_START, json!({"folder": "/work/a"}))
        .unwrap();
    let id = r.card_of(&reply);
    let lines: Vec<&str> = reply["request"]["facts"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|f| f["line"].as_str())
        .collect();
    assert!(lines.contains(&"consent-session-trusts"));
    assert!(r.ops_run().is_empty());
    r.fail_new_once.store(true, Ordering::SeqCst);
    assert_eq!(r.allow(&id, "mac").state.as_str(), "applied");
    assert_eq!(
        r.ops_run(),
        [ops::SESSION_NEW, ops::FOLDER_TRUST, ops::SESSION_NEW]
    );
}

#[test]
fn a_start_for_a_folder_that_vanished_is_withdrawn_not_run() {
    let r = rig("vanish");
    let reply = r
        .call(ops::SESSION_START, json!({"folder": "/work/a"}))
        .unwrap();
    let id = r.card_of(&reply);
    r.dir.gone.lock().unwrap().insert("/work/a".to_string());
    let _ = r.consent.decide(&id, true, None, "mac", None);
    let row = r.consent.wait_for(&id, Duration::from_secs(5)).unwrap();
    assert_eq!(row.state.as_str(), "withdrawn");
    assert!(r.ops_run().is_empty());
}

#[test]
fn a_bad_start_is_a_bad_request() {
    let r = rig("bad-start");
    for args in [
        json!({}),
        json!({"folder": "relative/path"}),
        json!({"folder": "/work/a", "tool": "vim"}),
        json!({"folder": "/work/a", "effort": "high"}),
        json!({"folder": "/work/a", "message": "x".repeat(MESSAGE_BYTES + 1)}),
        json!({"folder": "/nowhere/else"}),
    ] {
        let e = r.call(ops::SESSION_START, args.clone()).unwrap_err();
        assert_eq!(e.code, ErrorCode::BadRequest, "{args}");
    }
    let later = r
        .call(
            ops::SESSION_START,
            json!({"folder": "/work/a", "tool": "codex"}),
        )
        .unwrap_err();
    assert_eq!(reason(&later), "not_yet");
    assert_eq!(r.cards(), 0);
}

#[test]
fn say_on_a_child_is_free_on_itself_refused_and_on_another_session_a_card() {
    let r = rig("say");
    r.make_child("kid", "parent");
    let free = r
        .call(ops::SESSION_SAY, json!({"id": "kid", "text": TEXT}))
        .unwrap();
    assert_eq!(free["free"], json!(true));
    assert_eq!(r.ops_run(), [ops::SESSION_SEND]);
    assert_eq!(
        r.calls.lock().unwrap()[0].1,
        json!({"id": "kid", "text": TEXT})
    );
    assert_eq!(r.cards(), 0);
    let own = r
        .call(ops::SESSION_SAY, json!({"id": "parent", "text": TEXT}))
        .unwrap_err();
    assert_eq!(own.code, ErrorCode::BadRequest);
    let other = r
        .call(ops::SESSION_SAY, json!({"id": "other", "text": TEXT}))
        .unwrap();
    let id = r.card_of(&other);
    assert_eq!(other["request"]["kind"], "session");
    assert_eq!(r.ops_run().len(), 1, "no second send before a yes");
    assert_eq!(r.allow(&id, "mac").state.as_str(), "applied");
    assert_eq!(r.ops_run(), [ops::SESSION_SEND, ops::SESSION_SEND]);
    assert_eq!(
        r.calls.lock().unwrap()[1].1,
        json!({"id": "other", "text": TEXT})
    );
}

#[test]
fn stop_and_interrupt_are_free_on_a_child_and_on_itself_and_a_card_on_another() {
    let r = rig("halt");
    r.make_child("kid", "parent");
    for (id, how) in [
        ("kid", "stop"),
        ("parent", "stop"),
        ("kid", "interrupt"),
        ("parent", "interrupt"),
    ] {
        let said = r
            .call(ops::SESSION_HALT, json!({"id": id, "how": how}))
            .unwrap();
        assert_eq!(said["free"], json!(true), "{id} {how}");
    }
    assert_eq!(
        r.ops_run(),
        [
            ops::SESSION_STOP,
            ops::SESSION_STOP,
            ops::SESSION_INTERRUPT,
            ops::SESSION_INTERRUPT
        ]
    );
    assert!(r.asks.children().get("kid").unwrap().stopped_ms.is_some());
    assert_eq!(r.cards(), 0);
    let other = r
        .call(ops::SESSION_HALT, json!({"id": "other", "how": "stop"}))
        .unwrap();
    let id = r.card_of(&other);
    assert_eq!(other["request"]["kind"], "session_stop");
    assert_eq!(r.ops_run().len(), 4);
    assert_eq!(r.allow(&id, "mac").state.as_str(), "applied");
    assert_eq!(
        r.ops_run().last().map(String::as_str),
        Some(ops::SESSION_STOP)
    );
    let bad = r
        .call(ops::SESSION_HALT, json!({"id": "kid", "how": "kill"}))
        .unwrap_err();
    assert_eq!(bad.code, ErrorCode::BadRequest);
}

#[test]
fn remove_is_free_on_a_stopped_child_and_a_card_on_anything_else() {
    let r = rig("remove");
    r.make_child("kid", "parent");
    let early = r
        .call(ops::SESSION_DISCARD, json!({"id": "kid"}))
        .unwrap_err();
    assert_eq!(early.code, ErrorCode::BadRequest);
    r.call(ops::SESSION_HALT, json!({"id": "kid", "how": "stop"}))
        .unwrap();
    let free = r.call(ops::SESSION_DISCARD, json!({"id": "kid"})).unwrap();
    assert_eq!(free["free"], json!(true));
    assert_eq!(
        r.ops_run().last().map(String::as_str),
        Some(ops::SESSION_REMOVE)
    );
    assert!(r.asks.children().get("kid").is_none());
    let own = r
        .call(ops::SESSION_DISCARD, json!({"id": "parent"}))
        .unwrap_err();
    assert_eq!(own.code, ErrorCode::BadRequest);
    let other = r
        .call(ops::SESSION_DISCARD, json!({"id": "other"}))
        .unwrap();
    let id = r.card_of(&other);
    assert_eq!(other["request"]["kind"], "session");
    let before = r.ops_run().len();
    assert_eq!(r.allow(&id, "mac").state.as_str(), "applied");
    assert_eq!(r.ops_run().len(), before + 1);
}

#[test]
fn the_thirty_first_free_message_in_an_hour_is_busy_with_a_wait() {
    let r = rig("limit");
    r.make_child("kid", "parent");
    for i in 0..FREE_PER_HOUR {
        r.clock.store(1_000 + i as i64, Ordering::SeqCst);
        r.call(ops::SESSION_SAY, json!({"id": "kid", "text": "hi"}))
            .unwrap();
    }
    r.clock.store(60_000, Ordering::SeqCst);
    let busy = r
        .call(ops::SESSION_SAY, json!({"id": "kid", "text": "hi"}))
        .unwrap_err();
    assert_eq!(reason(&busy), "busy");
    let wait = field(&busy, "retryAfterMs").as_i64().unwrap();
    assert!(wait > 0 && wait <= 3_600_000, "{wait}");
    assert_eq!(r.ops_run().len(), FREE_PER_HOUR);
    r.clock.store(3_700_000, Ordering::SeqCst);
    r.call(ops::SESSION_SAY, json!({"id": "kid", "text": "hi"}))
        .unwrap();
}

#[test]
fn a_child_stays_a_child_after_a_restart() {
    let tmp = TempDir::new("session-asks-restart-book");
    let file = tmp.path("daemon").join("session-children.json");
    let r = rig_with("restart", Arc::new(Children::open(file.clone())));
    let reply = r
        .call(ops::SESSION_START, json!({"folder": "/work/a"}))
        .unwrap();
    let id = r.card_of(&reply);
    r.allow(&id, "mac");
    assert!(r.asks.children().owned_by("parent", "job-new").is_some());
    let again = Children::open(file);
    let child = again.owned_by("parent", "job-new").expect("kept");
    assert_eq!(child.folder, "/work/a");
    assert!(again.owned_by("grandparent", "job-new").is_none());
}

#[test]
fn a_grandchild_is_not_owned_by_the_grandparent() {
    let r = rig("grand");
    r.make_child("kid", "parent");
    r.make_child("grand", "kid");
    let said = r
        .call(ops::SESSION_SAY, json!({"id": "grand", "text": TEXT}))
        .unwrap();
    r.card_of(&said);
    let halted = r
        .call(ops::SESSION_HALT, json!({"id": "grand", "how": "stop"}))
        .unwrap();
    assert_eq!(halted["request"]["kind"], "session_stop");
    assert!(r.ops_run().is_empty());
}

#[test]
fn another_organization_is_refused_before_any_card() {
    let r = rig("org");
    r.dir.trusted.lock().unwrap().insert("/work/b".to_string());
    let start = r
        .call(ops::SESSION_START, json!({"folder": "/work/b"}))
        .unwrap_err();
    assert_eq!(start.code, ErrorCode::Forbidden);
    assert_eq!(reason(&start), "other_organization");
    r.make_child("abroad", "parent");
    for (op, args) in [
        (ops::SESSION_SAY, json!({"id": "abroad", "text": TEXT})),
        (ops::SESSION_HALT, json!({"id": "abroad", "how": "stop"})),
        (ops::SESSION_DISCARD, json!({"id": "abroad"})),
    ] {
        let e = r.call(op, args).unwrap_err();
        assert_eq!(reason(&e), "other_organization", "{op}");
    }
    assert_eq!(r.cards(), 0);
    assert!(r.ops_run().is_empty());
}

#[test]
fn a_start_that_skips_permission_prompts_is_never() {
    let r = rig("never");
    for key in [
        "permissionMode",
        "permission_mode",
        "skipPermissions",
        "dangerouslySkipPermissions",
        "flags",
        "yolo",
    ] {
        let e = r
            .call(
                ops::SESSION_START,
                json!({"folder": "/work/a", key: "bypassPermissions"}),
            )
            .unwrap_err();
        assert_eq!(e.code, ErrorCode::Forbidden, "{key}");
        assert_eq!(reason(&e), "never", "{key}");
    }
    r.make_child("kid", "parent");
    let say = r
        .call(
            ops::SESSION_SAY,
            json!({"id": "kid", "text": "go", "permissionMode": "bypassPermissions"}),
        )
        .unwrap_err();
    assert_eq!(reason(&say), "never");
    assert_eq!(r.cards(), 0);
    assert!(r.ops_run().is_empty());
}

#[test]
fn with_acting_off_every_op_says_to_switch_it_on() {
    let r = rig("acting-off");
    r.make_child("kid", "parent");
    r.acting.store(false, Ordering::SeqCst);
    let attempts = [
        (ops::SESSION_START, json!({"folder": "/work/a"})),
        (ops::SESSION_SAY, json!({"id": "kid", "text": "hi"})),
        (ops::SESSION_HALT, json!({"id": "kid", "how": "stop"})),
        (ops::SESSION_DISCARD, json!({"id": "kid"})),
    ];
    for (op, args) in attempts {
        let e = r.call(op, args).unwrap_err();
        assert_eq!(e.code, ErrorCode::Forbidden, "{op}");
        assert_eq!(reason(&e), "switch_off", "{op}");
        assert_eq!(
            field(&e, "ask"),
            json!({"command": "notchling switch on acting", "kind": "switch"})
        );
    }
    assert_eq!(r.cards(), 0);
    assert!(r.ops_run().is_empty());
}

#[test]
fn a_yes_after_acting_went_off_is_withdrawn() {
    let r = rig("acting-meanwhile");
    let reply = r
        .call(ops::SESSION_SAY, json!({"id": "other", "text": TEXT}))
        .unwrap();
    let id = r.card_of(&reply);
    r.acting.store(false, Ordering::SeqCst);
    let _ = r.consent.decide(&id, true, None, "mac", None);
    let row = r.consent.wait_for(&id, Duration::from_secs(5)).unwrap();
    assert_eq!(row.state.as_str(), "withdrawn");
    assert!(r.ops_run().is_empty());
}

#[test]
fn a_phone_may_allow_a_stop_but_not_a_session_and_may_deny_either() {
    let r = rig("phone");
    let say = r
        .call(ops::SESSION_SAY, json!({"id": "other", "text": TEXT}))
        .unwrap();
    let said = r.card_of(&say);
    let refused = r
        .consent
        .decide(&said, true, None, "phone", None)
        .unwrap_err();
    assert_eq!(refused.code, ErrorCode::Forbidden);
    assert_eq!(
        r.consent
            .store()
            .get(&said)
            .unwrap()
            .unwrap()
            .state
            .as_str(),
        "waiting"
    );
    assert!(r.ops_run().is_empty());
    let stop = r
        .call(ops::SESSION_HALT, json!({"id": "other", "how": "stop"}))
        .unwrap();
    let stopped = r.card_of(&stop);
    assert_eq!(r.allow(&stopped, "phone").state.as_str(), "applied");
    assert_eq!(r.ops_run(), [ops::SESSION_STOP]);
    let denied = r.consent.decide(&said, false, None, "phone", None).unwrap();
    assert_eq!(denied["answered"], json!(true));
    assert_eq!(
        r.consent
            .store()
            .get(&said)
            .unwrap()
            .unwrap()
            .state
            .as_str(),
        "denied"
    );
    assert_eq!(r.ops_run().len(), 1);
}

#[test]
fn the_text_is_on_the_card_and_in_the_session_but_never_in_an_event_or_the_inbox() {
    let r = rig("privacy");
    let say = r
        .call(
            ops::SESSION_SAY,
            json!({"id": "other", "text": TEXT, "consent": {"reason": "needed"}}),
        )
        .unwrap();
    let id = r.card_of(&say);
    let card = r
        .consent
        .project(&r.consent.store().get(&id).unwrap().unwrap(), false);
    assert_eq!(card["change"]["message"], TEXT);
    let item = r.item(&id);
    assert!(!item.title.contains("billing") && !item.detail.contains("billing"));
    r.allow(&id, "mac");
    let done = r.consent.store().get(&id).unwrap().unwrap();
    assert_eq!(done.change.get("message"), None);
    let agent_view = r.consent.project(&done, true);
    assert!(!agent_view.to_string().contains("billing"));
    let events = format!("{:?}", r.sink.events());
    assert!(!events.contains("billing"), "an event carries the text");
    assert!(!format!("{:?}", r.room.events.lock().unwrap()).contains("billing"));
    let inbox = serde_json::to_string(&r.inbox.items()).unwrap();
    assert!(!inbox.contains("billing"), "the inbox carries the text");
    let sent = r.calls.lock().unwrap().clone();
    assert_eq!(sent[0].1["text"], TEXT, "the session got the text");
}

#[test]
fn the_text_goes_from_the_store_when_a_card_is_denied() {
    let r = rig("deny-wipe");
    let say = r
        .call(ops::SESSION_SAY, json!({"id": "other", "text": TEXT}))
        .unwrap();
    let id = r.card_of(&say);
    r.consent.decide(&id, false, None, "mac", None).unwrap();
    let row = r.consent.store().get(&id).unwrap().unwrap();
    assert_eq!(row.state.as_str(), "denied");
    assert_eq!(row.change.get("message"), None);
    assert_eq!(row.change["action"], "say");
    assert!(r.ops_run().is_empty());
}

#[test]
fn the_inbox_item_offers_only_the_existing_actions_and_a_mirror_keeps_allow_and_deny() {
    let r = rig("mirror");
    let say = r
        .call(ops::SESSION_SAY, json!({"id": "other", "text": TEXT}))
        .unwrap();
    let said = r.card_of(&say);
    let stop = r
        .call(ops::SESSION_HALT, json!({"id": "other", "how": "stop"}))
        .unwrap();
    let stopped = r.card_of(&stop);
    let owner = crate::cluster_inbox_testkit::node(9);
    for (id, raw, mirrored) in [
        (
            &said,
            vec![ActionId::Deny, ActionId::Open],
            vec![ActionId::Deny],
        ),
        (
            &stopped,
            vec![ActionId::Allow, ActionId::Deny],
            vec![ActionId::Allow, ActionId::Deny],
        ),
    ] {
        let item = r.item(id);
        let ids: Vec<ActionId> = item.actions.iter().map(|a| a.id).collect();
        assert_eq!(ids, raw);
        let shown = crate::inbox_mirrors::display(&owner, item).unwrap();
        let kept: Vec<ActionId> = shown.actions.iter().map(|a| a.id).collect();
        assert_eq!(kept, mirrored);
    }
}

#[path = "session_acts_tests.rs"]
mod acts;

#[path = "session_dry_tests.rs"]
mod dry;
