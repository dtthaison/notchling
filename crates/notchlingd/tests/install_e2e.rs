//! The install ops through a real daemon over a temp data root: the switch is off, so the plan is
//! readable and `install.apply` / `install.remove` answer the dry run and write nothing. The
//! daemon sees a fixture root and a fake `HOME`, never the real `~/.claude`, `~/.codex` or
//! `~/.gemini`, and is stopped even when an assert fails.
#![cfg(unix)]

use notchling_local_api::{ops, Client, ClientKind, Tier};
use serde_json::json;
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
    let dir = PathBuf::from(format!("/tmp/nl-inst-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    Dir(dir)
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

/// Every file under `dir`, relative and sorted.
fn tree(dir: &Path) -> Vec<String> {
    fn walk(dir: &Path, base: &Path, out: &mut Vec<String>) {
        let Ok(read) = std::fs::read_dir(dir) else {
            return;
        };
        let mut entries: Vec<_> = read.map(|e| e.unwrap()).collect();
        entries.sort_by_key(|e| e.file_name());
        for e in entries {
            let rel = e
                .path()
                .strip_prefix(base)
                .unwrap()
                .to_string_lossy()
                .into_owned();
            // The socket, its lock and the audit log are the daemon's own.
            if !rel.starts_with("root/notchling") {
                out.push(rel);
            }
            if e.file_type().unwrap().is_dir() {
                walk(&e.path(), base, out);
            }
        }
    }
    let mut out = Vec::new();
    walk(dir, dir, &mut out);
    out
}

#[test]
fn a_real_daemon_plans_but_with_the_switch_off_writes_nothing() {
    let dir = scratch("off");
    let root = dir.0.join("root");
    std::fs::create_dir_all(root.join("claude")).unwrap();
    std::fs::create_dir_all(root.join("notchling")).unwrap();
    let settings = "{\n  \"model\": \"opus\",\n  \"hooks\": {\"Stop\": []}\n}\n";
    std::fs::write(root.join("claude/settings.json"), settings).unwrap();
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
    assert_eq!(app.tier(), Tier::App);
    let before = tree(&dir.0);

    let plan = app.call(ops::INSTALL_PLAN, json!({})).unwrap();
    assert_eq!(plan["on"], false);
    assert_eq!(plan["applied"], false);
    assert_eq!(plan["plan"]["hooks"]["state"], "present");
    assert_eq!(
        plan["plan"]["hooks"]["changes"].as_array().unwrap().len(),
        5
    );
    let file = plan["plan"]["hooks"]["file"].as_str().unwrap();
    assert_eq!(Path::new(file), root.join("claude/settings.json"));
    // The daemon's own program is what the hook would run.
    let change = plan["plan"]["hooks"]["changes"][0].as_str().unwrap();
    assert!(change.contains(" hook (timeout 300 s)"), "{change}");
    // The fake home holds no tool but Claude Code, so only its skills would be written.
    let skills = plan["plan"]["skills"].as_array().unwrap();
    assert!(skills
        .iter()
        .any(|s| s["tool"] == "claude" && s["action"] == "write"));
    assert!(skills
        .iter()
        .filter(|s| s["tool"] != "claude")
        .all(|s| s["action"] == "skip"));

    for op in [ops::INSTALL_APPLY, ops::INSTALL_REMOVE] {
        let reply = app.call(op, json!({})).unwrap();
        assert_eq!(reply["applied"], false, "{op}");
        assert_eq!(
            reply["message"],
            "install is off: the Swift app owns the hooks until the maintainer turns this on",
            "{op}"
        );
    }
    assert_eq!(tree(&dir.0), before, "nothing was written anywhere");
    assert_eq!(
        std::fs::read_to_string(root.join("claude/settings.json")).unwrap(),
        settings
    );

    // Each app-tier call is on the record.
    let audit = std::fs::read_to_string(root.join("notchling/audit.jsonl")).unwrap();
    assert!(audit.contains("\"op\":\"install.apply\""), "{audit}");
    assert!(audit.contains("\"op\":\"install.plan\""), "{audit}");
}
