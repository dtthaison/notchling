//! The pages and search ops through a real daemon over a temp data root, and `notchling page` /
//! `notchling pages` against it and without it. It never sees the real folders: every run sets
//! `NOTCHLING_DATA_ROOT` and a fake `HOME`, and a guard stops every daemon it starts.
#![cfg(unix)]

use notchling_engine::pages::{PageLibrary, RecordContext};
use notchling_local_api::{ops, Client, ClientKind};
use serde_json::json;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

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

fn now() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

const LINK: &str = "https://claude.ai/code/artifact/AbCdEfGhIjKlMnOpQrStUv";

/// A data root with two saved pages and one recent Claude Code transcript.
fn make_root(dir: &Path) -> PathBuf {
    let root = dir.join("root");
    std::fs::create_dir_all(root.join("work")).unwrap();
    std::fs::write(
        root.join("work/a.html"),
        "<title>Cost Report</title><p>numbers</p>",
    )
    .unwrap();
    std::fs::write(root.join("work/b.html"), "<title>Release Plan</title>").unwrap();
    let lib = PageLibrary::writable(root.join("notchling/pages"));
    for (file, url, account) in [
        ("a.html", LINK, "me@example.com"),
        (
            "b.html",
            "https://claude.ai/artifact/ZZZZZZZZZZZZZZZZZZZZZZ",
            "work@example.com",
        ),
    ] {
        let event = json!({
            "tool_name": "Artifact",
            "cwd": root.join("work"),
            "tool_input": {"file_path": file},
            "tool_response": format!("Published {url}"),
        });
        lib.record(
            &event,
            &RecordContext {
                account,
                now: now(),
                home: &root,
            },
        )
        .unwrap()
        .unwrap();
    }
    let transcript = root.join("claude/projects/-w-app/sess1.jsonl");
    std::fs::create_dir_all(transcript.parent().unwrap()).unwrap();
    std::fs::write(
        &transcript,
        concat!(
            r#"{"type":"user","cwd":"/w/app","message":{"role":"user","content":"investigate the numbers drift"}}"#,
            "\n",
            r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"The numbers drift came from rounding."}]}}"#,
            "\n"
        ),
    )
    .unwrap();
    root
}

#[test]
fn the_ops_and_the_commands_work_with_and_without_a_daemon() {
    let dir = scratch("pages-e2e");
    let root = make_root(&dir.0);
    let socket = root.join("notchling/notchlingd.sock");
    let list_file = root.join("notchling/pages/pages.json");
    let list_before = std::fs::read(&list_file).unwrap();

    let mut cmd = notchling(&root);
    cmd.arg("daemon")
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let daemon = Daemon(cmd.spawn().unwrap());
    wait_for_socket(&socket);

    let mut client = Client::connect(&socket, ClientKind::Cli, "0.1.0").unwrap();
    // Acting is off, and reads work.
    assert_eq!(client.status().unwrap().acting, Some(false));
    let list = client.call(ops::PAGES_LIST, json!({})).unwrap();
    assert_eq!(list["pages"].as_array().unwrap().len(), 2, "{list}");
    let found = client
        .call(ops::PAGES_FIND, json!({"query": LINK}))
        .unwrap();
    assert_eq!(found["pages"][0]["title"], "Cost Report");
    let path = PathBuf::from(found["pages"][0]["path"].as_str().unwrap());
    assert!(path.is_file() && std::fs::read_to_string(&path).unwrap().contains("numbers"));
    let search = client
        .call(ops::SEARCH_QUERY, json!({"text": "numbers"}))
        .unwrap();
    // Pages match on title, project, account and link, not on what is inside.
    assert!(search["pages"].as_array().unwrap().is_empty(), "{search}");
    assert_eq!(search["sessions"][0]["key"], "claude:sess1", "{search}");
    assert!(search["sessions"][0]["snippet"]
        .as_str()
        .unwrap()
        .contains("numbers"));
    let by_title = client
        .call(ops::SEARCH_QUERY, json!({"text": "cost report"}))
        .unwrap();
    assert_eq!(by_title["pages"][0]["title"], "Cost Report", "{by_title}");
    assert!(client.call(ops::PAGES_FIND, json!({})).is_err());

    // The commands, through the daemon.
    let page = notchling(&root).args(["page", LINK]).output().unwrap();
    let text = String::from_utf8_lossy(&page.stdout).into_owned();
    assert!(
        page.status.success(),
        "{}",
        String::from_utf8_lossy(&page.stderr)
    );
    assert!(
        text.starts_with(&format!(
            "Title: Cost Report\nLink: {LINK}\nPublished by: me@example.com\n"
        )),
        "{text}"
    );
    let pages = notchling(&root).args(["pages"]).output().unwrap();
    let table = String::from_utf8_lossy(&pages.stdout).into_owned();
    assert_eq!(table.lines().count(), 2, "{table}");
    assert!(table.contains("AbCdEfGhIjKlMnOpQrStUv\tCost Report\tme@example.com"));
    let none = notchling(&root)
        .args(["page", "no such page"])
        .output()
        .unwrap();
    assert_eq!(none.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&none.stderr).contains("No saved page matches \"no such page\".")
    );

    // The daemon read; it wrote the pages folder not at all, and its cache is its own.
    assert_eq!(std::fs::read(&list_file).unwrap(), list_before);
    assert!(root.join("notchling/daemon/search-index.json").exists());
    assert!(!root.join("notchling/search-index.json").exists());

    // Without a daemon the same commands read the library themselves, with the same text.
    drop(client);
    drop(daemon);
    let deadline = Instant::now() + Duration::from_secs(10);
    while socket.exists() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(50));
    }
    let direct = notchling(&root).args(["page", LINK]).output().unwrap();
    assert!(direct.status.success());
    assert_eq!(String::from_utf8_lossy(&direct.stdout), text);
}

#[test]
fn a_page_without_a_link_shows_a_dash_for_its_id() {
    let dir = scratch("pages-e2e-nolink");
    let root = dir.0.join("root");
    std::fs::create_dir_all(root.join("work")).unwrap();
    std::fs::write(root.join("work/n.md"), "# Notes").unwrap();
    let lib = PageLibrary::writable(root.join("notchling/pages"));
    let event = json!({
        "tool_name": "Artifact", "cwd": root.join("work"),
        "tool_input": {"file_path": "n.md"}, "tool_response": "ok",
    });
    lib.record(
        &event,
        &RecordContext {
            account: "",
            now: 1.0,
            home: &root,
        },
    )
    .unwrap()
    .unwrap();
    let out = notchling(&root).args(["pages"]).output().unwrap();
    assert_eq!(String::from_utf8_lossy(&out.stdout), "-\tNotes\t-\n");
}
