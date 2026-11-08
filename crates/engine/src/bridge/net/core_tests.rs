//! The router (E5a) on the transport's own stores: `hello`, `pair`, the lockout and revoke, with
//! the device store, codes, limiter and audit log writing the Swift app's files in a temp folder.

use super::*;
use crate::bridge::model::{Control, Session, SessionState};
use crate::bridge::net::jwt::tests::{TestClock, START};
use crate::bridge::test_support::{FakeBackend, RecordingIo};
use crate::sessions::testutil::TempDir;

pub(crate) fn no_fetch() -> Fetch {
    Arc::new(|_: &str| Err("no network in tests".to_string()))
}

/// The router's parts over a fake backend, for a core in `bridge_dir`.
pub(crate) fn parts(backend: Arc<FakeBackend>, home: &Path, clock: Clock) -> CoreParts {
    CoreParts {
        backend,
        home: home.to_path_buf(),
        images_dir: home.join("phone-images"),
        fetch: no_fetch(),
        now: clock,
    }
}

struct Rig {
    _dir: TempDir,
    core: Core,
    backend: Arc<FakeBackend>,
    clock: TestClock,
}

impl Rig {
    fn new() -> Rig {
        let dir = TempDir::new("bridge-core");
        let backend = Arc::new(FakeBackend::default());
        backend.with(|s| {
            s.sessions = vec![Session::new(
                "s1",
                "fix tests",
                "/Users/example/src/app",
                Control::Job("ab12".into()),
                SessionState::Running,
            )]
        });
        let clock = TestClock::new(START);
        let core = Core::new(
            &dir.path("bridge"),
            parts(backend.clone(), &dir.path("home"), clock.clock()),
        );
        Rig {
            _dir: dir,
            core,
            backend,
            clock,
        }
    }

    fn connect_from(&self, from: &str, tunnel: bool) -> (Arc<Connection>, Arc<RecordingIo>) {
        let io = Arc::new(RecordingIo::default());
        let connection = self.core.connect(from, tunnel, io.clone());
        (connection, io)
    }

    fn connect(&self, from: &str) -> (Arc<Connection>, Arc<RecordingIo>) {
        self.connect_from(from, false)
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

fn pair(code: &str, name: &str) -> String {
    format!(
        r#"{{"id":1,"op":"pair","code":"{code}","device":{{"name":"{name}","platform":"ios"}}}}"#
    )
}

#[test]
fn nothing_is_served_before_hello() {
    let rig = Rig::new();
    let (connection, io) = rig.connect("192.168.1.20");
    rig.core
        .handle(&connection, r#"{"id":7,"op":"sessions.list"}"#);
    assert_eq!(
        io.texts()[0],
        r#"{"error":{"code":"unauthorized","message":"Cần đăng nhập (hello) trước."},"id":7,"ok":false}"#
    );
    assert!(io.is_closed());
    assert_eq!(rig.backend.with(|s| s.session_calls), 0, "nothing was read");
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
    rig.core.handle(&good, r#"{"id":2,"op":"sessions.list"}"#);
    assert_eq!(good_io.json(1)["data"]["sessions"][0]["id"], "s1");
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
    let (later, later_io) = rig.connect("192.168.1.50");
    rig.core
        .handle(&later, r#"{"id":1,"op":"hello","token":"bad"}"#);
    assert_eq!(
        code(&later_io, 0),
        "unauthorized",
        "answered again after the lockout"
    );
}

#[test]
fn pair_with_the_code_needs_approval_and_returns_the_token_once() {
    let rig = Rig::new();
    let code_text = rig.core.codes.issue(START).unwrap().code;
    let (connection, io) = rig.connect("192.168.1.20");
    rig.core.handle(&connection, &pair(&code_text, "p4-test"));
    let reply = io.json(0);
    assert_eq!(reply["ok"], true);
    let token = reply["data"]["token"].as_str().unwrap().to_string();
    assert_eq!(reply["data"]["version"], 1);
    assert_eq!(rig.backend.with(|s| s.approval_calls), 1);
    assert_eq!(rig.core.devices.devices()[0].name, "p4-test");
    assert!(rig.core.devices.authenticate(&token).is_some());
    assert!(connection.is_authenticated());

    let (second, second_io) = rig.connect("192.168.1.20");
    rig.core.handle(&second, &pair(&code_text, "again"));
    assert_eq!(code(&second_io, 0), "unauthorized", "the code was one-time");
    assert_eq!(rig.core.devices.devices().len(), 1);
    let audit = rig.core.audit.recent(50);
    assert_eq!(audit[0].op, "pair");
    assert_eq!(audit[0].time, START, "stamped by the bridge's clock");
    assert_eq!(
        audit[0].address, None,
        "LAN clients aren't logged by address"
    );
}

#[test]
fn a_pair_denied_at_the_computer_registers_nothing() {
    let rig = Rig::new();
    rig.backend.with(|s| s.refuse_approval = true);
    let code_text = rig.core.codes.issue(START).unwrap().code;
    let (connection, io) = rig.connect("192.168.1.20");
    rig.core.handle(&connection, &pair(&code_text, "x"));
    assert_eq!(code(&io, 0), "unauthorized");
    assert!(io.is_closed());
    assert!(rig.core.devices.devices().is_empty());
}

#[test]
fn a_wrong_or_expired_code_fails_without_asking() {
    let rig = Rig::new();
    let code_text = rig.core.codes.issue(START).unwrap().code;
    let (wrong, wrong_io) = rig.connect("192.168.1.20");
    rig.core.handle(&wrong, &pair("ZZZZZZZZ", "x"));
    assert_eq!(code(&wrong_io, 0), "unauthorized");
    rig.clock.advance(301_000);
    let (late, late_io) = rig.connect("192.168.1.60");
    rig.core.handle(&late, &pair(&code_text, "x"));
    assert_eq!(code(&late_io, 0), "unauthorized");
    assert_eq!(rig.backend.with(|s| s.approval_calls), 0);
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
    assert_eq!(rig.core.connections(), 2);
    rig.core.closed(&connection);
    rig.core.closed(&again);
    assert_eq!(rig.core.connections(), 0, "the hub forgets them");
}

#[test]
fn a_tunnel_pairing_is_audited_by_the_clients_own_address() {
    let rig = Rig::new();
    let code_text = rig.core.codes.issue(START).unwrap().code;
    let (connection, _io) = rig.connect_from("203.0.113.77", true);
    assert_eq!(rig.core.last_tunnel_connection(), Some(START));
    rig.core.handle(&connection, &pair(&code_text, "away"));
    assert_eq!(
        rig.core.audit.recent(1)[0].address.as_deref(),
        Some("203.0.113.77")
    );
}

#[test]
fn once_halted_a_hello_pair_or_revoke_in_flight_writes_nothing() {
    let rig = Rig::new();
    let (device, token) = rig.core.devices.register("phone", "ios", START).unwrap();
    let devices = rig.core.devices.file().to_path_buf();
    let before = std::fs::read(&devices).unwrap();
    let code_text = rig.core.codes.issue(START).unwrap().code;
    // Bridge-off has begun: what was already on its way must not write.
    rig.core.halt();
    let (hello, _) = rig.connect("192.168.1.20");
    rig.core.handle(
        &hello,
        &format!(r#"{{"id":1,"op":"hello","token":"{token}"}}"#),
    );
    let (pairing, pair_io) = rig.connect("192.168.1.21");
    rig.core.handle(&pairing, &pair(&code_text, "late"));
    assert_eq!(code(&pair_io, 0), "internal", "{:?}", pair_io.texts());
    assert!(rig.core.revoke(&device.id).is_err());
    assert_eq!(
        std::fs::read(&devices).unwrap(),
        before,
        "devices.json untouched"
    );
    assert!(!rig.core.audit.file().exists(), "no audit line");
}

#[test]
fn the_tick_pushes_sessions_to_a_subscriber() {
    let rig = Rig::new();
    let (connection, io, _) = rig.login();
    rig.core
        .handle(&connection, r#"{"id":2,"op":"subscribe","sessions":true}"#);
    rig.backend
        .with(|s| s.sessions[0].state = SessionState::NeedsInput);
    let before = io.texts().len();
    rig.core.tick();
    let pushed: Vec<String> = io.texts()[before..].to_vec();
    assert!(
        pushed.iter().any(|t| t.contains(r#""event":"sessions""#)),
        "{pushed:?}"
    );
}
