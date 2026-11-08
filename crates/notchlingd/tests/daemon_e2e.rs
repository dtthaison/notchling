//! The daemon end to end over a copy of the M1 notch fixture in a temp data root: it serves
//! `sessions.list`, sends `sessions.changed` after a fixture file changes, refuses a second daemon,
//! starts past a stale socket and stops cleanly on SIGTERM, removing its socket. It never sees the
//! real data folder: every run sets `NOTCHLING_DATA_ROOT` and a fake `HOME`. A guard stops every
//! daemon a test starts, detached ones included, even when an assert fails midway.
#![cfg(unix)]

use notchling_local_api::{ensure_daemon_with, Client, ClientKind, SpawnOptions, Started};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

struct Dir(PathBuf);

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A short folder under /tmp (socket paths are capped at 104 bytes on macOS).
fn scratch(tag: &str) -> Dir {
    let dir = PathBuf::from(format!("/tmp/nl-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    Dir(dir)
}

fn copy_tree(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_tree(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), &target).unwrap();
        }
    }
}

/// A data root holding a copy of `crates/app/fixtures/notch`.
fn fixture_root(dir: &Path) -> PathBuf {
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("../app/fixtures/notch");
    let root = dir.join("root");
    copy_tree(&fixture, &root);
    root
}

fn daemon(root: &Path, home: &Path) -> Command {
    let mut c = Command::new(env!("CARGO_BIN_EXE_notchling"));
    c.arg("daemon")
        .env("NOTCHLING_DATA_ROOT", root)
        .env("HOME", home)
        .env_remove(notchling_local_api::APP_TIER_PATHS_ENV)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    c
}

fn wait_for_socket(socket: &Path) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        if std::os::unix::net::UnixStream::connect(socket).is_ok() {
            return;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    panic!("no daemon on {}", socket.display());
}

/// Stops a daemon a test started, however the test ends: an assert that fails mid-test must not
/// leave a daemon running. SIGTERM, a short wait, then SIGKILL.
struct Daemon {
    /// Started here as a child.
    child: Option<Child>,
    /// Started detached by `ensure_daemon`: found by its socket, its pid asked of it.
    socket: Option<PathBuf>,
}

impl Daemon {
    fn child(child: Child) -> Daemon {
        Daemon {
            child: Some(child),
            socket: None,
        }
    }

    fn detached(socket: &Path) -> Daemon {
        Daemon {
            child: None,
            socket: Some(socket.to_path_buf()),
        }
    }
}

fn alive(pid: libc::pid_t) -> bool {
    // SAFETY: signal 0 only asks whether the process exists.
    unsafe { libc::kill(pid, 0) == 0 }
}

fn stop_pid(pid: libc::pid_t) {
    // SAFETY: a daemon this test started.
    unsafe { libc::kill(pid, libc::SIGTERM) };
    let deadline = Instant::now() + Duration::from_secs(5);
    while alive(pid) && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(50));
    }
    if alive(pid) {
        // SAFETY: as above.
        unsafe { libc::kill(pid, libc::SIGKILL) };
    }
}

impl Drop for Daemon {
    fn drop(&mut self) {
        if let Some(child) = &mut self.child {
            if matches!(child.try_wait(), Ok(None)) {
                stop_pid(child.id() as libc::pid_t);
                let _ = child.kill();
                let _ = child.wait();
            }
        }
        if let Some(socket) = &self.socket {
            let pid = Client::connect(socket, ClientKind::Cli, "0.1.0")
                .and_then(|mut c| c.status())
                .map(|s| s.pid);
            if let Ok(pid) = pid {
                stop_pid(pid as libc::pid_t);
            }
        }
    }
}

fn terminate(child: &mut Child) -> std::process::ExitStatus {
    // SAFETY: signalling our own child by its pid.
    unsafe { libc::kill(child.id() as libc::pid_t, libc::SIGTERM) };
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            return status;
        }
        assert!(Instant::now() < deadline, "the daemon ignored SIGTERM");
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn ids(list: &Value) -> Vec<String> {
    list.as_array()
        .unwrap()
        .iter()
        .map(|a| a["id"].as_str().unwrap().to_string())
        .collect()
}

#[test]
fn serves_the_fixture_and_its_changes_then_stops_on_sigterm() {
    let dir = scratch("e2e");
    let root = fixture_root(&dir.0);
    let socket = root.join("notchling/notchlingd.sock");
    // A socket file left by a crashed daemon.
    drop(std::os::unix::net::UnixListener::bind(&socket).unwrap());
    assert!(socket.exists());

    let mut running = Daemon::child(daemon(&root, &dir.0).spawn().unwrap());
    wait_for_socket(&socket);

    let mut client = Client::connect(&socket, ClientKind::Cli, "0.1.0").unwrap();
    assert_eq!(client.tier(), notchling_local_api::Tier::Agent);
    client.subscribe(&["sessions"]).unwrap();
    let list = client
        .call(
            notchling_local_api::ops::SESSIONS_LIST,
            serde_json::json!({}),
        )
        .unwrap();
    let rows = list.as_array().unwrap();
    assert!(rows.len() >= 5, "{list}");
    // The engine's own field names, through serde.
    let first = &rows[0];
    for key in [
        "id", "name", "project", "group", "label", "sinceMs", "target",
    ] {
        assert!(first.get(key).is_some(), "no {key} in {first}");
    }
    assert!(ids(&list).contains(&"sess-notch".to_string()), "{list}");
    let working = rows.iter().find(|r| r["id"] == "sess-notch").unwrap();
    assert_eq!(working["group"], "working");

    // The job stops to ask: its state file changes.
    let state = root.join("claude/jobs/a1b2c3d4/state.json");
    let text = std::fs::read_to_string(&state).unwrap();
    std::fs::write(&state, text.replace("\"working\"", "\"blocked\"")).unwrap();

    let event = client
        .next_event(Duration::from_secs(15))
        .unwrap()
        .expect("a sessions.changed event");
    assert_eq!(event.event, "sessions.changed");
    let row = event
        .data
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["id"] == "sess-notch")
        .unwrap()
        .clone();
    assert_ne!(row["group"], "working", "{row}");

    // A second daemon on the same socket says so and exits 0, leaving the first serving.
    let second = daemon(&root, &dir.0).output().unwrap();
    assert!(second.status.success());
    assert!(String::from_utf8_lossy(&second.stderr).contains("already serves"));
    assert!(client.status().is_ok());

    // `notchling status` and `sessions` talk to it.
    let status = Command::new(env!("CARGO_BIN_EXE_notchling"))
        .args(["status", "--socket"])
        .arg(&socket)
        .output()
        .unwrap();
    assert!(status.status.success());
    assert!(String::from_utf8_lossy(&status.stdout).contains("clients: app 0, cli"));
    let sessions = Command::new(env!("CARGO_BIN_EXE_notchling"))
        .args(["sessions", "--socket"])
        .arg(&socket)
        .output()
        .unwrap();
    assert!(String::from_utf8_lossy(&sessions.stdout).contains("notch layout"));

    drop(client);
    let status = terminate(running.child.as_mut().unwrap());
    assert!(status.success(), "{status:?}");
    assert!(!socket.exists(), "the socket is removed on SIGTERM");
    // The lock file stays (it holds nothing once the process is gone); nothing else was written
    // outside the data root's notchling folder.
    assert!(root.join("notchling/notchlingd.lock").exists());
}

#[test]
fn a_client_with_no_daemon_says_so() {
    let dir = scratch("e2e-none");
    let out = Command::new(env!("CARGO_BIN_EXE_notchling"))
        .args(["status", "--socket"])
        .arg(dir.0.join("none.sock"))
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(3));
    assert!(String::from_utf8_lossy(&out.stderr).contains("isn't running"));
}

#[test]
fn ensure_daemon_starts_one_and_then_finds_it() {
    let dir = scratch("e2e-ensure");
    let root = fixture_root(&dir.0);
    let socket = dir.0.join("s/notchlingd.sock");
    let binary = Path::new(env!("CARGO_BIN_EXE_notchling"));
    let options = SpawnOptions {
        data_root: Some(root.clone()),
        ..SpawnOptions::default()
    };
    // Detached, so not our child to kill: the guard finds it by its socket if the test fails.
    let _running = Daemon::detached(&socket);
    assert_eq!(
        ensure_daemon_with(binary, &socket, &options).unwrap(),
        Started::Spawned
    );
    assert_eq!(
        ensure_daemon_with(binary, &socket, &options).unwrap(),
        Started::Running
    );
    let mut client = Client::connect(&socket, ClientKind::App, "0.1.0").unwrap();
    let rows: Vec<Value> = client.sessions().unwrap();
    assert!(!rows.is_empty());
    // Its log, lock and audit log sit beside the given socket, not in the data root.
    assert!(dir.0.join("s/notchlingd.log").exists());
    assert!(dir.0.join("s/notchlingd.lock").exists());
    let pid = client.status().unwrap().pid;
    drop(client);
    // SAFETY: the daemon this test started.
    unsafe { libc::kill(pid as libc::pid_t, libc::SIGTERM) };
    let deadline = Instant::now() + Duration::from_secs(10);
    while socket.exists() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(!socket.exists());
}
