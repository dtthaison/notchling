#![cfg(unix)]

use notchling_local_api::{ops, topics, Client, ClientError, ClientKind, ErrorCode};
use serde_json::json;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const OFF: &str = "the vault is off: the Swift app owns it until the maintainer turns this on";

struct Dir(PathBuf);

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn scratch(tag: &str) -> Dir {
    let dir = PathBuf::from(format!("/tmp/nl-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
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
        .env_remove(notchling_local_api::APP_TIER_PATHS_ENV)
        .stdin(Stdio::null());
    c
}

fn wait_for_socket(socket: &Path) {
    let deadline = Instant::now() + Duration::from_secs(30);
    while Instant::now() < deadline {
        if std::os::unix::net::UnixStream::connect(socket).is_ok() {
            return;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    panic!("no daemon on {}", socket.display());
}

fn run(root: &Path, args: &[&str]) -> (i32, String, String) {
    let out = notchling(root).args(args).output().unwrap();
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

#[test]
fn with_the_switch_off_every_vault_path_says_so() {
    let dir = scratch("vault-off");
    let root = dir.0.join("root");
    std::fs::create_dir_all(root.join("claude")).unwrap();
    let mut daemon = notchling(&root);
    daemon
        .arg("daemon")
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let _daemon = Daemon(daemon.spawn().unwrap());
    let socket = root.join("notchling/notchlingd.sock");
    wait_for_socket(&socket);

    let mut client = Client::connect(&socket, ClientKind::Cli, "0.1.0").unwrap();
    for op in [ops::VAULT_FIND, ops::VAULT_USE, ops::VAULT_REQUEST] {
        match client.call(op, json!({"names": ["gh"], "name": "gh"})) {
            Err(ClientError::Api(e)) => {
                assert_eq!(
                    (e.code, e.message.as_str()),
                    (ErrorCode::Forbidden, OFF),
                    "{op}"
                )
            }
            other => panic!("{op}: {other:?}"),
        }
    }
    match client.call(
        ops::VAULT_APPROVAL_ANSWER,
        json!({"id": "x", "decision": "once"}),
    ) {
        Err(ClientError::Api(e)) => assert_eq!(e.code, ErrorCode::Forbidden),
        other => panic!("{other:?}"),
    }
    match client.subscribe(&[topics::VAULT]) {
        Err(ClientError::Api(e)) => assert_eq!(e.code, ErrorCode::Forbidden),
        other => panic!("{other:?}"),
    }

    let (code, _, err) = run(&root, &["secrets", "github"]);
    assert_eq!((code, err.trim()), (2, OFF));
    let marker = dir.0.join("ran");
    let command = format!("touch '{}'", marker.display());
    let (code, _, err) = run(&root, &["secret", "gh", "--", &command]);
    assert_eq!((code, err.trim()), (2, OFF));
    assert!(!marker.exists(), "the command never ran");
    let (code, _, err) = run(&root, &["secret-request", "gh", "--kind", "token"]);
    assert_eq!((code, err.trim()), (2, OFF));

    assert!(!root.join("notchling/daemon/vault.json").exists());
    assert!(!root.join("notchling/vault.json").exists());
    let audit = std::fs::read_to_string(root.join("notchling/audit.jsonl")).unwrap();
    assert!(audit.contains("\"op\":\"vault.use\"") && audit.contains("\"result\":\"off\""));
}

#[test]
fn with_no_daemon_the_commands_say_notchling_isnt_running() {
    let dir = scratch("vault-none");
    let root = dir.0.join("root");
    std::fs::create_dir_all(&root).unwrap();
    let (code, _, err) = run(&root, &["secrets"]);
    assert_eq!(code, 2);
    assert!(err.contains("Notchling isn't running"), "{err}");
    let (code, _, err) = run(&root, &["secret", "gh"]);
    assert_eq!(code, 2);
    assert!(err.starts_with("Usage: notchling secret <name>"), "{err}");
    let (code, _, err) = run(&root, &["secret-request", "gh", "--expires", "soon"]);
    assert_eq!(code, 2);
    assert!(err.contains("YYYY-MM-DD"), "{err}");
}
