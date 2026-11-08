//! The real pty handle's scrollback, and the `notchling-ptyhost` binary itself, always with
//! `--socket` and `--state` in a scratch folder so the running hostd is never asked or replaced.
#![cfg(unix)]

mod common;

use common::*;
use notchling_ptyhost::client::HostClient;
use notchling_ptyhost::host::Handle;
use notchling_ptyhost::pty_handle::{PtyHandle, SCROLLBACK_LIMIT};
use notchling_ptyhost::state::StateStore;
use serde_json::{json, Map};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[test]
fn scrollback_keeps_the_last_two_megabytes_of_a_real_pty() {
    let scratch = Scratch::new();
    let env = vec![
        ("PATH".to_string(), "/usr/bin:/bin".to_string()),
        ("TERM".to_string(), "xterm".to_string()),
    ];
    let pty = PtyHandle::spawn(
        Path::new("/bin/sh"),
        &[
            "-c".to_string(),
            "head -c 3000000 /dev/zero | tr '\\0' 'a'; echo END; sleep 0.3".to_string(),
        ],
        &scratch.dir,
        None,
        &env,
    )
    .expect("sh starts");
    let (tx, rx) = std::sync::mpsc::channel();
    pty.on_exit(Box::new(move || {
        let _ = tx.send(());
    }));
    rx.recv_timeout(Duration::from_secs(20)).expect("exited");
    let tail: Arc<Mutex<Vec<u8>>> = Arc::default();
    let sink = tail.clone();
    pty.add_listener(Arc::new(move |chunk: &[u8]| {
        sink.lock().expect("lock").extend_from_slice(chunk)
    }));
    let got = tail.lock().expect("lock").clone();
    assert!(got.len() <= SCROLLBACK_LIMIT);
    assert_eq!(SCROLLBACK_LIMIT, 2 * 1024 * 1024);
    assert!(
        got.len() > 1_900_000,
        "a late attach sees nearly all of the last 2 MB"
    );
    assert!(
        String::from_utf8_lossy(&got[got.len() - 10..]).contains("END"),
        "and it is the newest part"
    );
    assert!(!pty.is_running());
}

struct Host {
    child: Child,
}

impl Drop for Host {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn binary() -> &'static str {
    env!("CARGO_BIN_EXE_notchling-ptyhost")
}

fn run(args: &[&str]) -> String {
    let out = Command::new(binary())
        .args(args)
        .output()
        .expect("the binary runs");
    assert!(out.status.success(), "{out:?}");
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn start(socket: &Path, state: &Path) -> Host {
    let child = Command::new(binary())
        .args(["--socket", &socket.to_string_lossy()])
        .args(["--state", &state.to_string_lossy()])
        .stdout(Stdio::null())
        .spawn()
        .expect("the host starts");
    let host = Host { child };
    let deadline = Instant::now() + Duration::from_secs(10);
    while HostClient::connect(socket, Duration::from_secs(1)).is_err() {
        assert!(Instant::now() < deadline, "the host never listened");
        std::thread::sleep(Duration::from_millis(50));
    }
    host
}

#[test]
fn status_without_a_host_says_nothing_runs() {
    let scratch = Scratch::new();
    let socket = scratch.path("h.sock");
    let out = run(&["--socket", &socket.to_string_lossy(), "--status"]);
    assert_eq!(out.trim(), r#"{"running":0,"total":0,"host":false}"#);
}

#[test]
fn the_binary_serves_answers_status_and_leaves_on_sigterm() {
    let scratch = Scratch::new();
    let socket = scratch.path("h.sock");
    let state = scratch.path("state.json");
    let mut host = start(&socket, &state);

    let client = HostClient::connect(&socket, Duration::from_secs(3)).expect("connects");
    let hello = client
        .request("hello", Map::new(), Duration::from_secs(5))
        .expect("hello");
    assert_eq!(hello["version"], json!(1));
    assert_eq!(hello["pid"], json!(host.child.id()));
    client.close();

    let out = run(&["--socket", &socket.to_string_lossy(), "--status"]);
    assert_eq!(out.trim(), r#"{"running":0,"total":0}"#);

    // SAFETY: the pid is our own child, not yet waited for.
    unsafe { libc::kill(host.child.id() as i32, libc::SIGTERM) };
    let deadline = Instant::now() + Duration::from_secs(5);
    let status = loop {
        if let Some(status) = host.child.try_wait().expect("waits") {
            break status;
        }
        assert!(Instant::now() < deadline, "the host didn't leave");
        std::thread::sleep(Duration::from_millis(20));
    };
    assert!(status.success(), "{status:?}");
    assert!(!socket.exists(), "it removed its socket");
}

#[test]
fn record_resume_writes_a_record_for_the_next_start() {
    let scratch = Scratch::new();
    let state = scratch.path("state.json");
    let out = run(&[
        "--state",
        &state.to_string_lossy(),
        "--record-resume",
        "session-x",
        "/tmp/work",
    ]);
    assert_eq!(out.trim(), "recorded session-x for resume");
    let records = StateStore::new(&state).records();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].claude_session_id.as_deref(), Some("session-x"));
    assert_eq!(records[0].cwd, "/tmp/work");
    assert_eq!(records[0].pid, 0);
}

#[test]
fn a_second_binary_on_the_same_socket_leaves_with_an_error() {
    let scratch = Scratch::new();
    let socket = scratch.path("h.sock");
    let state = scratch.path("state.json");
    let _host = start(&socket, &state);
    let out = Command::new(binary())
        .args(["--socket", &socket.to_string_lossy()])
        .args(["--state", &state.to_string_lossy()])
        .output()
        .expect("runs");
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stdout).starts_with("hostd: cannot start:"));
}

#[test]
fn version_flag() {
    assert_eq!(run(&["--version"]).trim(), "ptyhost 0.1.0");
}
