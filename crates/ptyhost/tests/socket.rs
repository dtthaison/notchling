#![cfg(unix)]

mod common;

use common::*;
use notchling_ptyhost::client::HostClient;
use notchling_ptyhost::host::{Handle, HostOptions, SessionHost};
use notchling_ptyhost::pty_handle::PtySpawner;
use notchling_ptyhost::server::{HostServer, PROTOCOL_VERSION};
use notchling_ptyhost::state::StateStore;
use notchling_ptyhost::HostError;
use serde_json::{json, Map, Value};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

const TIMEOUT: Duration = Duration::from_secs(5);

fn params(value: Value) -> Map<String, Value> {
    value.as_object().expect("an object").clone()
}

struct Running {
    server: Arc<HostServer>,
    host: Arc<SessionHost>,
    path: PathBuf,
}

impl Drop for Running {
    fn drop(&mut self) {
        self.server.stop();
    }
}

fn start_server(
    scratch: &Scratch,
    spawner: Arc<FakeSpawner>,
    authorize: fn(u32) -> bool,
) -> Running {
    let (host, _) = make_host(scratch, spawner, never_alive, nothing_found);
    let path = scratch.path("h.sock");
    let server = HostServer::new(host.clone(), &path, Arc::new(authorize));
    server.start().expect("the server starts");
    Running { server, host, path }
}

fn owner(uid: u32) -> bool {
    // SAFETY: getuid has no preconditions.
    uid == unsafe { libc::getuid() }
}

fn connect(path: &Path) -> HostClient {
    HostClient::connect(path, Duration::from_secs(3)).expect("connects")
}

#[cfg(unix)]
fn mode(path: &Path) -> u32 {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path)
        .expect("exists")
        .permissions()
        .mode()
        & 0o777
}

#[test]
fn the_socket_is_private_and_serves_the_owner() {
    let scratch = Scratch::new();
    let running = start_server(&scratch, FakeSpawner::new(), owner);
    assert_eq!(mode(&running.path), 0o600);
    assert_eq!(mode(&scratch.dir), 0o700);
    let client = connect(&running.path);
    let hello = client.request("hello", Map::new(), TIMEOUT).expect("hello");
    assert_eq!(hello["version"], json!(PROTOCOL_VERSION));
    assert_eq!(hello["pid"], json!(std::process::id()));
    client.close();
}

#[test]
fn another_uid_is_refused_before_any_request_is_served() {
    let scratch = Scratch::new();
    let spawner = FakeSpawner::new();
    let running = start_server(&scratch, spawner.clone(), |uid| !owner(uid));
    let client = connect(&running.path);
    let answer = client.request(
        "spawn",
        params(json!({"cwd": "/tmp", "kind": "claude"})),
        Duration::from_secs(2),
    );
    assert!(answer.is_err(), "{answer:?}");
    assert!(spawner.spawns().is_empty());
    assert!(running.host.list().is_empty());
}

#[test]
fn a_second_host_on_the_same_socket_refuses_to_start() {
    let scratch = Scratch::new();
    let running = start_server(&scratch, FakeSpawner::new(), owner);
    let (other, _) = make_host(&scratch, FakeSpawner::new(), never_alive, nothing_found);
    let second = HostServer::new(other, &running.path, Arc::new(owner));
    assert!(matches!(second.start(), Err(HostError::AlreadyRunning)));
    connect(&running.path)
        .request("hello", Map::new(), TIMEOUT)
        .expect("still serving");
}

#[test]
fn a_stale_socket_file_is_replaced() {
    let scratch = Scratch::new();
    let path = scratch.path("h.sock");
    std::fs::write(&path, b"").expect("a leftover file");
    let (host, _) = make_host(&scratch, FakeSpawner::new(), never_alive, nothing_found);
    let server = HostServer::new(host, &path, Arc::new(owner));
    server.start().expect("starts over the stale file");
    connect(&path)
        .request("hello", Map::new(), TIMEOUT)
        .expect("hello");
    server.stop();
    assert!(!path.exists(), "stop removes the socket");
}

#[test]
fn a_socket_path_that_is_too_long_is_refused() {
    let scratch = Scratch::new();
    let path = scratch.path(&"x".repeat(120));
    let (host, _) = make_host(&scratch, FakeSpawner::new(), never_alive, nothing_found);
    let server = HostServer::new(host, &path, Arc::new(owner));
    assert!(matches!(server.start(), Err(HostError::PathTooLong)));
}

#[test]
fn spawn_list_write_attach_resize_and_terminate_over_the_socket() {
    let scratch = Scratch::new();
    let spawner = FakeSpawner::new();
    let running = start_server(&scratch, spawner.clone(), owner);
    let client = connect(&running.path);
    let exits: Arc<Mutex<Vec<String>>> = Arc::default();
    let seen = exits.clone();
    client.on_event(move |event| {
        if event["event"] == json!("exit") {
            let id = event["sessionId"].as_str().unwrap_or("").to_string();
            seen.lock().expect("lock").push(id);
        }
    });

    let reply = client
        .request(
            "spawn",
            params(json!({"kind": "claude", "cwd": "/tmp", "model": "sonnet"})),
            TIMEOUT,
        )
        .expect("spawned");
    let info = HostClient::info_from(&Value::Object(reply)).expect("an info");
    assert_eq!(spawner.spawns()[0].arguments, ["--model", "sonnet"]);
    assert_eq!(info.model.as_deref(), Some("sonnet"));
    assert!(info.running);
    assert_eq!(client.sessions().expect("listed").len(), 1);

    spawner.handle(0).emit("before attach ");
    let received: Arc<Mutex<String>> = Arc::default();
    let sink = received.clone();
    client
        .attach(info.id, move |chunk| {
            sink.lock()
                .expect("lock")
                .push_str(&String::from_utf8_lossy(chunk))
        })
        .expect("attached");
    spawner.handle(0).emit("after");
    wait_until(|| received.lock().expect("lock").as_str() == "before attach after");

    client.write(info.id, b"hello\r").expect("sent");
    wait_until(|| spawner.handle(0).written().contains(&b"hello\r".to_vec()));
    client
        .request(
            "resize",
            params(json!({"session": info.id.to_string(), "columns": 120, "rows": 40})),
            TIMEOUT,
        )
        .expect("resized");
    assert_eq!(spawner.handle(0).resizes().last(), Some(&(120, 40)));
    client.detach(info.id).expect("detached");

    client
        .request(
            "terminate",
            params(json!({"session": info.id.to_string(), "confirm": true})),
            TIMEOUT,
        )
        .expect("terminated");
    assert!(spawner.handle(0).terminated());
    wait_until(|| exits.lock().expect("lock").contains(&info.id.to_string()));
    assert!(client.sessions().expect("listed").is_empty());
}

#[test]
fn terminate_needs_the_confirm_flag() {
    let scratch = Scratch::new();
    let spawner = FakeSpawner::new();
    let running = start_server(&scratch, spawner.clone(), owner);
    let client = connect(&running.path);
    let reply = client
        .request("spawn", params(json!({"cwd": "/tmp"})), TIMEOUT)
        .expect("spawned");
    let id = reply["id"].as_str().expect("an id").to_string();
    let refused = client.request("terminate", params(json!({"session": id})), TIMEOUT);
    match refused {
        Err(HostError::Refused(why)) => assert!(why.contains("confirm"), "{why}"),
        other => panic!("{other:?}"),
    }
    assert!(!spawner.handle(0).terminated());
}

#[test]
fn an_app_that_restarts_finds_the_running_sessions_again() {
    let scratch = Scratch::new();
    let spawner = FakeSpawner::new();
    let running = start_server(&scratch, spawner.clone(), owner);
    let first = connect(&running.path);
    first
        .request("spawn", params(json!({"cwd": "/tmp"})), TIMEOUT)
        .expect("spawned");
    first.close();

    let second = connect(&running.path);
    let found: Vec<_> = second
        .sessions()
        .expect("listed")
        .into_iter()
        .filter(|s| s.running)
        .collect();
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].pid, spawner.handle(0).root_pid());
    second.write(found[0].id, b"still here").expect("sent");
    wait_until(|| {
        spawner
            .handle(0)
            .written()
            .contains(&b"still here".to_vec())
    });
}

#[test]
fn settings_and_status_over_the_socket() {
    let scratch = Scratch::new();
    let running = start_server(&scratch, FakeSpawner::new(), owner);
    let client = connect(&running.path);
    let ask = |op: &str, p: Value| client.request(op, params(p), TIMEOUT);
    assert_eq!(
        ask("status", json!({})).expect("status")["running"],
        json!(0)
    );
    assert_eq!(
        ask("settings.get", json!({})).expect("get")["resumeOnStart"],
        json!(true)
    );
    assert_eq!(
        ask("settings.set", json!({"resumeOnStart": false})).expect("set")["resumeOnStart"],
        json!(false)
    );
    assert_eq!(
        ask("settings.get", json!({})).expect("get")["resumeOnStart"],
        json!(false)
    );
    assert!(matches!(ask("nope", json!({})), Err(HostError::Refused(_))));
    assert!(matches!(
        ask("spawn", json!({"kind": "claude"})),
        Err(HostError::Refused(_))
    ));
    assert!(matches!(
        ask("attach", json!({"session": "not-an-id"})),
        Err(HostError::Refused(_))
    ));
}

#[test]
fn losing_the_host_closes_the_client() {
    let scratch = Scratch::new();
    let running = start_server(&scratch, FakeSpawner::new(), owner);
    let client = connect(&running.path);
    let (tx, rx) = std::sync::mpsc::channel();
    client.on_closed(move || {
        let _ = tx.send(());
    });
    running.server.stop();
    rx.recv_timeout(Duration::from_secs(3))
        .expect("told the host is gone");
    assert!(!client.is_connected());
    assert!(matches!(
        client.request("hello", Map::new(), TIMEOUT),
        Err(HostError::Closed)
    ));
}

fn real_server(scratch: &Scratch) -> Running {
    let state = Arc::new(StateStore::new(scratch.path("state.json")));
    let host = SessionHost::new(
        Arc::new(PtySpawner::with_environment(vec![
            ("PATH".into(), "/usr/bin:/bin".into()),
            ("NO_COLOR".into(), "1".into()),
        ])),
        state,
        HostOptions::default(),
    );
    let path = scratch.path("h.sock");
    let server = HostServer::new(host.clone(), &path, Arc::new(owner));
    server.start().expect("the server starts");
    Running { server, host, path }
}

#[test]
fn a_cat_session_over_the_socket() {
    let scratch = Scratch::new();
    let running = real_server(&scratch);
    let client = connect(&running.path);
    let exits: Arc<Mutex<Vec<String>>> = Arc::default();
    let seen = exits.clone();
    client.on_event(move |event| {
        if event["event"] == json!("exit") {
            seen.lock()
                .expect("lock")
                .push(event["sessionId"].as_str().unwrap_or("").to_string());
        }
    });
    let cwd = scratch.dir.to_string_lossy().into_owned();
    let reply = client
        .request(
            "spawn",
            params(
                json!({"kind": "command", "cwd": cwd, "executable": "/bin/cat", "arguments": []}),
            ),
            TIMEOUT,
        )
        .expect("spawned");
    let info = HostClient::info_from(&Value::Object(reply)).expect("an info");
    assert!(info.pid > 1);
    let received: Arc<Mutex<Vec<u8>>> = Arc::default();
    let sink = received.clone();
    client
        .attach(info.id, move |chunk| {
            sink.lock().expect("lock").extend_from_slice(chunk)
        })
        .expect("attached");
    client.write(info.id, b"hello host\n").expect("sent");
    wait_until(|| {
        String::from_utf8_lossy(&received.lock().expect("lock"))
            .matches("hello host")
            .count()
            == 2
    });
    client
        .request(
            "terminate",
            params(json!({"session": info.id.to_string(), "confirm": true})),
            TIMEOUT,
        )
        .expect("terminated");
    wait_until(|| exits.lock().expect("lock").contains(&info.id.to_string()));
    assert_eq!(running.host.running_count(), 0);
}

#[test]
fn a_shell_session_sees_its_size_and_no_no_color() {
    let scratch = Scratch::new();
    let running = real_server(&scratch);
    let client = connect(&running.path);
    let cwd = scratch.dir.to_string_lossy().into_owned();
    let script = "echo \"nc=${NO_COLOR-unset}\"; trap 'stty size' WINCH; echo ready; while :; do sleep 0.05; done";
    let reply = client
        .request(
            "spawn",
            params(json!({"kind": "command", "cwd": cwd, "executable": "/bin/sh", "arguments": ["-c", script]})),
            TIMEOUT,
        )
        .expect("spawned");
    let id = HostClient::info_from(&Value::Object(reply))
        .expect("an info")
        .id;
    let received: Arc<Mutex<Vec<u8>>> = Arc::default();
    let sink = received.clone();
    client
        .attach(id, move |chunk| {
            sink.lock().expect("lock").extend_from_slice(chunk)
        })
        .expect("attached");
    let text = || String::from_utf8_lossy(&received.lock().expect("lock")).into_owned();
    wait_until(|| text().contains("ready"));
    assert!(text().contains("nc=unset"), "{}", text());
    client
        .request(
            "resize",
            params(json!({"session": id.to_string(), "columns": 77, "rows": 33})),
            TIMEOUT,
        )
        .expect("resized");
    wait_until(|| text().contains("33 77"));
    client
        .request(
            "terminate",
            params(json!({"session": id.to_string(), "confirm": true})),
            TIMEOUT,
        )
        .expect("terminated");
    wait_until(|| running.host.running_count() == 0);
}
