use super::*;
use crate::switches::VAULT;
use notchling_engine::vault::list::VaultIndex;
use notchling_engine::vault::KeychainError;
use notchling_events::{MemorySink, Salt};
use notchling_local_api::ClientKind;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

const TOKEN: &str = "fake-token-value-0001";
const PASSWORD: &str = "fake-password-0002";

#[derive(Default)]
pub(crate) struct FakeRoom {
    pub events: Mutex<Vec<(String, String, Value)>>,
    pub follows: AtomicBool,
}

impl Room for FakeRoom {
    fn publish(&self, topic: &str, event: &str, data: &Value) {
        let mut events = self.events.lock().unwrap();
        events.push((topic.into(), event.into(), data.clone()));
    }
    fn can_answer(&self) -> bool {
        self.follows.load(Ordering::SeqCst)
    }
    fn can_follow(&self, topic: &str) -> bool {
        topic == topics::VAULT && self.follows.load(Ordering::SeqCst)
    }
}

impl FakeRoom {
    pub(super) fn pending(&self, event: &str) -> Option<Value> {
        let events = self.events.lock().unwrap();
        events
            .iter()
            .rev()
            .find(|e| e.1 == event)
            .map(|e| e.2.clone())
    }
}

#[derive(Clone, Default)]
pub(super) struct FakeItem {
    data: Arc<Mutex<Option<Vec<u8>>>>,
    reads: Arc<Mutex<u32>>,
}

impl VaultKeychain for FakeItem {
    fn read(&self) -> Result<Option<Vec<u8>>, KeychainError> {
        *self.reads.lock().unwrap() += 1;
        Ok(self.data.lock().unwrap().clone())
    }
    fn write(&self, data: &[u8]) -> Result<(), KeychainError> {
        *self.data.lock().unwrap() = Some(data.to_vec());
        Ok(())
    }
}

pub(super) struct Setup {
    pub(super) ops: Arc<VaultOps>,
    pub(super) room: Arc<FakeRoom>,
    pub(super) item: FakeItem,
    pub(super) switch: Arc<SwitchFlag>,
    pub(super) sink: Arc<MemorySink>,
    dir: std::path::PathBuf,
}

impl Drop for Setup {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

impl Setup {
    fn list_text(&self) -> String {
        notchling_engine::store::doc(&self.dir, notchling_engine::vault::list::INDEX_DOC)
            .unwrap()
            .unwrap_or_default()
    }
    fn audit(&self) -> String {
        format!("{:?}", self.sink.events())
    }
    fn outside(&self) -> String {
        let events = format!("{:?}", self.room.events.lock().unwrap());
        let list = self.list_text();
        format!("{events}\n{}\n{list}", self.audit())
    }
}

pub(super) fn setup(tag: &str, on: bool) -> Setup {
    setup_with(tag, on, None, None)
}

pub(super) fn setup_with(
    tag: &str,
    on: bool,
    inbox: Option<Arc<crate::inbox_ops::InboxOps>>,
    timing: Option<VaultTiming>,
) -> Setup {
    let dir = std::env::temp_dir().join(format!("nl-vault-ops-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let item = FakeItem::default();
    let room = Arc::new(FakeRoom::default());
    let switch = SwitchFlag::off(VAULT, VAULT_OFF);
    switch.set(on);
    let store = VaultStore::new(VaultIndex::in_data(&dir), Box::new(item.clone()));
    let timing = timing.unwrap_or(VaultTiming {
        approval_wait: Duration::from_secs(10),
        request_wait: Duration::from_secs(10),
        slice: Duration::from_millis(10),
    });
    let sink = Arc::new(MemorySink::new());
    let mut ops = VaultOps::new(switch.clone(), store, room.clone())
        .with_events(Events::fixed(sink.clone(), Salt([1; 32])))
        .with_timing(timing)
        .with_processes(Arc::new(super::pending_tests::FakeProcesses));
    if let Some(inbox) = inbox {
        ops = ops.with_inbox(inbox);
    }
    Setup {
        ops: Arc::new(ops),
        room,
        item,
        switch,
        sink,
        dir,
    }
}

fn caller(tier: Tier, client: ClientKind) -> Caller {
    Caller {
        tier,
        client,
        peer: Some("/fake/peer/notchling".into()),
        pid: Some(std::process::id()),
    }
}

pub(super) fn app() -> Caller {
    caller(Tier::App, ClientKind::App)
}

pub(super) fn agent() -> Caller {
    caller(Tier::Agent, ClientKind::Cli)
}

pub(super) fn seed(s: &Setup, ask: bool) {
    let gh = json!({"name": "gh", "kind": "token", "url": "https://api.github.com", "ask": ask});
    s.ops
        .call(
            &app(),
            ops::VAULT_SAVE,
            &json!({"entry": gh, "value": {"secret": TOKEN}}),
        )
        .unwrap();
    let reg = json!({"name": "reg", "kind": "login"});
    let value = json!({"username": "fake-user", "secret": PASSWORD});
    s.ops
        .call(
            &app(),
            ops::VAULT_SAVE,
            &json!({"entry": reg, "value": value}),
        )
        .unwrap();
}

pub(super) fn err(result: Result<Value, ApiError>) -> ApiError {
    result.unwrap_err()
}

#[test]
fn the_tiers_of_each_op() {
    let s = setup("tiers", false);
    for op in [ops::VAULT_FIND, ops::VAULT_USE, ops::VAULT_REQUEST] {
        assert_eq!(s.ops.spec(op), Some(OpSpec::AGENT), "{op}");
    }
    for op in [
        ops::VAULT_LIST,
        ops::VAULT_SAVE,
        ops::VAULT_REMOVE,
        ops::VAULT_APPROVAL_ANSWER,
        ops::VAULT_REQUEST_CANCEL,
    ] {
        assert_eq!(s.ops.spec(op), Some(OpSpec::APP), "{op}");
    }
    assert_eq!(s.ops.spec("vault.read"), None);
}

#[test]
fn while_off_every_op_says_so_and_nothing_is_read_or_written() {
    let s = setup("off", false);
    let args = json!({"names": ["gh"], "name": "gh", "query": "", "id": "x", "decision": "once",
                      "entry": {"name": "gh", "kind": "token"}, "value": {"secret": TOKEN}});
    for op in VAULT_OPS {
        for who in [app(), agent()] {
            let e = err(s.ops.call(&who, op, &args));
            assert_eq!(
                (e.code, e.message.as_str()),
                (ErrorCode::Forbidden, VAULT_OFF),
                "{op}"
            );
        }
    }
    assert_eq!(
        *s.item.reads.lock().unwrap(),
        0,
        "the Keychain item is never opened"
    );
    assert!(s.item.data.lock().unwrap().is_none());
    assert!(
        !notchling_engine::store::exists(&s.dir),
        "no vault list, no database"
    );
    assert!(!s.dir.join("daemon/vault.json").exists());
    assert!(s.room.events.lock().unwrap().is_empty());
    assert!(s.sink.events().is_empty(), "an off vault records no use");
}

#[test]
fn turning_it_off_drops_the_values_read_while_on() {
    let s = setup("forget", true);
    seed(&s, false);
    let use_gh = json!({"names": ["gh"], "folder": "/w", "command": "true"});
    s.ops.call(&agent(), ops::VAULT_USE, &use_gh).unwrap();
    let reads = *s.item.reads.lock().unwrap();
    s.switch.set(false);
    assert_eq!(
        err(s.ops.call(&agent(), ops::VAULT_USE, &use_gh)).message,
        VAULT_OFF
    );
    s.switch.set(true);
    s.ops.call(&agent(), ops::VAULT_USE, &use_gh).unwrap();
    assert_eq!(*s.item.reads.lock().unwrap(), reads + 1, "read again");
}

#[test]
fn the_switch_going_off_forgets_and_ends_the_waits_at_once_with_no_call() {
    let s = setup("forget-now", true);
    seed(&s, true);
    let use_reg = json!({"names": ["reg"], "folder": "/w", "command": "true"});
    s.ops.call(&agent(), ops::VAULT_USE, &use_reg).unwrap();
    let reads = *s.item.reads.lock().unwrap();
    s.switch.set(false);
    s.switch.set(true);
    s.ops.call(&agent(), ops::VAULT_USE, &use_reg).unwrap();
    assert_eq!(*s.item.reads.lock().unwrap(), reads + 1, "read again");

    let (user, _) = pending_use(&s);
    assert_eq!(s.ops.approvals.len(), 1);
    s.switch.set(false);
    assert!(
        s.ops.approvals.is_empty(),
        "ended by the switch, not a tick"
    );
    assert!(user.join().unwrap().is_err());
}

#[test]
fn find_lists_entries_and_never_values() {
    let s = setup("find", true);
    seed(&s, false);
    let reply = s
        .ops
        .call(
            &agent(),
            ops::VAULT_FIND,
            &json!({"query": "api.github.com"}),
        )
        .unwrap();
    assert_eq!(reply["entries"][0]["name"], "gh");
    assert_eq!(reply["saved"], json!(["gh (api.github.com)", "reg"]));
    let text = reply.to_string();
    assert!(!text.contains(TOKEN) && !text.contains(PASSWORD));
    let all = s.ops.call(&app(), ops::VAULT_LIST, &json!({})).unwrap();
    assert_eq!(all["entries"].as_array().unwrap().len(), 2);
    assert!(!all.to_string().contains(TOKEN));
}

#[test]
fn use_hands_over_the_env_and_what_to_hide() {
    let s = setup("use", true);
    seed(&s, false);
    let reply = s
        .ops
        .call(
            &agent(),
            ops::VAULT_USE,
            &json!({"names": ["GH", "reg", "gh"], "folder": "/w", "command": "make"}),
        )
        .unwrap();
    assert_eq!(reply["env"]["GH_TOKEN"], TOKEN);
    assert_eq!(reply["env"]["GH_URL"], "https://api.github.com");
    assert_eq!(reply["env"]["REG_USERNAME"], "fake-user");
    assert_eq!(reply["env"]["REG_PASSWORD"], PASSWORD);
    assert_eq!(reply["hide"][TOKEN], "[secret:gh]");
    let used = crate::event_assert::of(&s.sink, "vault.secret_used");
    assert_eq!(used.len(), 1);
    assert_eq!(
        crate::event_assert::text(&used[0], "result").as_deref(),
        Some("given")
    );
    assert!(s.audit().contains("gh") && s.audit().contains("reg"));
    assert!(!s.outside().contains(TOKEN) && !s.outside().contains(PASSWORD));
}

#[test]
fn a_missing_name_is_said_without_repeating_what_isnt_a_name() {
    let s = setup("missing", true);
    seed(&s, false);
    let e = err(s
        .ops
        .call(&agent(), ops::VAULT_USE, &json!({"names": ["npm"]})));
    assert_eq!(
        e.message,
        "No secret named npm. Saved: gh, reg. Ask the user for it with secret-request."
    );
    let pasted = "ghp_fakePastedValue 123";
    let e = err(s
        .ops
        .call(&agent(), ops::VAULT_USE, &json!({"names": [pasted]})));
    assert!(!e.message.contains(pasted), "{}", e.message);
    assert!(!s.audit().contains(pasted));
    assert_eq!(
        err(s.ops.call(&agent(), ops::VAULT_USE, &json!({"names": []}))).code,
        ErrorCode::BadRequest
    );
}

#[test]
fn ask_first_with_no_app_is_denied_without_reading_a_value() {
    let s = setup("noapp", true);
    seed(&s, true);
    s.ops.store.forget();
    let reads = *s.item.reads.lock().unwrap();
    let e = err(s
        .ops
        .call(&agent(), ops::VAULT_USE, &json!({"names": ["gh"]})));
    assert_eq!(e.code, ErrorCode::NeedsApproval);
    assert!(e.message.contains("isn't open"), "{}", e.message);
    assert_eq!(*s.item.reads.lock().unwrap(), reads, "no value read");
    assert!(s.room.pending(topics::VAULT_APPROVAL_PENDING).is_none());
    assert_eq!(
        crate::event_assert::results(&s.sink, "vault.secret_used"),
        ["no_app"]
    );
}

pub(super) fn pending_use(s: &Setup) -> (std::thread::JoinHandle<Result<Value, ApiError>>, Value) {
    s.room.follows.store(true, Ordering::SeqCst);
    let asked = || {
        let events = s.room.events.lock().unwrap();
        events
            .iter()
            .filter(|e| e.1 == topics::VAULT_APPROVAL_PENDING)
            .count()
    };
    let before = asked();
    let ops = s.ops.clone();
    let user = std::thread::spawn(move || {
        let args = json!({"names": ["gh"], "folder": "/w/app", "command": "curl \"$GH_URL\""});
        ops.call(&agent(), ops::VAULT_USE, &args)
    });
    for _ in 0..500 {
        if asked() > before {
            let event = s.room.pending(topics::VAULT_APPROVAL_PENDING).unwrap();
            return (user, event);
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    panic!("no approval asked");
}

#[test]
fn the_app_approves_once_with_the_id_it_was_sent() {
    let s = setup("once", true);
    seed(&s, true);
    let (user, event) = pending_use(&s);
    assert_eq!(event["names"], json!(["gh"]));
    assert_eq!(event["command"], "curl \"$GH_URL\"");
    assert_eq!(event["folder"], "/w/app");
    let id = event["id"].as_str().unwrap().to_string();
    assert_eq!(id.len(), "vault-approval-".len() + 32);
    let answer = |d: &str| json!({"id": id, "decision": d});
    for who in [
        agent(),
        caller(Tier::App, ClientKind::Cli),
        caller(Tier::Agent, ClientKind::App),
    ] {
        let e = err(s
            .ops
            .call(&who, ops::VAULT_APPROVAL_ANSWER, &answer("once")));
        assert_eq!(e.code, ErrorCode::Forbidden);
    }
    let guess =
        json!({"id": "vault-approval-00000000000000000000000000000000", "decision": "once"});
    let got = s
        .ops
        .call(&app(), ops::VAULT_APPROVAL_ANSWER, &guess)
        .unwrap();
    assert_eq!(got["answered"], false);
    let bad = json!({"id": id, "decision": "yes"});
    assert_eq!(
        err(s.ops.call(&app(), ops::VAULT_APPROVAL_ANSWER, &bad)).code,
        ErrorCode::BadRequest
    );
    let got = s
        .ops
        .call(&app(), ops::VAULT_APPROVAL_ANSWER, &answer("once"))
        .unwrap();
    assert_eq!(got["answered"], true);
    let reply = user.join().unwrap().unwrap();
    assert_eq!(reply["env"]["GH_TOKEN"], TOKEN);
    assert_eq!(
        s.room.pending(topics::VAULT_APPROVAL_RESOLVED).unwrap()["decision"],
        "once"
    );
    assert_eq!(s.ops.store.entry("gh").unwrap().unwrap().ask, Some(true));
    let again = s
        .ops
        .call(&app(), ops::VAULT_APPROVAL_ANSWER, &answer("once"))
        .unwrap();
    assert_eq!(again["answered"], false, "an id answers once");
    assert!(!s.outside().contains(TOKEN));
}

#[test]
fn deny_and_always() {
    let s = setup("deny", true);
    seed(&s, true);
    let (user, event) = pending_use(&s);
    let id = event["id"].clone();
    s.ops
        .call(
            &app(),
            ops::VAULT_APPROVAL_ANSWER,
            &json!({"id": id, "decision": "deny"}),
        )
        .unwrap();
    let e = user.join().unwrap().unwrap_err();
    assert_eq!(
        (e.code, e.message.as_str()),
        (ErrorCode::NeedsApproval, "The user didn't allow it.")
    );
    assert_eq!(
        crate::event_assert::results(&s.sink, "vault.secret_used"),
        ["denied"]
    );

    let (user, event) = pending_use(&s);
    s.ops
        .call(
            &app(),
            ops::VAULT_APPROVAL_ANSWER,
            &json!({"id": event["id"], "decision": "always"}),
        )
        .unwrap();
    assert!(user.join().unwrap().is_ok());
    assert_eq!(s.ops.store.entry("gh").unwrap().unwrap().ask, Some(false));
    let changed = s.room.pending(topics::VAULT_CHANGED).unwrap();
    assert_eq!(changed["names"], json!(["gh"]));
}

#[test]
fn the_app_leaving_or_the_switch_going_off_ends_the_wait() {
    let s = setup("leave", true);
    seed(&s, true);
    let (user, _) = pending_use(&s);
    s.room.follows.store(false, Ordering::SeqCst);
    let e = user.join().unwrap().unwrap_err();
    assert_eq!(e.code, ErrorCode::NeedsApproval);
    let (user, _) = pending_use(&s);
    s.switch.set(false);
    let e = user.join().unwrap().unwrap_err();
    assert_eq!(
        (e.code, e.message.as_str()),
        (ErrorCode::Forbidden, VAULT_OFF)
    );
    s.switch.set(true);
    let (user, _) = pending_use(&s);
    s.ops.close();
    assert!(user.join().unwrap().is_err(), "the daemon stopping is a no");
    assert!(s.ops.approvals.is_empty());
}

#[test]
fn a_request_opens_the_form_and_returns_what_the_user_saved() {
    let s = setup("request", true);
    seed(&s, false);
    let req = json!({"name": "gh", "url": "https://api.github-cdn.io", "note": "new\nnote",
                     "expires": "2027-03-04", "reason": "to push"});
    let e = err(s.ops.call(&agent(), ops::VAULT_REQUEST, &req));
    assert!(e.message.contains("isn't open"), "no app, no form");
    s.room.follows.store(true, Ordering::SeqCst);
    let ops = s.ops.clone();
    let asked = req.clone();
    let user = std::thread::spawn(move || ops.call(&agent(), ops::VAULT_REQUEST, &asked));
    let event = loop {
        if let Some(e) = s.room.pending(topics::VAULT_REQUEST_PENDING) {
            break e;
        }
        std::thread::sleep(Duration::from_millis(5));
    };
    assert_eq!(event["existing"], true);
    assert_eq!(event["entry"]["note"], "new\nnote");
    assert_eq!(event["entry"]["secretVar"], "GH_TOKEN");
    assert_eq!(event["changes"]["old"]["url"], "https://api.github.com");
    assert_eq!(
        event["changes"]["hostChange"]["to"],
        "https://api.github-cdn.io"
    );
    assert_eq!(event["request"]["reason"], "to push");
    let id = event["id"].as_str().unwrap().to_string();
    let e = err(s
        .ops
        .call(&agent(), ops::VAULT_REQUEST_CANCEL, &json!({"id": id})));
    assert_eq!(e.code, ErrorCode::Forbidden);
    let saved = s
        .ops
        .call(
            &app(),
            ops::VAULT_SAVE,
            &json!({"entry": event["entry"], "replacing": "gh", "request": id}),
        )
        .unwrap();
    assert_eq!(saved["answered"], true);
    let reply = user.join().unwrap().unwrap();
    assert_eq!(reply["entry"]["note"], "new\nnote");
    assert!(
        !reply.to_string().contains(TOKEN),
        "a request never returns the value"
    );

    let ops = s.ops.clone();
    let user =
        std::thread::spawn(move || ops.call(&agent(), ops::VAULT_REQUEST, &json!({"name": "npm"})));
    let id = loop {
        match s.room.pending(topics::VAULT_REQUEST_PENDING) {
            Some(e) if e["entry"]["name"] == "npm" => break e["id"].clone(),
            _ => std::thread::sleep(Duration::from_millis(5)),
        }
    };
    s.ops
        .call(&app(), ops::VAULT_REQUEST_CANCEL, &json!({"id": id}))
        .unwrap();
    let e = user.join().unwrap().unwrap_err();
    assert_eq!(
        (e.code, e.message.as_str()),
        (ErrorCode::NeedsApproval, "The user cancelled.")
    );
}

#[test]
fn a_request_checks_its_fields() {
    let s = setup("reqbad", true);
    s.room.follows.store(true, Ordering::SeqCst);
    for args in [
        json!({"name": "2fa"}),
        json!({"name": "gh", "kind": "pin"}),
        json!({"name": "gh", "expires": "2026-02-30"}),
        json!({"name": "gh", "note": "x".repeat(9000)}),
        json!({"name": "gh", "url": 5}),
    ] {
        let e = err(s.ops.call(&agent(), ops::VAULT_REQUEST, &args));
        assert_eq!(e.code, ErrorCode::BadRequest, "{args}");
    }
    assert!(s.room.events.lock().unwrap().is_empty());
}

#[test]
fn saves_and_removes_say_why_without_the_value() {
    let s = setup("save", true);
    seed(&s, false);
    let e = err(s.ops.call(
        &app(),
        ops::VAULT_SAVE,
        &json!({"entry": {"name": "x", "kind": "token"},
                                                           "value": {"secret": 12345678}}),
    ));
    assert_eq!(e.code, ErrorCode::BadRequest);
    assert!(!e.message.contains("12345678"));
    let e = err(s.ops.call(
        &app(),
        ops::VAULT_SAVE,
        &json!({"entry": {"name": "x", "kind": TOKEN}}),
    ));
    assert!(!e.message.contains(TOKEN), "{}", e.message);
    let clash = json!({"entry": {"name": "gh2", "kind": "token", "secretVar": "GH_TOKEN"},
                       "value": {"secret": "fake-other-0003"}});
    let e = err(s.ops.call(&app(), ops::VAULT_SAVE, &clash));
    assert_eq!(e.message, "Another secret already uses GH_TOKEN.");
    let changed = s.room.pending(topics::VAULT_CHANGED).unwrap();
    assert_eq!(changed["value_changed"], true);
    let removed = s
        .ops
        .call(&app(), ops::VAULT_REMOVE, &json!({"name": "reg"}))
        .unwrap();
    assert_eq!(removed["removed"], true);
    let removed = s
        .ops
        .call(&app(), ops::VAULT_REMOVE, &json!({"name": "reg"}))
        .unwrap();
    assert_eq!(removed["removed"], false);
    assert!(!s.outside().contains(TOKEN) && !s.outside().contains(PASSWORD));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let db = notchling_engine::store::db_path(&s.dir);
        let mode = std::fs::metadata(db).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
    }
}
