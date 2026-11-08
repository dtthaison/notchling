//! The server over a real socket in a temp folder; the peer is this test process.
#![cfg(unix)]

use crate::client::{Client, ClientError, ConnectOptions};
use crate::framing::{LineReader, MAX_LINE};
use crate::server::{Caller, Handler, Limits, OpSpec, Server, ServerConfig, ServerHandle};
use crate::testutil::TempDir;
use crate::tier::Allowlist;
use crate::wire::{ApiError, ClientKind, ErrorCode, Tier};
use crate::AuditLog;
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

struct TestHandler;

impl Handler for TestHandler {
    fn spec(&self, op: &str) -> Option<OpSpec> {
        match op {
            "echo" => Some(OpSpec::AGENT),
            "settings.set" => Some(OpSpec::APP),
            // Stands for an op that would need the user's approval (approval.* comes later).
            "test.needsApproval" => Some(OpSpec::APPROVAL),
            // A handler bug: the connection's thread unwinds.
            "test.panic" => Some(OpSpec::AGENT),
            _ => None,
        }
    }

    fn call(&self, caller: &Caller, op: &str, args: &Value) -> Result<Value, ApiError> {
        match op {
            "echo" => Ok(json!({ "args": args, "tier": caller.tier })),
            "test.panic" => panic!("a handler bug"),
            _ => Ok(json!("done")),
        }
    }

    fn topics(&self) -> &[&str] {
        &["sessions"]
    }
}

fn fast() -> Limits {
    Limits {
        max_clients: 8,
        app_reserved: 0,
        hello_within: Duration::from_millis(600),
        idle_within: Duration::from_millis(600),
        line_within: Duration::from_millis(600),
        write_within: Duration::from_millis(500),
        queue: 4,
        tick: Duration::from_millis(50),
    }
}

struct Running {
    dir: TempDir,
    socket: PathBuf,
    handle: ServerHandle,
    thread: Option<JoinHandle<std::io::Result<()>>>,
}

impl Running {
    fn audit_path(&self) -> PathBuf {
        self.dir.path().join("audit.jsonl")
    }
}

impl Drop for Running {
    fn drop(&mut self) {
        self.handle.shutdown();
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

fn me() -> PathBuf {
    std::env::current_exe().unwrap()
}

fn start_with(allow: Allowlist, limits: Limits) -> Running {
    let dir = TempDir::new("srv");
    let socket = dir.path().join("run/notchlingd.sock");
    let server = Server::bind(
        ServerConfig {
            socket: socket.clone(),
            allowlist: allow,
            audit: Some(AuditLog::new(dir.path().join("audit.jsonl"))),
            version: "9.9.9".into(),
            limits,
        },
        Arc::new(TestHandler),
    )
    .unwrap();
    let (handle, thread) = server.spawn().unwrap();
    Running {
        dir,
        socket,
        handle,
        thread: Some(thread),
    }
}

fn start() -> Running {
    start_with(Allowlist::empty(), fast())
}

fn client(socket: &Path) -> Client {
    connect_as(socket, ClientKind::Cli)
}

/// The server here is this test binary, not a `notchling`, so the client's server check is off.
fn relaxed() -> ConnectOptions {
    ConnectOptions {
        check_server: false,
        ..ConnectOptions::default()
    }
}

fn connect_as(socket: &Path, kind: ClientKind) -> Client {
    Client::connect_with(socket, kind, "0.1.0", &relaxed()).unwrap()
}

#[test]
fn a_client_refuses_a_server_that_is_not_a_notchling_daemon() {
    // Strict by default: the test binary serving here is no `notchling`.
    let run = start();
    match Client::connect(&run.socket, ClientKind::App, "0.1.0") {
        Err(ClientError::UntrustedServer(why)) => assert!(why.contains("not a notchling"), "{why}"),
        Err(e) => panic!("{e}"),
        Ok(c) => panic!("said hello to {:?}", c.hello()),
    }
    // It said nothing: the server never heard a hello.
    assert_eq!(run.handle.status().clients, Default::default());
    // Relaxed, as tests do, the same server is fine.
    assert!(Client::connect_with(&run.socket, ClientKind::App, "0.1.0", &relaxed()).is_ok());
}

/// A raw connection and its lines.
fn raw(socket: &Path) -> (UnixStream, LineReader<UnixStream>) {
    let s = UnixStream::connect(socket).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    let r = LineReader::new(s.try_clone().unwrap());
    (s, r)
}

fn send(s: &mut UnixStream, line: &str) {
    s.write_all(line.as_bytes()).unwrap();
    s.write_all(b"\n").unwrap();
}

fn reply(r: &mut LineReader<UnixStream>) -> Value {
    serde_json::from_slice(&r.next_line().unwrap().unwrap()).unwrap()
}

/// True when the server closed the connection (EOF or reset).
fn closed(s: &mut UnixStream) -> bool {
    // macOS refuses setsockopt (EINVAL) on a socket whose peer has already shut it down.
    if s.set_read_timeout(Some(Duration::from_secs(5))).is_err() {
        return true;
    }
    let mut buf = [0u8; 4096];
    loop {
        match s.read(&mut buf) {
            Ok(0) => return true,
            Ok(_) => continue,
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => return false,
            Err(_) => return true,
        }
    }
}

const HELLO: &str = r#"{"id":1,"op":"hello","args":{"client":"cli","version":"0.1.0","api":1}}"#;

#[test]
fn hello_names_the_api_the_daemon_and_the_tier() {
    let run = start();
    let c = client(&run.socket);
    assert_eq!(c.hello().api, 1);
    assert_eq!(c.hello().daemon, "9.9.9");
    assert_eq!(c.tier(), Tier::Agent);
}

#[test]
fn hello_is_required_first() {
    let run = start();
    let (mut s, mut r) = raw(&run.socket);
    send(&mut s, r#"{"id":5,"op":"daemon.status"}"#);
    let v = reply(&mut r);
    assert_eq!(v["id"], 5);
    assert_eq!(v["err"]["code"], "bad_request");
    assert!(closed(&mut s));
}

#[test]
fn a_wrong_api_version_is_refused() {
    let run = start();
    let (mut s, mut r) = raw(&run.socket);
    send(
        &mut s,
        r#"{"id":1,"op":"hello","args":{"client":"cli","version":"0.1.0","api":2}}"#,
    );
    assert_eq!(reply(&mut r)["err"]["code"], "bad_request");
    assert!(closed(&mut s));
}

#[test]
fn garbage_after_hello_is_a_bad_request_and_the_connection_stays() {
    let run = start();
    let (mut s, mut r) = raw(&run.socket);
    send(&mut s, HELLO);
    assert_eq!(reply(&mut r)["ok"]["tier"], "agent");
    s.write_all(b"\xff\xfenot json\n").unwrap();
    let v = reply(&mut r);
    assert_eq!(
        (v["id"].as_u64(), v["err"]["code"].as_str()),
        (Some(0), Some("bad_request"))
    );
    send(&mut s, r#"{"id":2,"op":"echo","args":{"x":1}}"#);
    assert_eq!(reply(&mut r)["ok"]["args"]["x"], 1);
}

#[test]
fn an_unknown_op_says_so() {
    let run = start();
    let mut c = client(&run.socket);
    match c.call("no.such", json!({})) {
        Err(ClientError::Api(e)) => assert_eq!(e.code, ErrorCode::UnknownOp),
        other => panic!("{other:?}"),
    }
}

#[test]
fn the_tier_comes_from_the_allowlist_not_the_hello() {
    // The peer is this test binary: listed, it is the app tier; unlisted, the agent tier, even
    // when its hello says "app".
    let listed = start_with(Allowlist::new([me()]), fast());
    assert_eq!(client(&listed.socket).tier(), Tier::App);
    let unlisted = start();
    let c = connect_as(&unlisted.socket, ClientKind::App);
    assert_eq!(c.tier(), Tier::Agent);
}

#[cfg(unix)]
#[test]
fn a_symlink_to_the_peer_in_the_allowlist_still_names_the_peer() {
    let dir = TempDir::new("srv-link");
    let link = dir.path().join("app-link");
    std::os::unix::fs::symlink(me(), &link).unwrap();
    let run = start_with(Allowlist::new([&link]), fast());
    assert_eq!(client(&run.socket).tier(), Tier::App);
}

#[test]
fn an_app_op_from_the_agent_tier_is_forbidden_and_audited() {
    let run = start();
    let mut c = client(&run.socket);
    match c.call("settings.set", json!({})) {
        Err(ClientError::Api(e)) => assert_eq!(e.code, ErrorCode::Forbidden),
        other => panic!("{other:?}"),
    }
    let text = std::fs::read_to_string(run.audit_path()).unwrap();
    let line: Value = serde_json::from_str(text.trim()).unwrap();
    assert_eq!(line["op"], "settings.set");
    assert_eq!(line["tier"], "agent");
    assert_eq!(line["result"], "forbidden");
    let peer = PathBuf::from(line["peer"].as_str().unwrap());
    assert_eq!(
        std::fs::canonicalize(peer).unwrap(),
        std::fs::canonicalize(me()).unwrap()
    );
}

#[test]
fn approval_is_needed_while_no_app_is_attached() {
    let run = start();
    let mut c = client(&run.socket);
    match c.call("test.needsApproval", json!({})) {
        Err(ClientError::Api(e)) => assert_eq!(e.code, ErrorCode::NeedsApproval),
        other => panic!("{other:?}"),
    }
    let text = std::fs::read_to_string(run.audit_path()).unwrap();
    assert!(text.contains("needs_approval"));
}

#[test]
fn with_an_app_attached_the_approval_op_reaches_the_handler() {
    let run = start_with(Allowlist::new([me()]), fast());
    let _app = connect_as(&run.socket, ClientKind::App);
    let mut c = client(&run.socket);
    assert_eq!(
        c.call("test.needsApproval", json!({})).unwrap(),
        json!("done")
    );
    assert_eq!(c.call("settings.set", json!({})).unwrap(), json!("done"));
}

#[test]
fn an_app_connection_whose_handler_panics_is_forgotten() {
    // Left on the hub, it would keep `app_attached` true and approval would fail open.
    let run = start_with(Allowlist::new([me()]), fast());
    let mut app = connect_as(&run.socket, ClientKind::App);
    assert_eq!(app.tier(), Tier::App);
    assert!(app.call("test.panic", json!({})).is_err());
    drop(app);
    let deadline = Instant::now() + Duration::from_secs(5);
    while run.handle.connections() > 0 && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(run.handle.connections(), 0);
    assert_eq!(run.handle.status().clients.app, 0);
}

#[test]
fn status_counts_clients_by_kind() {
    let run = start();
    let _hook = connect_as(&run.socket, ClientKind::Hook);
    let mut c = client(&run.socket);
    let status = c.status().unwrap();
    assert_eq!(status.version, "9.9.9");
    assert_eq!(status.pid, std::process::id());
    assert_eq!(
        (status.clients.cli, status.clients.hook, status.clients.app),
        (1, 1, 0)
    );
}

#[test]
fn events_reach_subscribers_only() {
    let run = start();
    let mut sub = client(&run.socket);
    sub.subscribe(&["sessions"]).unwrap();
    let mut other = client(&run.socket);
    assert_eq!(
        run.handle
            .publish("sessions", "sessions.changed", &json!([{"id": "a"}])),
        1
    );
    let event = sub.next_event(Duration::from_secs(5)).unwrap().unwrap();
    assert_eq!(event.event, "sessions.changed");
    assert_eq!(event.data[0]["id"], "a");
    assert!(other
        .next_event(Duration::from_millis(200))
        .unwrap()
        .is_none());
    match other.subscribe(&["nope"]) {
        Err(ClientError::Api(e)) => assert_eq!(e.code, ErrorCode::BadRequest),
        x => panic!("{x:?}"),
    }
}

#[test]
fn an_oversize_line_closes_that_client_and_others_go_on() {
    let run = start();
    let (mut s, mut r) = raw(&run.socket);
    send(&mut s, HELLO);
    reply(&mut r);
    let big = vec![b'x'; MAX_LINE + 10];
    // The server may close mid-write; that's the point.
    let _ = s.write_all(&big);
    assert!(closed(&mut s));
    let mut c = client(&run.socket);
    assert!(c.status().is_ok());
}

#[test]
fn a_line_that_never_ends_is_cut_off_after_the_deadline() {
    let run = start();
    let (mut s, mut r) = raw(&run.socket);
    send(&mut s, HELLO);
    reply(&mut r);
    s.write_all(br#"{"id":2,"op":"ec"#).unwrap();
    // Meanwhile another client is served at once.
    let started = Instant::now();
    let mut c = client(&run.socket);
    c.status().unwrap();
    assert!(started.elapsed() < Duration::from_millis(500));
    assert!(closed(&mut s));
}

#[test]
fn a_client_that_never_says_hello_is_closed() {
    let run = start();
    let (mut s, _r) = raw(&run.socket);
    let started = Instant::now();
    assert!(closed(&mut s));
    assert!(started.elapsed() >= Duration::from_millis(500));
}

#[test]
fn a_subscriber_that_never_reads_is_dropped_without_stalling_the_publisher() {
    let run = start();
    let mut slow = client(&run.socket);
    slow.subscribe(&["sessions"]).unwrap();
    let mut good = client(&run.socket);
    good.subscribe(&["sessions"]).unwrap();
    let big = json!("y".repeat(200_000));
    let started = Instant::now();
    for _ in 0..40 {
        run.handle.publish("sessions", "sessions.changed", &big);
        // The good one reads; the slow one doesn't.
        let _ = good.next_event(Duration::from_millis(50));
    }
    assert!(started.elapsed() < Duration::from_secs(10));
    // The slow one was closed: two connections became one.
    let deadline = Instant::now() + Duration::from_secs(5);
    while run.handle.connections() > 1 && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(run.handle.connections(), 1);
    assert!(good.status().is_ok());
}

#[test]
fn clients_past_the_limit_are_closed_on_accept() {
    let limits = Limits {
        max_clients: 2,
        ..fast()
    };
    let run = start_with(Allowlist::empty(), limits);
    let _a = client(&run.socket);
    let _b = client(&run.socket);
    let (mut s, _r) = raw(&run.socket);
    assert!(closed(&mut s));
}

/// Says hello on a raw connection; true when the server answered it.
fn hello_answered(s: &mut UnixStream) -> bool {
    if s.write_all(format!("{HELLO}\n").as_bytes()).is_err() {
        return false;
    }
    let mut r = LineReader::new(s.try_clone().unwrap());
    matches!(r.next_line(), Ok(Some(line)) if serde_json::from_slice::<Value>(&line).is_ok_and(|v| v["ok"].is_object()))
}

#[test]
fn the_cap_is_counted_at_accept_so_a_burst_cannot_pass_it() {
    let limits = Limits {
        max_clients: 2,
        ..fast()
    };
    let run = start_with(Allowlist::empty(), limits);
    // All at once, before any connection's thread has run.
    let mut burst: Vec<UnixStream> = (0..12)
        .map(|_| UnixStream::connect(&run.socket).unwrap())
        .collect();
    for s in &burst {
        // macOS refuses this (EINVAL) on a socket the server already shut: one that was turned away.
        let _ = s.set_read_timeout(Some(Duration::from_secs(5)));
    }
    let served = burst
        .iter_mut()
        .map(hello_answered)
        .filter(|served| *served)
        .count();
    assert_eq!(served, 2);
}

#[test]
fn the_last_slots_are_kept_for_the_app() {
    let limits = Limits {
        max_clients: 4,
        app_reserved: 2,
        ..fast()
    };
    // Agents get two of the four.
    let agents = start_with(Allowlist::empty(), limits);
    let _a = client(&agents.socket);
    let _b = client(&agents.socket);
    let (mut third, _r) = raw(&agents.socket);
    assert!(!hello_answered(&mut third));
    // The app (this binary, listed) gets all four.
    let apps = start_with(Allowlist::new([me()]), limits);
    let held: Vec<Client> = (0..4)
        .map(|_| connect_as(&apps.socket, ClientKind::App))
        .collect();
    assert!(held.iter().all(|c| c.tier() == Tier::App));
    let (mut fifth, _r) = raw(&apps.socket);
    assert!(!hello_answered(&mut fifth));
}

#[test]
fn an_idle_client_that_follows_nothing_is_closed_and_a_follower_stays() {
    let run = start();
    let (mut idle, mut r) = raw(&run.socket);
    send(&mut idle, HELLO);
    assert!(reply(&mut r)["ok"].is_object());
    let mut follower = client(&run.socket);
    follower.subscribe(&["sessions"]).unwrap();
    let started = Instant::now();
    assert!(closed(&mut idle));
    assert!(started.elapsed() >= Duration::from_millis(500));
    std::thread::sleep(Duration::from_millis(400));
    assert!(follower.status().is_ok(), "a follower waits for events");
}

#[test]
fn the_socket_is_0600_and_its_new_folder_0700() {
    use std::os::unix::fs::PermissionsExt;
    let run = start();
    let mode = std::fs::symlink_metadata(&run.socket)
        .unwrap()
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, 0o600);
    let folder = std::fs::metadata(run.socket.parent().unwrap())
        .unwrap()
        .permissions()
        .mode();
    assert_eq!(folder & 0o777, 0o700);
}

fn config(socket: &Path) -> ServerConfig {
    ServerConfig {
        socket: socket.to_path_buf(),
        allowlist: Allowlist::empty(),
        audit: None,
        version: "1".into(),
        limits: fast(),
    }
}

#[test]
fn a_second_server_on_the_same_socket_is_refused() {
    let run = start();
    let err = Server::bind(config(&run.socket), Arc::new(TestHandler))
        .err()
        .unwrap();
    assert_eq!(err.kind(), std::io::ErrorKind::AddrInUse);
    // The first one still serves.
    assert!(client(&run.socket).status().is_ok());
}

#[test]
fn a_stale_socket_from_a_crash_does_not_stop_a_start() {
    let dir = TempDir::new("srv-stale");
    let socket = dir.path().join("notchlingd.sock");
    // A listener dropped without cleanup leaves its socket file, as a crash would.
    drop(std::os::unix::net::UnixListener::bind(&socket).unwrap());
    assert!(socket.exists());
    assert!(UnixStream::connect(&socket).is_err());
    let server = Server::bind(config(&socket), Arc::new(TestHandler)).unwrap();
    let (handle, thread) = server.spawn().unwrap();
    assert!(client(&socket).status().is_ok());
    handle.shutdown();
    thread.join().unwrap().unwrap();
    assert!(!socket.exists(), "shutdown removes the socket");
}

#[test]
fn a_file_that_is_not_a_socket_is_never_removed() {
    let dir = TempDir::new("srv-file");
    let socket = dir.path().join("notchlingd.sock");
    std::fs::write(&socket, b"keep me").unwrap();
    let err = Server::bind(config(&socket), Arc::new(TestHandler))
        .err()
        .unwrap();
    assert_eq!(err.kind(), std::io::ErrorKind::AlreadyExists);
    assert_eq!(std::fs::read(&socket).unwrap(), b"keep me");
}

#[test]
fn shutdown_closes_open_clients() {
    let run = start();
    let (mut s, mut r) = raw(&run.socket);
    send(&mut s, HELLO);
    reply(&mut r);
    run.handle.shutdown();
    assert!(closed(&mut s));
}

#[path = "server_exec_tests.rs"]
mod exec;
