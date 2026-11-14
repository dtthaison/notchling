use crate::pages_ops::*;
use notchling_engine::pages::{PageLibrary, RecordContext};
use notchling_engine::search::index::TitledSession;
use notchling_engine::sessions::Roots;
use notchling_local_api::{ops, ErrorCode};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

struct Dir(PathBuf);

impl Dir {
    fn new(tag: &str) -> Dir {
        let dir = std::env::temp_dir().join(format!("nl-pagesops-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Dir(dir)
    }

    fn path(&self, rel: &str) -> PathBuf {
        self.0.join(rel)
    }

    fn write(&self, rel: &str, text: &str) -> PathBuf {
        let path = self.path(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, text).unwrap();
        path
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn now() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

fn age(path: &Path, ago: f64) {
    let file = std::fs::OpenOptions::new().write(true).open(path).unwrap();
    file.set_modified(UNIX_EPOCH + Duration::from_secs_f64(now() - ago))
        .unwrap();
}

fn setup(dir: &Dir) -> PagesSetup {
    PagesSetup {
        pages_dir: dir.path("data/pages"),
        writable: false,
        roots: Roots {
            main_dir: dir.path("claude"),
            lanes_file: dir.path("data/lanes.json"),
        },
        codex_sessions: dir.path("codex/sessions"),
        cache: Some(dir.path("data/daemon/search.db")),
    }
}

fn save_page(dir: &Dir, file: &str, html: &str, url: &str, account: &str, at: f64) {
    dir.write(&format!("work/{file}"), html);
    let event = json!({
        "tool_name": "Artifact",
        "cwd": dir.path("work"),
        "tool_input": {"file_path": file},
        "tool_response": format!("Published {url}"),
    });
    PageLibrary::writable(dir.path("data/pages"))
        .record(
            &event,
            &RecordContext {
                account,
                now: at,
                home: &dir.0,
            },
        )
        .unwrap()
        .unwrap();
}

fn call(ops: &PagesOps, op: &str, args: Value) -> Value {
    ops.call(op, &args).unwrap()
}

fn titles(value: &Value, key: &str) -> Vec<String> {
    value[key]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["title"].as_str().unwrap().to_string())
        .collect()
}

const A: &str = "https://claude.ai/artifact/AbCdEfGhIjKlMnOpQrStUv";
const B: &str = "https://claude.ai/code/artifact/ZZZZZZZZZZZZZZZZZZZZZZ";

#[test]
fn the_ops_are_agent_tier_reads() {
    let dir = Dir::new("spec");
    let ops_ = PagesOps::new(setup(&dir));
    for op in [ops::SEARCH_QUERY, ops::PAGES_LIST, ops::PAGES_FIND] {
        assert_eq!(
            ops_.spec(op),
            Some(notchling_local_api::OpSpec::AGENT),
            "{op}"
        );
    }
    assert_eq!(ops_.spec(ops::SESSIONS_LIST), None);
    assert_eq!(ops_.spec("pages.remove"), None);
}

#[test]
fn pages_list_and_find_return_the_library() {
    let dir = Dir::new("list");
    save_page(
        &dir,
        "a.html",
        "<title>Cost Report</title>",
        A,
        "me@example.com",
        100.0,
    );
    save_page(
        &dir,
        "b.html",
        "<title>Release Plan</title>",
        B,
        "work@example.com",
        200.0,
    );
    let ops_ = PagesOps::new(setup(&dir));

    let list = call(&ops_, ops::PAGES_LIST, json!({}));
    assert_eq!(list["truncated"], false);
    assert_eq!(
        titles(&list, "pages"),
        ["Release Plan", "Cost Report"],
        "newest first"
    );
    let first = &list["pages"][0];
    assert_eq!(first["id"], "ZZZZZZZZZZZZZZZZZZZZZZ");
    assert_eq!(first["url"], B);
    assert_eq!(first["account"], "work@example.com");
    assert_eq!(first["project"], "work");
    assert_eq!(first["updated"], 200.0);
    assert_eq!(first["file"], "index.html");
    let path = first["path"].as_str().unwrap();
    assert!(
        path.starts_with(&*dir.path("data/pages").to_string_lossy())
            && path.ends_with("/index.html")
    );
    assert!(Path::new(path).is_file());
    assert!(first.get("describe").is_none(), "list stays light");

    let found = call(&ops_, ops::PAGES_FIND, json!({"query": A}));
    assert_eq!(titles(&found, "pages"), ["Cost Report"]);
    let describe = found["pages"][0]["describe"].as_str().unwrap();
    assert!(describe.starts_with("Title: Cost Report\nLink: https://claude.ai/artifact/AbCd"));
    assert_eq!(
        titles(
            &call(&ops_, ops::PAGES_FIND, json!({"query": "plan work@"})),
            "pages"
        ),
        ["Release Plan"]
    );
    assert_eq!(
        titles(&call(&ops_, ops::PAGES_FIND, json!({"query": ""})), "pages").len(),
        2
    );
    assert!(
        call(&ops_, ops::PAGES_FIND, json!({"query": "nothing"}))["pages"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    for bad in [json!({}), json!({"query": 3}), Value::Null] {
        let e = ops_.call(ops::PAGES_FIND, &bad).unwrap_err();
        assert_eq!(e.code, ErrorCode::BadRequest, "{bad}");
    }
}

#[test]
fn a_damaged_entry_has_no_path_and_a_big_library_is_cut() {
    let dir = Dir::new("big");
    let mut pages: Vec<Value> = (0..1500)
        .map(|n| json!({"folder": format!("f{n}"), "file": "index.html", "title": format!("Page {n} {}", "x".repeat(100)),
                        "url": format!("https://claude.ai/artifact/Long{n:018}"), "updated": n}))
        .collect();
    pages.push(json!({"folder": "../..", "file": "etc", "title": "Evil", "updated": 5000}));
    dir.write(
        "data/pages/pages.json",
        &json!({ "pages": pages }).to_string(),
    );
    let ops_ = PagesOps::new(setup(&dir));
    let list = call(&ops_, ops::PAGES_LIST, json!({}));
    assert_eq!(list["truncated"], true);
    assert_eq!(list["pages"].as_array().unwrap().len(), 1000);
    assert!(list.to_string().len() < notchling_local_api::MAX_LINE);
    assert_eq!(list["pages"][0]["title"], "Evil");
    assert_eq!(list["pages"][0]["path"], Value::Null);
}

#[test]
fn search_returns_pages_and_sessions_grouped() {
    let dir = Dir::new("search");
    save_page(
        &dir,
        "a.html",
        "<title>Token Report</title>",
        A,
        "me@example.com",
        100.0,
    );
    save_page(
        &dir,
        "b.html",
        "<title>Release Plan</title>",
        B,
        "token@example.com",
        200.0,
    );
    let transcript = dir.write(
        "claude/projects/-w-app/c1.jsonl",
        &format!(
            "{}\n{}\n",
            r#"{"type":"user","cwd":"/w/app","message":{"role":"user","content":"fix the token refresh"}}"#,
            r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"The stale token cache was it."}]}}"#
        ),
    );
    age(&transcript, 60.0);
    let ops_ = PagesOps::new(setup(&dir)).with_titles(Arc::new(|| {
        vec![TitledSession {
            id: "ag1".into(),
            title: "Token docs".into(),
            folder: "/w/docs".into(),
            modified: now() - 30.0,
        }]
    }));

    let reply = call(&ops_, ops::SEARCH_QUERY, json!({"text": "Token"}));
    assert_eq!(titles(&reply, "pages"), ["Token Report", "Release Plan"]);
    let sessions = reply["sessions"].as_array().unwrap();
    let keys: Vec<&str> = sessions
        .iter()
        .map(|s| s["key"].as_str().unwrap())
        .collect();
    assert!(
        keys.contains(&"claude:c1") && keys.contains(&"agy:ag1"),
        "{keys:?}"
    );
    let claude = sessions.iter().find(|s| s["key"] == "claude:c1").unwrap();
    assert_eq!(claude["tool"], "claude");
    assert_eq!(claude["session_id"], "c1");
    assert_eq!(claude["folder"], "/w/app");
    assert_eq!(claude["config_dir"], Value::Null);
    assert!(claude["path"].as_str().unwrap().ends_with("c1.jsonl"));
    let snippet = claude["snippet"].as_str().unwrap();
    assert!(snippet.contains("token"), "{snippet}");
    let marks = claude["highlights"].as_array().unwrap();
    assert!(!marks.is_empty());
    let (s, e) = (
        marks[0][0].as_u64().unwrap() as usize,
        marks[0][1].as_u64().unwrap() as usize,
    );
    assert_eq!(snippet[s..e].to_lowercase(), "token");
    assert!(reply.get("indexing").is_some());

    let latest = call(&ops_, ops::SEARCH_QUERY, json!({}));
    assert_eq!(titles(&latest, "pages"), ["Release Plan", "Token Report"]);
    assert!(latest["sessions"]
        .as_array()
        .unwrap()
        .iter()
        .all(|s| s["snippet"].is_null()));
    let one = call(
        &ops_,
        ops::SEARCH_QUERY,
        json!({"text": "token", "limit": 1}),
    );
    assert_eq!(one["pages"].as_array().unwrap().len(), 1);
    assert_eq!(one["sessions"].as_array().unwrap().len(), 1);
    assert_eq!(
        titles(&call(&ops_, ops::SEARCH_QUERY, json!({"text": B})), "pages"),
        ["Release Plan"]
    );
    let none = call(&ops_, ops::SEARCH_QUERY, json!({"text": "zzzzqqq"}));
    assert!(
        none["pages"].as_array().unwrap().is_empty()
            && none["sessions"].as_array().unwrap().is_empty()
    );
}

fn codex_fixture(name: &str) -> String {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../engine/tests/fixtures/codex-children/sessions/2026/11/13");
    let file = std::fs::read_dir(dir)
        .unwrap()
        .flatten()
        .find(|e| {
            e.file_name()
                .to_string_lossy()
                .ends_with(&format!("{name}.jsonl"))
        })
        .unwrap();
    std::fs::read_to_string(file.path()).unwrap()
}

#[test]
fn a_codex_child_is_a_hit_that_names_its_parent() {
    let dir = Dir::new("children");
    let day = notchling_engine::pages::clock::civil_at(now(), 0);
    let place = |thread: &str, stamp: &str| {
        let name =
            format!("rollout-2026-11-13T{stamp}-01a1d5da-0000-7000-8000-00000000000{thread}.jsonl");
        let path = dir.write(
            &format!(
                "codex/sessions/{:04}/{:02}/{:02}/{name}",
                day.year, day.month, day.day
            ),
            &codex_fixture(&format!("00000000000{thread}")),
        );
        age(&path, 60.0);
    };
    place("a", "09-00-00");
    place("b", "09-00-20");
    let ops_ = PagesOps::new(setup(&dir));
    let reply = call(&ops_, ops::SEARCH_QUERY, json!({"text": "tusks"}));
    let sessions = reply["sessions"].as_array().unwrap();
    assert_eq!(sessions.len(), 1, "{sessions:?}");
    assert_eq!(
        sessions[0]["key"],
        "codex:01a1d5da-0000-7000-8000-00000000000b"
    );
    assert_eq!(
        sessions[0]["parent"],
        "codex:01a1d5da-0000-7000-8000-00000000000a"
    );
    let reply = call(&ops_, ops::SEARCH_QUERY, json!({"text": "zebrafish"}));
    let sessions = reply["sessions"].as_array().unwrap();
    assert_eq!(sessions.len(), 1, "{sessions:?}");
    assert_eq!(sessions[0]["parent"], Value::Null);
}

#[test]
fn bad_search_arguments_are_refused() {
    let dir = Dir::new("badargs");
    let ops_ = PagesOps::new(setup(&dir));
    for bad in [
        json!({"text": 3}),
        json!({"limit": "ten"}),
        json!({"limit": -1}),
        json!({"text": ["a"]}),
    ] {
        let e = ops_.call(ops::SEARCH_QUERY, &bad).unwrap_err();
        assert_eq!(e.code, ErrorCode::BadRequest, "{bad}");
    }
    assert!(ops_.call(ops::SEARCH_QUERY, &Value::Null).is_ok());
    assert!(ops_
        .call(ops::SEARCH_QUERY, &json!({"text": null, "limit": null}))
        .is_ok());
    assert!(ops_
        .call(ops::SEARCH_QUERY, &json!({"limit": 1_000_000}))
        .is_ok());
}

#[test]
fn reads_leave_the_swift_apps_folder_alone() {
    let dir = Dir::new("readonly");
    save_page(
        &dir,
        "a.html",
        "<title>Cost</title>",
        A,
        "me@example.com",
        100.0,
    );
    let transcript = dir.write(
        "claude/projects/p/s.jsonl",
        r#"{"type":"user","message":{"role":"user","content":"hello there"}}"#,
    );
    age(&transcript, 10.0);
    let snapshot = |root: &Path| -> Vec<(String, u64)> {
        fn walk(dir: &Path, out: &mut Vec<(String, u64)>) {
            for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
                let meta = e.metadata().unwrap();
                if meta.is_dir() {
                    walk(&e.path(), out);
                } else {
                    let m = meta
                        .modified()
                        .unwrap()
                        .duration_since(UNIX_EPOCH)
                        .unwrap()
                        .as_nanos() as u64;
                    out.push((e.path().to_string_lossy().into_owned(), m ^ meta.len()));
                }
            }
        }
        let mut out = Vec::new();
        walk(root, &mut out);
        out.sort();
        out
    };
    let ops_ = PagesOps::new(setup(&dir));
    assert!(!ops_.library().is_writable());
    let before = snapshot(&dir.path("data/pages"));
    call(&ops_, ops::PAGES_LIST, json!({}));
    call(&ops_, ops::PAGES_FIND, json!({"query": "cost"}));
    call(&ops_, ops::SEARCH_QUERY, json!({"text": "hello"}));
    assert_eq!(
        snapshot(&dir.path("data/pages")),
        before,
        "the pages folder is read, not written"
    );
    assert!(dir.path("data/daemon/search.db").exists());
    assert!(!dir.path("data/search-index.json").exists());
}

#[test]
fn a_refresh_runs_after_the_first_and_only_when_due() {
    let dir = Dir::new("due");
    let transcript = dir.write(
        "claude/projects/p/s.jsonl",
        r#"{"type":"user","message":{"role":"user","content":"first words"}}"#,
    );
    age(&transcript, 10.0);
    let clock = Arc::new(std::sync::Mutex::new(now()));
    let reader = clock.clone();
    let ops_ = PagesOps::new(setup(&dir)).with_clock(Arc::new(move || *reader.lock().unwrap()));
    let first = call(&ops_, ops::SEARCH_QUERY, json!({"text": "first"}));
    assert_eq!(first["sessions"].as_array().unwrap().len(), 1);
    let newer = dir.write(
        "claude/projects/p/t.jsonl",
        r#"{"type":"user","message":{"role":"user","content":"second words"}}"#,
    );
    age(&newer, 5.0);
    assert!(
        call(&ops_, ops::SEARCH_QUERY, json!({"text": "second"}))["sessions"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    *clock.lock().unwrap() += 9.0;
    call(&ops_, ops::SEARCH_QUERY, json!({"text": "second"}));
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    loop {
        let again = call(&ops_, ops::SEARCH_QUERY, json!({"text": "second"}));
        if !again["sessions"].as_array().unwrap().is_empty() {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "the background refresh never landed"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn the_setup_follows_the_layout() {
    use crate::layout::Layout;
    let root = std::ffi::OsString::from("/r");
    let fixture = Layout::resolve(None, Some(root.as_os_str()), || None).unwrap();
    let s = PagesSetup::from_layout(&fixture);
    assert_eq!(s.pages_dir, PathBuf::from("/r/notchling/pages"));
    assert!(s.writable, "a test folder may be written");
    assert_eq!(s.codex_sessions, PathBuf::from("/r/codex/sessions"));
    assert_eq!(
        s.cache,
        Some(PathBuf::from("/r/notchling/daemon/search.db"))
    );
    let with_socket = Layout::resolve(
        Some(Path::new("/tmp/x.sock")),
        Some(root.as_os_str()),
        || None,
    )
    .unwrap();
    assert_eq!(PagesSetup::from_layout(&with_socket).cache, None);
    let real = Layout::resolve(None, None, || {
        Some((
            "/Users/me".into(),
            "/Users/me/Library/Application Support/Notchling".into(),
        ))
    })
    .unwrap();
    let s = PagesSetup::from_layout(&real);
    assert!(!s.writable);
    assert!(s
        .cache
        .as_ref()
        .unwrap()
        .ends_with("Notchling/daemon/search.db"));
    assert_eq!(
        s.pages_dir,
        PathBuf::from("/Users/me/Library/Application Support/Notchling/pages")
    );
}
