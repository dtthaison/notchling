#![cfg(unix)]

use crate::acting::Acting;
use crate::acting_ops::{ActingOps, NoTerminal};
use crate::acting_ops_tests::sessions;
use crate::daemon::{LateRoom, Ops};
use crate::hook::{decide, headless, HookOptions};
use notchling_engine::sessions::Roots;
use notchling_local_api::{
    ops, topics, Allowlist, Client, ClientKind, ConnectOptions, ErrorCode, Limits, Server,
    ServerConfig, ServerHandle,
};
use serde_json::{json, Value};
use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/hook")
}

fn fixture(name: &str) -> Value {
    serde_json::from_slice(&std::fs::read(fixtures().join(name)).unwrap()).unwrap()
}

fn input(name: &str) -> Vec<u8> {
    std::fs::read(fixtures().join(name)).unwrap()
}

struct Dir(PathBuf);

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn scratch(tag: &str) -> Dir {
    use std::sync::atomic::{AtomicU32, Ordering};
    static N: AtomicU32 = AtomicU32::new(0);
    let n = N.fetch_add(1, Ordering::SeqCst);
    let dir = PathBuf::from(format!("/tmp/nl-hk-{tag}-{}-{n}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    Dir(dir)
}

struct Daemon {
    dir: Dir,
    socket: PathBuf,
    handle: ServerHandle,
    ops: Arc<ActingOps>,
}

impl Drop for Daemon {
    fn drop(&mut self) {
        self.ops.close();
        self.handle.shutdown();
    }
}

fn daemon(acting: bool, app_tier: bool) -> Daemon {
    let dir = scratch("d");
    let socket = dir.0.join("s.sock");
    let room = Arc::new(LateRoom::default());
    let roots = Roots {
        main_dir: dir.0.join("claude"),
        lanes_file: dir.0.join("lanes.json"),
    };
    let acting_ops = Arc::new(ActingOps::new(
        Arc::new(Acting::in_memory(acting)),
        Arc::new(Mutex::new(sessions())),
        roots,
        room.clone(),
        Box::new(NoTerminal),
    ));
    let handler = Ops::with_acting(Arc::new(Mutex::new(sessions())), acting_ops.clone());
    let allow = if app_tier {
        Allowlist::new([std::env::current_exe().unwrap()])
    } else {
        Allowlist::empty()
    };
    let server = Server::bind(
        ServerConfig {
            socket: socket.clone(),
            allowlist: allow,
            audit: None,
            version: "9.9.9".into(),
            limits: Limits::default(),
        },
        Arc::new(handler),
    )
    .unwrap();
    let (handle, _thread) = server.spawn().unwrap();
    room.set(handle.clone());
    Daemon {
        dir,
        socket,
        handle,
        ops: acting_ops,
    }
}

fn claim_session(d: &Daemon) {
    let sessions = d.dir.0.join("claude/sessions");
    std::fs::create_dir_all(&sessions).unwrap();
    // SAFETY: getppid takes nothing and can't fail.
    let parent = unsafe { libc::getppid() };
    let session = json!({"pid": parent, "sessionId": crate::acting_ops_tests::SESSION,
        "cwd": "/tmp/demo", "entrypoint": "cli"});
    std::fs::write(sessions.join(format!("{parent}.json")), session.to_string()).unwrap();
}

fn connect(socket: &Path, kind: ClientKind) -> Client {
    let options = ConnectOptions {
        check_server: false,
        ..ConnectOptions::default()
    };
    Client::connect_with(socket, kind, "0.1.0", &options).unwrap()
}

fn options(socket: &Path, claude_dir: &Path, wait_ms: u64) -> HookOptions {
    HookOptions {
        socket: socket.to_path_buf(),
        claude_dir: claude_dir.to_path_buf(),
        ancestors: vec![4242],
        wait: Duration::from_millis(wait_ms),
        slack: Duration::from_millis(500),
        connect_timeout: Duration::from_millis(500),
        check_server: false,
    }
}

fn answering_app(socket: &Path, op: &'static str, args: Value) -> std::thread::JoinHandle<Value> {
    let mut app = connect(socket, ClientKind::App);
    app.subscribe(&[topics::PROMPTS]).unwrap();
    std::thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(10);
        while Instant::now() < deadline {
            if let Ok(Some(e)) = app.next_event(Duration::from_millis(100)) {
                if e.event == topics::PROMPT_PENDING {
                    let mut args = args.clone();
                    args["id"] = e.data["id"].clone();
                    return app.call(op, args).unwrap();
                }
            }
        }
        panic!("no prompt arrived");
    })
}

#[test]
fn passes_at_once_with_no_daemon() {
    let dir = scratch("none");
    let started = Instant::now();
    let out = decide(
        &input("permission-request.json"),
        &options(&dir.0.join("missing.sock"), &dir.0, 10_000),
    );
    assert_eq!(out, None);
    assert!(
        started.elapsed() < Duration::from_millis(200),
        "{:?}",
        started.elapsed()
    );
}

#[test]
fn passes_when_no_app_is_attached() {
    let d = daemon(true, true);
    let out = decide(
        &input("permission-request.json"),
        &options(&d.socket, &d.dir.0, 10_000),
    );
    assert_eq!(out, None);
    assert!(d.ops.prompts().list().is_empty());
}

#[test]
fn passes_on_a_socket_error() {
    let dir = scratch("err");
    let socket = dir.0.join("rude.sock");
    let listener = UnixListener::bind(&socket).unwrap();
    let rude = std::thread::spawn(move || {
        if let Ok((stream, _)) = listener.accept() {
            drop(stream);
        }
    });
    let out = decide(
        &input("permission-request.json"),
        &options(&socket, &dir.0, 10_000),
    );
    assert_eq!(out, None);
    let _ = rude.join();
    std::fs::write(dir.0.join("file.sock"), b"not a socket").unwrap();
    let out = decide(
        &input("permission-request.json"),
        &options(&dir.0.join("file.sock"), &dir.0, 10_000),
    );
    assert_eq!(out, None);
}

#[test]
fn passes_for_a_claude_p_session_without_asking() {
    let d = daemon(true, true);
    claim_session(&d);
    let app = answering_app(
        &d.socket,
        ops::SESSION_PERMISSION,
        json!({"decision": "allow"}),
    );
    let sdk = fixtures().join("sdk-cli/claude");
    assert!(headless(&sdk.join("sessions"), &[4242]));
    let out = decide(
        &input("permission-request.json"),
        &options(&d.socket, &sdk, 10_000),
    );
    assert_eq!(out, None);
    assert!(
        d.ops.prompts().list().is_empty(),
        "the daemon never heard of it"
    );
    let cli = fixtures().join("cli/claude");
    assert!(!headless(&cli.join("sessions"), &[4242]));
    let out = decide(
        &input("permission-request.json"),
        &options(&d.socket, &cli, 10_000),
    );
    assert_eq!(out, Some(fixture("out-allow.json")));
    app.join().unwrap();
}

#[test]
fn passes_at_its_deadline() {
    let d = daemon(true, true);
    claim_session(&d);
    let mut app = connect(&d.socket, ClientKind::App);
    app.subscribe(&[topics::PROMPTS]).unwrap();
    let started = Instant::now();
    let out = decide(
        &input("permission-request.json"),
        &options(&d.socket, &d.dir.0, 300),
    );
    assert_eq!(out, None);
    assert!(
        started.elapsed() < Duration::from_secs(3),
        "{:?}",
        started.elapsed()
    );
    let mut seen = Vec::new();
    while let Ok(Some(e)) = app.next_event(Duration::from_millis(300)) {
        seen.push((e.event, e.data["by"].clone()));
    }
    assert_eq!(
        seen.last(),
        Some(&(topics::PROMPT_RESOLVED.to_string(), json!("timeout")))
    );
}

#[test]
fn passes_when_acting_is_off() {
    let d = daemon(false, true);
    let _app = connect(&d.socket, ClientKind::App);
    let out = decide(
        &input("permission-request.json"),
        &options(&d.socket, &d.dir.0, 10_000),
    );
    assert_eq!(out, None);
    let mut hook = connect(&d.socket, ClientKind::Hook);
    let reply = hook
        .call(
            ops::HOOK_REQUEST,
            json!({"event": fixture("permission-request.json")}),
        )
        .unwrap();
    assert_eq!(reply, json!({"action": "pass", "reason": "acting is off"}));
}

#[test]
fn daemon_status_carries_the_switch_for_the_app() {
    for on in [false, true] {
        let d = daemon(on, true);
        let mut app = connect(&d.socket, ClientKind::App);
        assert_eq!(app.status().unwrap().acting, Some(on));
        let raw = app.call(ops::DAEMON_STATUS, json!({})).unwrap();
        assert_eq!(raw["acting"], json!(on), "{raw}");
        app.call(ops::SETTINGS_SET_ACTING, json!({"on": !on}))
            .unwrap();
        assert_eq!(app.status().unwrap().acting, Some(!on));
    }
}

#[test]
fn prints_the_apps_decision_as_the_swift_hook_does() {
    let d = daemon(true, true);
    claim_session(&d);
    let app = answering_app(
        &d.socket,
        ops::SESSION_PERMISSION,
        json!({"decision": "always"}),
    );
    let out = decide(
        &input("permission-request.json"),
        &options(&d.socket, &d.dir.0, 10_000),
    );
    assert_eq!(out, Some(fixture("out-always.json")));
    app.join().unwrap();

    let app = answering_app(
        &d.socket,
        ops::SESSION_PERMISSION,
        json!({"decision": "deny"}),
    );
    let out = decide(
        &input("permission-request.json"),
        &options(&d.socket, &d.dir.0, 10_000),
    );
    assert_eq!(out, Some(fixture("out-deny.json")));
    app.join().unwrap();

    let app = answering_app(
        &d.socket,
        ops::SESSION_ANSWER,
        json!({"answers": [[1, 3], [2]]}),
    );
    let out = decide(
        &input("ask-user-question.json"),
        &options(&d.socket, &d.dir.0, 10_000),
    );
    assert_eq!(out, Some(fixture("out-answer-pretooluse.json")));
    app.join().unwrap();

    let app = answering_app(&d.socket, ops::SESSION_ANSWER, json!({"answers": [[1]]}));
    let out = decide(
        &input("ask-as-permission.json"),
        &options(&d.socket, &d.dir.0, 10_000),
    );
    assert_eq!(out, Some(fixture("out-answer-permission.json")));
    app.join().unwrap();
}

#[test]
fn the_agent_tier_reaches_no_acting_op_and_no_decision() {
    let d = daemon(true, false);
    let mut agent = connect(&d.socket, ClientKind::Cli);
    for op in crate::acting_ops::ACTING_OPS
        .into_iter()
        .chain([ops::SETTINGS_SET_ACTING, ops::PROMPTS_LIST])
    {
        match agent.call(op, json!({"id": "job-a", "on": true})) {
            Err(notchling_local_api::ClientError::Api(e)) => {
                assert_eq!(e.code, ErrorCode::Forbidden, "{op}")
            }
            other => panic!("{op}: {other:?}"),
        }
    }
    let mut posing = connect(&d.socket, ClientKind::App);
    assert!(posing
        .call(ops::SESSION_REPLY, json!({"id": "job-a", "text": "x"}))
        .is_err());
    match agent.call(
        ops::HOOK_REQUEST,
        json!({"event": fixture("permission-request.json")}),
    ) {
        Err(notchling_local_api::ClientError::Api(e)) => assert_eq!(e.code, ErrorCode::Forbidden),
        other => panic!("{other:?}"),
    }
    let mut hook = connect(&d.socket, ClientKind::Hook);
    match hook.call(
        ops::HOOK_REQUEST,
        json!({"event": fixture("permission-request.json"), "pid": 4242}),
    ) {
        Err(notchling_local_api::ClientError::Api(e)) => {
            assert_eq!(e.code, ErrorCode::Forbidden, "{}", e.message)
        }
        other => panic!("{other:?}"),
    }
    assert!(d.ops.prompts().list().is_empty());
    match agent.subscribe(&[topics::PROMPTS]) {
        Err(notchling_local_api::ClientError::Api(e)) => assert_eq!(e.code, ErrorCode::Forbidden),
        other => panic!("{other:?}"),
    }
    agent.subscribe(&[topics::SESSIONS]).unwrap();
    assert_eq!(
        agent.call(ops::SETTINGS_GET_ACTING, json!({})).unwrap(),
        json!({"acting": true})
    );
}

#[test]
fn a_hook_from_another_session_is_refused_and_passes() {
    let d = daemon(true, true);
    let mut app = connect(&d.socket, ClientKind::App);
    app.subscribe(&[topics::PROMPTS]).unwrap();
    claim_session(&d);
    let mut other = fixture("permission-request.json");
    other["session_id"] = json!("00000000-0000-0000-0000-000000000000");
    let out = decide(
        other.to_string().as_bytes(),
        &options(&d.socket, &d.dir.0, 10_000),
    );
    assert_eq!(out, None, "refused, so the hook passes");
    assert!(d.ops.prompts().list().is_empty());
    assert!(matches!(
        app.next_event(Duration::from_millis(200)),
        Ok(None)
    ));
    let mut hook = connect(&d.socket, ClientKind::Hook);
    let waiter = std::thread::spawn(move || {
        hook.call(
            ops::HOOK_REQUEST,
            json!({"event": fixture("permission-request.json"), "pid": 1, "waitMs": 400}),
        )
        .unwrap()
    });
    let deadline = Instant::now() + Duration::from_secs(5);
    while d.ops.prompts().list().is_empty() {
        assert!(Instant::now() < deadline, "never held");
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(d.ops.prompts().list()[0].hook_pid, Some(std::process::id()));
    assert_eq!(waiter.join().unwrap()["reason"], "timeout");
}

#[test]
fn a_daemon_that_accepts_and_never_answers_lets_the_hook_pass_by_its_deadline() {
    let dir = scratch("mute");
    let socket = dir.0.join("mute.sock");
    let listener = UnixListener::bind(&socket).unwrap();
    let mute = std::thread::spawn(move || {
        let held = listener.accept();
        std::thread::sleep(Duration::from_secs(4));
        drop(held);
    });
    let mut opts = options(&socket, &dir.0, 300);
    opts.connect_timeout = Duration::from_millis(300);
    opts.slack = Duration::from_millis(300);
    let started = Instant::now();
    let out = decide(&input("permission-request.json"), &opts);
    assert_eq!(out, None);
    let took = started.elapsed();
    assert!(took < Duration::from_secs(2), "{took:?}");
    let _ = mute.join();
}

#[test]
fn an_event_over_the_line_cap_passes_at_once() {
    let d = daemon(true, true);
    claim_session(&d);
    let mut app = connect(&d.socket, ClientKind::App);
    app.subscribe(&[topics::PROMPTS]).unwrap();
    let mut big = fixture("permission-request.json");
    big["tool_name"] = json!("Write");
    big["tool_input"] = json!({"file_path": "/tmp/demo/big.txt",
        "content": "x".repeat(notchling_local_api::MAX_LINE + 1)});
    let started = Instant::now();
    let out = decide(
        big.to_string().as_bytes(),
        &options(&d.socket, &d.dir.0, 10_000),
    );
    assert_eq!(out, None);
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "{:?}",
        started.elapsed()
    );
    assert!(d.ops.prompts().list().is_empty());
}
