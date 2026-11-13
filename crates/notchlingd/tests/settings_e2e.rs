#![cfg(unix)]

use notchling_engine::settings::Settings;
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
    cmd.arg("daemon")
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

fn run(root: &Path, args: &[&str]) -> (i32, String, String) {
    let out = notchling(root).args(args).output().unwrap();
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

fn json_of(text: &str) -> Value {
    serde_json::from_str(text).unwrap_or_else(|e| panic!("not one JSON value ({e}): {text}"))
}

fn stored(root: &Path, key: &str) -> Option<Value> {
    Settings::open(&root.join("notchling/settings.json"))
        .unwrap()
        .raw(key)
        .cloned()
}

#[test]
fn an_agent_reads_and_changes_settings_through_the_command_line() {
    let dir = scratch("settings-e2e");
    let root = dir.0.as_path();
    let _daemon = start_daemon(root);

    let (code, out, err) = run(root, &["settings", "list"]);
    assert_eq!(code, 0, "{err}");
    assert!(out.starts_with("You may change these:\n"), "{out}");
    assert!(
        out.contains(
            "  chatHistory = 4mb   Conversation history to load (one of 2mb, 4mb, 8mb, 16mb)"
        ),
        "{out}"
    );
    assert!(
        out.contains("These ask the user first, in Notchling's Inbox:"),
        "{out}"
    );
    assert!(
        out.contains("  switch.acting = off   Act on Sessions"),
        "{out}"
    );

    let (code, out, _) = run(root, &["settings", "list", "--json"]);
    assert_eq!(code, 0);
    let list = json_of(&out);
    let names: Vec<&str> = list["settings"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|e| e["key"].as_str())
        .collect();
    assert!(names.contains(&"switch.vault") && names.contains(&"chatHistory"));

    let (code, out, _) = run(root, &["settings", "set", "chatHistory", "8MB"]);
    assert_eq!((code, out.as_str()), (0, "Set chatHistory to 8mb.\n"));
    assert_eq!(stored(root, "chatHistory"), Some(Value::from("8mb")));
    let (code, out, _) = run(root, &["settings", "get", "chatHistory", "--json"]);
    assert_eq!(code, 0);
    assert_eq!(json_of(&out)["value"], "8mb");
    let (code, out, _) = run(root, &["settings", "get", "chatHistory"]);
    assert_eq!(code, 0);
    assert!(out.starts_with("chatHistory = 8mb\n"), "{out}");
    assert!(out.contains("Agents may change it."), "{out}");

    let (code, out, err) = run(root, &["settings", "set", "switch.acting", "on"]);
    assert_eq!(code, 1);
    assert!(out.is_empty());
    assert!(
        err.contains("is a switch: only the user turns it on or off"),
        "{err}"
    );

    let (code, _, err) = run(root, &["settings", "set", "codexNetwork", "true"]);
    assert_eq!(code, 1);
    assert!(err.contains("can't be changed by an agent"), "{err}");

    let (code, _, err) = run(root, &["settings", "set", "showPet", "maybe"]);
    assert_eq!((code, err.as_str()), (1, "showPet takes true or false.\n"));

    let (code, _, err) = run(root, &["settings", "get", "theme"]);
    assert_eq!(code, 1);
    assert!(err.starts_with("No setting named theme."), "{err}");

    let (code, _, err) = run(root, &["settings", "set", "limitMode", "off"]);
    assert_eq!(code, 3, "an ask with no app open to answer it: {err}");
    assert!(err.contains("the app isn't open"), "{err}");
    assert_eq!(stored(root, "limitMode"), None);

    let (code, _, err) = run(root, &["settings", "set", "chatHistory"]);
    assert_eq!(code, 2);
    assert!(
        err.starts_with("settings set needs a key and a value."),
        "{err}"
    );

    let audit = std::fs::read_to_string(root.join("notchling/audit.jsonl")).unwrap();
    let results: Vec<String> = audit
        .lines()
        .map(json_of)
        .filter(|e| e["op"] == "settings.set")
        .map(|e| e["result"].as_str().unwrap_or_default().to_string())
        .collect();
    assert_eq!(
        results,
        ["set", "refused", "refused", "bad_value", "no_app"]
    );
}

#[test]
fn with_no_daemon_the_command_says_notchling_isnt_running() {
    let dir = scratch("settings-e2e-off");
    let (code, out, err) = run(&dir.0, &["settings", "list"]);
    assert_eq!(code, 2);
    assert!(out.is_empty());
    assert_eq!(
        err,
        "Notchling isn't running. Ask the user to open it, then try again.\n"
    );
}
