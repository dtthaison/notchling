#![cfg(unix)]

use serde_json::Value;
use std::io::Write;
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
        .env_remove("NOTCHLING_CLUSTER_LOOPBACK")
        .env_remove("CODEX_HOME");
    c
}

fn start_daemon(root: &Path) -> Daemon {
    let mut cmd = notchling(root);
    cmd.arg("daemon")
        .stdin(Stdio::null())
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

fn run(root: &Path, args: &[&str], input: &str) -> (i32, String, String) {
    let mut child = notchling(root)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(input.as_bytes());
    }
    let out = child.wait_with_output().unwrap();
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

#[test]
fn a_fixture_daemon_is_a_node_that_links_nowhere() {
    let dir = scratch("cluster-e2e");
    let root = dir.0.as_path();
    let _daemon = start_daemon(root);

    let (code, out, err) = run(root, &["cluster", "status", "--json"], "");
    assert_eq!(code, 0, "{err}");
    let status: Value = serde_json::from_str(&out).unwrap();
    assert_eq!(status["cluster"], Value::Null, "{status}");
    let me = status["me"].as_str().unwrap();
    assert_eq!(me.len(), 16, "{status}");
    assert!(me.chars().all(|c| c.is_ascii_hexdigit()));
    let identity = root.join("notchling/cluster/node");
    assert!(identity.join("node.pem").is_file());
    assert!(identity.join("node.key").is_file());

    let (code, _, err) = run(root, &["cluster", "join", "nlj1.secret"], "");
    assert_eq!(code, 2);
    assert!(err.contains("pass the token on stdin"), "{err}");
    assert!(!err.contains("nlj1.secret"), "{err}");

    let (code, _, err) = run(root, &["cluster", "token"], "");
    assert_eq!(code, 1, "{err}");
    assert!(err.contains("no cluster"), "{err}");

    let (code, _, err) = run(
        root,
        &["cluster", "join", "--token-stdin"],
        "nlj1.not-a-token\n",
    );
    assert_eq!(code, 1, "{err}");

    let (code, out, _) = run(root, &["cluster", "status", "--json"], "");
    assert_eq!(code, 0);
    let status: Value = serde_json::from_str(&out).unwrap();
    assert_eq!(status["listening"], Value::Null, "{status}");
    assert_eq!(status["link"], false, "a bad token leaves the switch off");
    let audit = std::fs::read_to_string(root.join("notchling/audit.jsonl")).unwrap_or_default();
    assert!(!audit.contains("nlj1."), "no token in the audit log");
}
