#![cfg(unix)]

use notchling_local_api::{ops, Client, ClientKind};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

struct Dir(PathBuf);

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn scratch(tag: &str) -> Dir {
    let dir = PathBuf::from(format!("/tmp/nl-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("notchling")).unwrap();
    Dir(dir)
}

struct Daemon(Child);

impl Drop for Daemon {
    fn drop(&mut self) {
        if matches!(self.0.try_wait(), Ok(None)) {
            // SAFETY: signalling our own child by its pid.
            unsafe { libc::kill(self.0.id() as libc::pid_t, libc::SIGTERM) };
            let deadline = Instant::now() + Duration::from_secs(5);
            while matches!(self.0.try_wait(), Ok(None)) && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(50));
            }
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
}

fn notchling(root: &Path) -> Command {
    let mut c = Command::new(env!("CARGO_BIN_EXE_notchling"));
    c.env("NOTCHLING_DATA_ROOT", root)
        .env("HOME", root)
        .env("PATH", "/usr/bin:/bin")
        .env_remove(notchling_local_api::APP_TIER_PATHS_ENV)
        .env_remove("CODEX_HOME")
        .stdin(Stdio::null());
    c
}

fn start_daemon(root: &Path) -> Daemon {
    let mut cmd = notchling(root);
    cmd.env(
        notchling_local_api::APP_TIER_PATHS_ENV,
        std::env::current_exe().unwrap(),
    )
    .arg("daemon")
    .stdout(Stdio::null())
    .stderr(Stdio::null());
    let daemon = Daemon(cmd.spawn().unwrap());
    let socket = root.join("notchling/notchlingd.sock");
    let deadline = Instant::now() + Duration::from_secs(30);
    while Instant::now() < deadline {
        if std::os::unix::net::UnixStream::connect(&socket).is_ok() {
            return daemon;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    panic!("no daemon on {}", socket.display());
}

fn watch(root: &Path, args: &[&str]) -> Child {
    notchling(root)
        .arg("watch")
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap()
}

fn flip_until_exit(root: &Path, child: &mut Child) -> i32 {
    let socket = root.join("notchling/notchlingd.sock");
    let mut app = Client::connect(&socket, ClientKind::App, "0.1.0").unwrap();
    let deadline = Instant::now() + Duration::from_secs(30);
    let mut on = true;
    while Instant::now() < deadline {
        if let Some(status) = child.try_wait().unwrap() {
            return status.code().unwrap_or(-1);
        }
        app.call(
            ops::SETTINGS_SET_SWITCH,
            json!({"switch": "vault", "on": on}),
        )
        .unwrap();
        on = !on;
        std::thread::sleep(Duration::from_millis(200));
    }
    let _ = child.kill();
    panic!("watch never ended");
}

fn output_of(child: Child) -> (String, String) {
    let out = child.wait_with_output().unwrap();
    (
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

#[test]
fn watch_on_the_switch_topic_exits_zero_after_one_switch_event() {
    let dir = scratch("watch-switch");
    let root = dir.0.as_path();
    let _daemon = start_daemon(root);
    let mut child = watch(
        root,
        &[
            "--topic",
            "settings.switch",
            "--count",
            "1",
            "--timeout",
            "25",
        ],
    );
    assert_eq!(flip_until_exit(root, &mut child), 0);
    let (out, err) = output_of(child);
    assert!(err.is_empty(), "{err}");
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines.len(), 1, "{out}");
    let event: Value = serde_json::from_str(lines[0]).unwrap();
    assert_eq!(event["event"], "settings.switch");
    assert_eq!(event["data"]["switch"], "vault");
}

#[test]
fn a_shell_that_names_no_topic_still_follows_the_public_ones() {
    let dir = scratch("watch-default");
    let root = dir.0.as_path();
    let _daemon = start_daemon(root);
    let mut child = watch(root, &["--count", "1", "--timeout", "25"]);
    assert_eq!(flip_until_exit(root, &mut child), 0);
    let (out, _) = output_of(child);
    let event: Value = serde_json::from_str(out.lines().next().unwrap()).unwrap();
    assert_eq!(event["event"], "settings.switch");
}

#[test]
fn a_shell_cannot_follow_a_scoped_topic() {
    let dir = scratch("watch-scoped");
    let root = dir.0.as_path();
    let _daemon = start_daemon(root);
    for topic in ["mine.consents", "mine.asks"] {
        let started = Instant::now();
        let out = notchling(root)
            .args(["watch", "--topic", topic, "--timeout", "20", "--json"])
            .output()
            .unwrap();
        assert!(started.elapsed() < Duration::from_secs(10), "{topic}");
        assert_eq!(out.status.code(), Some(4), "{topic}");
        let error: Value = serde_json::from_slice(&out.stdout).unwrap();
        assert_eq!(error["error"]["reason"], "no_session", "{topic}: {error}");
    }
}

#[test]
fn watch_with_a_timeout_and_no_event_exits_124() {
    let dir = scratch("watch-timeout");
    let root = dir.0.as_path();
    let _daemon = start_daemon(root);
    let started = Instant::now();
    let out = notchling(root)
        .args(["watch", "--topic", "settings.switch", "--timeout", "1"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(124));
    assert!(out.stdout.is_empty());
    assert!(started.elapsed() < Duration::from_secs(10));
}

#[test]
fn watch_rejects_an_unknown_topic_and_a_zero_count() {
    let dir = scratch("watch-usage");
    let root = dir.0.as_path();
    for args in [
        ["watch", "--topic", "mine.sessions"],
        ["watch", "--count", "0"],
    ] {
        let out = notchling(root).args(args).output().unwrap();
        assert_eq!(out.status.code(), Some(2), "{args:?}");
    }
}

#[test]
fn watch_rejects_an_empty_until_and_one_no_topic_can_reach() {
    let dir = scratch("watch-until-usage");
    let root = dir.0.as_path();
    let empty: &[&str] = &["watch", "--until", ""];
    let public: &[&str] = &["watch", "--topic", "settings.switch", "--until", "applied"];
    for args in [empty, public] {
        let out = notchling(root).args(args).output().unwrap();
        assert_eq!(out.status.code(), Some(2), "{args:?}");
    }
}

#[test]
fn a_shell_that_waits_for_a_state_is_refused_instead_of_hanging() {
    let dir = scratch("watch-until-shell");
    let root = dir.0.as_path();
    let _daemon = start_daemon(root);
    let started = Instant::now();
    let out = notchling(root)
        .args(["watch", "--until", "applied", "--timeout", "6", "--json"])
        .output()
        .unwrap();
    assert!(started.elapsed() < Duration::from_secs(5));
    assert_eq!(out.status.code(), Some(4));
    let error: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(error["error"]["reason"], "no_session", "{error}");
}

#[test]
fn a_huge_timeout_is_a_usage_error_and_never_a_panic() {
    let dir = scratch("watch-huge-timeout");
    let root = dir.0.as_path();
    let _daemon = start_daemon(root);
    let child = watch(
        root,
        &[
            "--topic",
            "settings.switch",
            "--count",
            "1",
            "--timeout",
            "18446744073709551615",
        ],
    );
    let out = child.wait_with_output().unwrap();
    assert_eq!(out.status.code(), Some(2));
    assert!(out.stdout.is_empty());
}
