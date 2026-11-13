#![cfg(unix)]

use notchling_engine::pages::{PageLibrary, RecordContext};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const OFF: &str = "the vault is off: the Swift app owns it until the maintainer turns this on";
const LINK: &str = "https://claude.ai/code/artifact/AbCdEfGhIjKlMnOpQrStUv";

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

fn make_root(dir: &Path) -> PathBuf {
    let root = dir.join("root");
    std::fs::create_dir_all(root.join("work")).unwrap();
    std::fs::write(root.join("work/a.html"), "<title>Cost Report</title>").unwrap();
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
                now: 1000.0,
                home: &root,
            },
        )
        .unwrap()
        .unwrap();
    }
    root
}

fn json_of(text: &str) -> Value {
    serde_json::from_str(text).unwrap_or_else(|e| panic!("not one JSON value ({e}): {text}"))
}

#[test]
fn status_json_is_the_daemons_status_and_the_text_is_unchanged() {
    let dir = scratch("json-status");
    let root = dir.0.join("root");
    std::fs::create_dir_all(root.join("claude")).unwrap();

    let (code, out, err) = run(&root, &["status", "--json"]);
    assert_eq!(code, 3);
    assert!(out.is_empty(), "{out}");
    assert!(err.contains("the daemon isn't running"), "{err}");

    let _daemon = start_daemon(&root);
    let (code, out, err) = run(&root, &["status", "--json"]);
    assert_eq!((code, err.as_str()), (0, ""));
    let v = json_of(&out);
    assert_eq!(v["version"], env!("CARGO_PKG_VERSION"));
    assert!(v["pid"].as_u64().unwrap() > 0);
    assert!(v["uptimeSecs"].is_u64(), "{v}");
    assert!(v["clients"]["cli"].as_u64().unwrap() >= 1, "{v}");
    assert!(v["clients"]["app"].is_u64() && v["clients"]["hook"].is_u64());
    assert_eq!(v["acting"], false);

    let (code, text, _) = run(&root, &["status"]);
    assert_eq!(code, 0);
    assert!(
        text.starts_with(&format!("notchlingd {}, pid ", env!("CARGO_PKG_VERSION")))
            && text.trim_end().ends_with("; acting off"),
        "{text}"
    );
    assert!(serde_json::from_str::<Value>(&text).is_err());
}

#[test]
fn pages_and_page_json_with_and_without_a_daemon() {
    let dir = scratch("json-pages");
    let root = make_root(&dir.0);

    let check = |root: &Path| {
        let (code, out, err) = run(root, &["pages", "--json"]);
        assert_eq!((code, err.as_str()), (0, ""));
        let v = json_of(&out);
        let pages = v["pages"].as_array().unwrap();
        assert_eq!(pages.len(), 2, "{v}");
        let cost = pages.iter().find(|p| p["title"] == "Cost Report").unwrap();
        assert_eq!(cost["id"], "AbCdEfGhIjKlMnOpQrStUv");
        assert_eq!(cost["account"], "me@example.com");
        assert!(cost["describe"]
            .as_str()
            .unwrap()
            .starts_with("Title: Cost Report"));

        let (code, out, _) = run(root, &["pages", "release", "--json"]);
        assert_eq!(code, 0);
        let v = json_of(&out);
        assert_eq!(v["pages"].as_array().unwrap().len(), 1);
        assert_eq!(v["pages"][0]["title"], "Release Plan");

        let (code, out, _) = run(root, &["page", LINK, "--json"]);
        assert_eq!(code, 0);
        assert_eq!(json_of(&out)["pages"][0]["title"], "Cost Report");

        let (code, out, err) = run(root, &["page", "no such page", "--json"]);
        assert_eq!(code, 1);
        assert_eq!(err, "No saved page matches \"no such page\".\n");
        assert_eq!(json_of(&out)["pages"], json!([]));
    };
    check(&root);
    let _daemon = start_daemon(&root);
    check(&root);

    let (_, table, _) = run(&root, &["pages"]);
    assert!(table.contains("AbCdEfGhIjKlMnOpQrStUv\tCost Report\tme@example.com"));
    assert!(serde_json::from_str::<Value>(&table).is_err());
}

#[test]
fn secrets_json_keeps_the_exit_codes_and_prints_no_value() {
    let dir = scratch("json-secrets");
    let root = dir.0.join("root");
    std::fs::create_dir_all(root.join("claude")).unwrap();

    let (code, out, err) = run(&root, &["secrets", "--json"]);
    assert_eq!(code, 2);
    assert!(out.is_empty(), "{out}");
    assert!(err.contains("Notchling isn't running"), "{err}");

    let _daemon = start_daemon(&root);
    let (code, out, err) = run(&root, &["secrets", "github", "--json"]);
    assert_eq!((code, err.trim()), (2, OFF));
    assert!(out.is_empty(), "{out}");
    assert!(!root.join("notchling/vault.json").exists());
}

#[test]
fn delegate_models_json_lists_the_codex_cache() {
    let dir = scratch("json-models");
    let root = dir.0.join("root");
    std::fs::create_dir_all(root.join("codex")).unwrap();
    std::fs::write(
        root.join("codex/models_cache.json"),
        r#"{"models":[
          {"slug":"gpt-a","display_name":"A","priority":1,"visibility":"list","default_reasoning_level":"medium",
           "supported_reasoning_levels":[{"effort":"low"},{"effort":"medium"}]},
          {"slug":"gpt-b","priority":2,"visibility":"list"}]}"#,
    )
    .unwrap();

    let (code, out, _) = run(&root, &["delegate-models", "codex", "--json"]);
    assert_eq!(code, 0);
    let v = json_of(&out);
    assert!(v.get("agy").is_none(), "only the tool asked for: {v}");
    let models = v["codex"]["models"].as_array().unwrap();
    assert_eq!(models.len(), 2, "{v}");
    assert_eq!(models[0]["slug"], "gpt-a");
    assert_eq!(models[0]["efforts"], json!(["low", "medium"]));
    assert_eq!(models[0]["defaultEffort"], "medium");
    assert_eq!(models[1]["slug"], "gpt-b");
    assert_eq!(models[1]["efforts"], json!([]));

    let (code, out, _) = run(&root, &["delegate-models", "--json"]);
    assert_eq!(code, 0);
    let v = json_of(&out);
    assert_eq!(v["codex"]["models"].as_array().unwrap().len(), 2);
    assert!(v["agy"]["available"].is_boolean() && v["agy"]["models"].is_array());

    let (code, text, _) = run(&root, &["delegate-models", "codex"]);
    assert_eq!(code, 0);
    assert!(text.starts_with("codex (--model <slug> --effort <level>):\n  gpt-a  efforts: low, medium (default medium)\n"));
}
