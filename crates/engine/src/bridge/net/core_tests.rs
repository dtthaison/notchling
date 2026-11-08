use super::*;
use crate::bridge::net::connection::RecordingIo;
use crate::bridge::net::jwt::tests::{TestClock, START};
use crate::sessions::testutil::TempDir;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

/// What the router would do: records the messages it's handed.
#[derive(Default)]
pub(crate) struct FakeHandler {
    pub opened: AtomicUsize,
    pub closed: AtomicUsize,
    pub messages: Mutex<Vec<String>>,
}

impl BridgeHandler for FakeHandler {
    fn opened(&self, _connection: &Arc<Connection>) {
        self.opened.fetch_add(1, Ordering::SeqCst);
    }
    fn message(&self, connection: &Arc<Connection>, text: &str) {
        self.messages.lock().unwrap().push(text.to_string());
        connection.send_text(&format!("echo {text}"));
    }
    fn closed(&self, _connection: &Arc<Connection>) {
        self.closed.fetch_add(1, Ordering::SeqCst);
    }
}

pub(crate) struct FakeApprover {
    pub allow: AtomicBool,
    pub calls: AtomicUsize,
}

impl Default for FakeApprover {
    fn default() -> Self {
        FakeApprover {
            allow: AtomicBool::new(true),
            calls: AtomicUsize::new(0),
        }
    }
}

impl PairApprover for FakeApprover {
    fn approve(&self, _name: &str, _platform: &str) -> bool {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.allow.load(Ordering::SeqCst)
    }
}

pub(crate) fn no_fetch() -> Fetch {
    Arc::new(|_: &str| Err("no network in tests".to_string()))
}

struct Rig {
    _dir: TempDir,
    core: Core,
    handler: Arc<FakeHandler>,
    approver: Arc<FakeApprover>,
    clock: TestClock,
}

impl Rig {
    fn new() -> Rig {
        let dir = TempDir::new("bridge-core");
        let handler = Arc::new(FakeHandler::default());
        let approver = Arc::new(FakeApprover::default());
        let clock = TestClock::new(START);
        let core = Core::new(
            &dir.path("bridge"),
            handler.clone(),
            approver.clone(),
            no_fetch(),
            clock.clock(),
        );
        Rig {
            _dir: dir,
            core,
            handler,
            approver,
            clock,
        }
    }

    fn connect(&self, from: &str) -> (Arc<Connection>, Arc<RecordingIo>) {
        let io = Arc::new(RecordingIo::default());
        let connection = Arc::new(Connection::new(from, io.clone(), false));
        self.core.add(connection.clone());
        (connection, io)
    }

    fn login(&self) -> (Arc<Connection>, Arc<RecordingIo>, String) {
        let (_, token) = self
            .core
            .devices
            .register("test-phone", "ios", START)
            .unwrap();
        let (connection, io) = self.connect("192.168.1.20");
        self.core.handle(
            &connection,
            &format!(r#"{{"id":1,"op":"hello","token":"{token}"}}"#),
        );
        assert!(connection.is_authenticated());
        (connection, io, token)
    }
}

fn code(io: &RecordingIo, index: usize) -> String {
    io.json(index)["error"]["code"]
        .as_str()
        .unwrap_or("")
        .to_string()
}

#[test]
fn nothing_reaches_the_router_before_hello() {
    let rig = Rig::new();
    let (connection, io) = rig.connect("192.168.1.20");
    rig.core
        .handle(&connection, r#"{"id":7,"op":"sessions.list"}"#);
    assert_eq!(code(&io, 0), "unauthorized");
    assert_eq!(io.json(0)["id"], 7);
    assert_eq!(
        io.texts()[0],
        r#"{"error":{"code":"unauthorized","message":"Cần đăng nhập (hello) trước."},"id":7,"ok":false}"#
    );
    assert!(io.is_closed());
    assert!(
        rig.handler.messages.lock().unwrap().is_empty(),
        "the router was never asked"
    );
}

#[test]
fn garbage_before_auth_closes_the_connection() {
    let rig = Rig::new();
    let (connection, io) = rig.connect("192.168.1.20");
    rig.core.handle(&connection, "not json");
    assert_eq!(
        io.texts()[0],
        r#"{"error":{"code":"invalid","message":"Tin nhắn không phải JSON hợp lệ."},"ok":false}"#
    );
    assert!(io.is_closed());
}

#[test]
fn hello_with_a_wrong_token_is_refused_and_the_right_one_works() {
    let rig = Rig::new();
    let (device, token) = rig.core.devices.register("phone", "ios", START).unwrap();
    let (bad, bad_io) = rig.connect("192.168.1.20");
    rig.core
        .handle(&bad, r#"{"id":"a","op":"hello","token":"nope"}"#);
    assert_eq!(code(&bad_io, 0), "unauthorized");
    assert_eq!(bad_io.json(0)["id"], "a");
    assert!(bad_io.is_closed());

    let (good, good_io) = rig.connect("192.168.1.21");
    rig.core.handle(
        &good,
        &format!(r#"{{"id":1,"op":"hello","token":"{token}"}}"#),
    );
    let expected = format!(
        r#"{{"data":{{"device":{{"id":"{}","name":"phone"}},"version":1}},"id":1,"ok":true}}"#,
        device.id
    );
    assert_eq!(
        good_io.texts()[0],
        expected,
        "the HelloResponse fixture's shape"
    );
    assert!(good.is_authenticated());
    assert_eq!(rig.handler.opened.load(Ordering::SeqCst), 1);
    rig.core.handle(&good, r#"{"id":2,"op":"sessions.list"}"#);
    assert_eq!(good_io.texts()[1], r#"echo {"id":2,"op":"sessions.list"}"#);
    assert!(rig.core.devices.devices()[0].last_seen.is_some(), "seen");
}

#[test]
fn five_bad_hellos_lock_the_address() {
    let rig = Rig::new();
    for _ in 0..5 {
        let (connection, _) = rig.connect("192.168.1.50");
        rig.core
            .handle(&connection, r#"{"id":1,"op":"hello","token":"bad"}"#);
    }
    let (locked, io) = rig.connect("192.168.1.50");
    rig.core
        .handle(&locked, r#"{"id":1,"op":"hello","token":"bad"}"#);
    assert!(
        io.texts().is_empty(),
        "a locked address gets silence, then the door"
    );
    assert!(io.is_closed());
    let (other, other_io) = rig.connect("192.168.1.51");
    rig.core
        .handle(&other, r#"{"id":1,"op":"hello","token":"bad"}"#);
    assert_eq!(
        code(&other_io, 0),
        "unauthorized",
        "another address is unaffected"
    );
    rig.clock.advance(5 * 60 * 1000 + 1);
}

#[test]
fn pair_with_the_code_needs_approval_and_returns_the_token_once() {
    let rig = Rig::new();
    let code_text = rig.core.codes.issue(START).unwrap().code;
    let (connection, io) = rig.connect("192.168.1.20");
    let pair = |code: &str, name: &str| {
        format!(
            r#"{{"id":1,"op":"pair","code":"{code}","device":{{"name":"{name}","platform":"ios"}}}}"#
        )
    };
    rig.core.handle(&connection, &pair(&code_text, "p4-test"));
    let reply = io.json(0);
    assert_eq!(reply["ok"], true);
    let token = reply["data"]["token"].as_str().unwrap().to_string();
    assert_eq!(reply["data"]["version"], 1);
    assert_eq!(rig.approver.calls.load(Ordering::SeqCst), 1);
    assert_eq!(rig.core.devices.devices()[0].name, "p4-test");
    assert!(rig.core.devices.authenticate(&token).is_some());
    assert!(connection.is_authenticated());

    let (second, second_io) = rig.connect("192.168.1.20");
    rig.core.handle(&second, &pair(&code_text, "again"));
    assert_eq!(code(&second_io, 0), "unauthorized", "the code was one-time");
    assert_eq!(rig.core.devices.devices().len(), 1);
    assert_eq!(rig.core.audit.recent(50)[0].op, "pair");
}

#[test]
fn a_pair_denied_at_the_computer_registers_nothing() {
    let rig = Rig::new();
    rig.approver.allow.store(false, Ordering::SeqCst);
    let code_text = rig.core.codes.issue(START).unwrap().code;
    let (connection, io) = rig.connect("192.168.1.20");
    rig.core.handle(&connection, &format!(r#"{{"id":1,"op":"pair","code":"{code_text}","device":{{"name":"x","platform":"ios"}}}}"#));
    assert_eq!(code(&io, 0), "unauthorized");
    assert!(io.is_closed());
    assert!(rig.core.devices.devices().is_empty());
}

#[test]
fn a_wrong_or_expired_code_fails_without_asking() {
    let rig = Rig::new();
    let code_text = rig.core.codes.issue(START).unwrap().code;
    let (wrong, wrong_io) = rig.connect("192.168.1.20");
    rig.core.handle(
        &wrong,
        r#"{"id":1,"op":"pair","code":"ZZZZZZZZ","device":{"name":"x","platform":"ios"}}"#,
    );
    assert_eq!(code(&wrong_io, 0), "unauthorized");
    rig.clock.advance(301_000);
    let (late, late_io) = rig.connect("192.168.1.60");
    rig.core.handle(&late, &format!(r#"{{"id":1,"op":"pair","code":"{code_text}","device":{{"name":"x","platform":"ios"}}}}"#));
    assert_eq!(code(&late_io, 0), "unauthorized");
    let (nameless, nameless_io) = rig.connect("192.168.1.61");
    let fresh = rig.core.codes.issue(rig.clock.now()).unwrap().code;
    rig.core.handle(
        &nameless,
        &format!(r#"{{"id":1,"op":"pair","code":"{fresh}","device":{{"name":"  "}}}}"#),
    );
    assert_eq!(code(&nameless_io, 0), "invalid");
    assert_eq!(rig.approver.calls.load(Ordering::SeqCst), 0);
}

#[test]
fn revoke_closes_live_connections_and_refuses_the_token() {
    let rig = Rig::new();
    let (connection, io, token) = rig.login();
    let device = connection.device().unwrap();
    rig.core.revoke(&device.id).unwrap();
    assert!(io.is_closed());
    assert!(rig.core.devices.authenticate(&token).is_none());
    let (again, again_io) = rig.connect("192.168.1.70");
    rig.core.handle(
        &again,
        &format!(r#"{{"id":1,"op":"hello","token":"{token}"}}"#),
    );
    assert_eq!(code(&again_io, 0), "unauthorized");
    assert_eq!(rig.core.audit.recent(50)[0].op, "revoke");
    rig.core.remove(&connection);
    assert_eq!(rig.handler.closed.load(Ordering::SeqCst), 1);
    rig.core.remove(&again);
    assert_eq!(
        rig.handler.closed.load(Ordering::SeqCst),
        1,
        "it never said hello"
    );
}

#[test]
fn a_tunnel_pairing_is_audited_by_the_clients_own_address() {
    let rig = Rig::new();
    let code_text = rig.core.codes.issue(START).unwrap().code;
    let io = Arc::new(RecordingIo::default());
    let connection = Arc::new(Connection::new("203.0.113.77", io.clone(), true));
    rig.core.add(connection.clone());
    assert_eq!(rig.core.last_tunnel_connection(), Some(START));
    rig.core.handle(&connection, &format!(r#"{{"id":1,"op":"pair","code":"{code_text}","device":{{"name":"away","platform":"ios"}}}}"#));
    assert_eq!(
        rig.core.audit.recent(1)[0].address.as_deref(),
        Some("203.0.113.77")
    );
}
