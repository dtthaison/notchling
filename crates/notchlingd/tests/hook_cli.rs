#![cfg(unix)]

mod common;

use notchling_local_api::{ops, Client, ClientError, ClientKind, ErrorCode};
use serde_json::json;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::time::{Duration, Instant};

struct Dir(PathBuf);

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn scratch(tag: &str) -> Dir {
    let dir = PathBuf::from(format!("/tmp/nl-hc-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    Dir(dir)
}

fn event(name: &str) -> Vec<u8> {
    std::fs::read(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/hook")
            .join(name),
    )
    .unwrap()
}

fn hook(dir: &Path, args: &[&str], stdin: &[u8]) -> (Output, Duration) {
    let started = Instant::now();
    let mut child = Command::new(env!("CARGO_BIN_EXE_notchling"))
        .arg("hook")
        .args(args)
        .env("NOTCHLING_DATA_ROOT", dir.join("root"))
        .env("HOME", dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(stdin).unwrap();
    let out = child.wait_with_output().unwrap();
    (out, started.elapsed())
}

fn passed(out: &Output) -> bool {
    out.status.code() == Some(0) && out.stdout.is_empty()
}

#[test]
fn with_no_daemon_it_passes_in_under_200_ms() {
    let dir = scratch("none");
    let socket = dir.0.join("missing.sock");
    let args = ["--socket", socket.to_str().unwrap()];
    let mut times = Vec::new();
    for _ in 0..5 {
        let (out, took) = hook(&dir.0, &args, &event("permission-request.json"));
        assert!(passed(&out), "{out:?}");
        times.push(took);
    }
    times.sort();
    let median = times[times.len() / 2];
    println!("notchling hook, no daemon: {times:?}, median {median:?}");
    assert!(median < Duration::from_millis(200), "{times:?}");
}

#[test]
fn odd_input_or_arguments_still_pass_with_exit_0() {
    let dir = scratch("odd");
    let socket = dir.0.join("missing.sock");
    for (args, stdin) in [
        (
            vec!["--socket", socket.to_str().unwrap()],
            b"not json".to_vec(),
        ),
        (vec!["--socket", socket.to_str().unwrap()], Vec::new()),
        (vec!["--bogus"], event("permission-request.json")),
        (vec!["--socket"], event("permission-request.json")),
    ] {
        let (out, _) = hook(&dir.0, &args, &stdin);
        assert!(passed(&out), "{args:?}: {out:?}");
        assert!(out.stderr.is_empty(), "{args:?}: a hook says nothing");
    }
}

struct Daemon(Child);

impl Drop for Daemon {
    fn drop(&mut self) {
        // SAFETY: signalling the daemon this test started.
        unsafe { libc::kill(self.0.id() as libc::pid_t, libc::SIGTERM) };
        let deadline = Instant::now() + Duration::from_secs(5);
        while matches!(self.0.try_wait(), Ok(None)) && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(50));
        }
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
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

#[test]
fn a_real_daemon_with_acting_off_lets_the_hook_pass_and_refuses_every_acting_op() {
    let dir = scratch("off");
    let root = dir.0.join("root");
    copy_tree(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../app/fixtures/notch"),
        &root,
    );
    let socket = root.join("notchling/notchlingd.sock");
    let me = std::env::current_exe().unwrap();
    let _daemon = Daemon(
        Command::new(env!("CARGO_BIN_EXE_notchling"))
            .arg("daemon")
            .env("NOTCHLING_DATA_ROOT", &root)
            .env("HOME", &dir.0)
            .env(notchling_local_api::APP_TIER_PATHS_ENV, &me)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    let deadline = Instant::now() + Duration::from_secs(10);
    while std::os::unix::net::UnixStream::connect(&socket).is_err() {
        assert!(
            Instant::now() < deadline,
            "no daemon on {}",
            socket.display()
        );
        std::thread::sleep(Duration::from_millis(50));
    }
    let mut app = Client::connect(&socket, ClientKind::App, "0.1.0").unwrap();
    assert_eq!(app.tier(), notchling_local_api::Tier::App);
    assert_eq!(
        app.call(ops::SETTINGS_GET_ACTING, json!({})).unwrap(),
        json!({"acting": false})
    );
    let status = app.call(ops::DAEMON_STATUS, json!({})).unwrap();
    assert_eq!(status["acting"], json!(false), "{status}");

    let (out, _) = hook(&dir.0, &[], &event("permission-request.json"));
    assert!(passed(&out), "{out:?}");
    let (out, _) = hook(&dir.0, &[], &event("ask-user-question.json"));
    assert!(passed(&out), "{out:?}");

    for op in [
        ops::SESSION_REPLY,
        ops::SESSION_ANSWER,
        ops::SESSION_PERMISSION,
        ops::SESSION_INTERRUPT,
        ops::SESSION_STOP,
        ops::SESSION_REMOVE,
        ops::SESSION_TAKE_BACK,
        ops::SESSION_NEW,
        ops::FOLDER_TRUST,
        ops::SESSION_MOVE,
        ops::SESSION_COMMAND,
        ops::CODEX_SEND,
        ops::CODEX_STOP,
        ops::CODEX_DELETE,
    ] {
        match app.call(op, json!({"id": "sess-notch", "text": "hi"})) {
            Err(ClientError::Api(e)) => {
                assert_eq!(e.code, ErrorCode::Forbidden, "{op}");
                assert_eq!(
                    e.message,
                    "acting is off: the Swift app owns sessions until the maintainer turns this on"
                );
            }
            other => panic!("{op}: {other:?}"),
        }
    }
    let lines = common::audit::wait_until(&root, |all| {
        all.iter().any(|l| l["kind"] == "session.reply")
            && all.iter().filter(|l| l["kind"] == "api.refused").count() >= 12
    });
    let refused: Vec<&str> = lines
        .iter()
        .filter(|l| l["kind"] == "api.refused")
        .filter_map(|l| l["f"]["op"].as_str())
        .collect();
    assert!(refused.contains(&"codex.send"), "{refused:?}");
    assert!(refused.contains(&"session.stop"), "{refused:?}");
    assert!(lines
        .iter()
        .filter(|l| l["kind"] == "api.refused")
        .all(|l| l["f"]["why"] == "forbidden"));
    let reply = lines.iter().find(|l| l["kind"] == "session.reply").unwrap();
    assert_eq!(reply["f"]["why"], "acting_off", "{reply}");
    assert_eq!(reply["outcome"], "failed");
    assert!(!root.join("notchling/notchlingd-settings.json").exists());
}

fn start_daemon(dir: &Path, root: &Path) -> Daemon {
    let socket = root.join("notchling/notchlingd.sock");
    let me = std::env::current_exe().unwrap();
    let daemon = Daemon(
        Command::new(env!("CARGO_BIN_EXE_notchling"))
            .arg("daemon")
            .env("NOTCHLING_DATA_ROOT", root)
            .env("HOME", dir)
            .env(notchling_local_api::APP_TIER_PATHS_ENV, &me)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    let deadline = Instant::now() + Duration::from_secs(10);
    while std::os::unix::net::UnixStream::connect(&socket).is_err() {
        assert!(
            Instant::now() < deadline,
            "no daemon on {}",
            socket.display()
        );
        std::thread::sleep(Duration::from_millis(50));
    }
    daemon
}

#[test]
fn a_settings_file_that_says_on_is_ignored_at_every_start() {
    let dir = scratch("file-on");
    let root = dir.0.join("root");
    copy_tree(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../app/fixtures/notch"),
        &root,
    );
    let file = root.join("notchling/notchlingd-settings.json");
    std::fs::write(&file, br#"{"acting": true}"#).unwrap();
    let socket = root.join("notchling/notchlingd.sock");
    for run in 0..2 {
        let daemon = start_daemon(&dir.0, &root);
        let mut app = Client::connect(&socket, ClientKind::App, "0.1.0").unwrap();
        assert_eq!(
            app.call(ops::SETTINGS_GET_ACTING, json!({})).unwrap(),
            json!({"acting": false}),
            "run {run}: the file says on, the daemon starts off"
        );
        assert_eq!(
            app.call(ops::SETTINGS_SET_ACTING, json!({"on": true}))
                .unwrap(),
            json!({"acting": true})
        );
        assert_eq!(app.status().unwrap().acting, Some(true));
        drop(app);
        drop(daemon);
    }
    let ignored = common::audit::of_kind(&root, "switch.changed")
        .into_iter()
        .filter(|line| line["f"]["cause"] == "file_said_on_ignored")
        .count();
    assert_eq!(ignored, 2);
    assert_eq!(std::fs::read(&file).unwrap(), br#"{"acting": true}"#);
}
