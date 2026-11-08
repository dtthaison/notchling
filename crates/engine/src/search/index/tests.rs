//! The index over temp folders: nothing here reads a real `~/.claude` or `~/.codex`.

use super::*;
use crate::sessions::testutil::TempDir;
use std::fs::{File, OpenOptions};
use std::io::{Seek, SeekFrom, Write};
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const DAY: f64 = 86_400.0;
const CODEX_FILE: &str = "rollout-2026-11-05T23-05-22-01a1ac75-2a19-7750-a0d3-2df380733486.jsonl";

fn now() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

fn user(text: &str, cwd: &str) -> String {
    format!(
        r#"{{"type":"user","cwd":"{cwd}","message":{{"role":"user","content":"{text}"}}}}"#
    )
}

fn assistant(text: &str) -> String {
    format!(r#"{{"type":"assistant","message":{{"role":"assistant","content":[{{"type":"text","text":"{text}"}}]}}}}"#)
}

/// Sets a file's modification time to `ago` seconds before now.
fn age(path: &Path, ago: f64) {
    let file = OpenOptions::new().write(true).open(path).unwrap();
    file.set_modified(UNIX_EPOCH + Duration::from_secs_f64((now() - ago).max(0.0)))
        .unwrap();
}

/// Writes `lines` as a transcript and ages it.
fn transcript(dir: &TempDir, rel: &str, lines: &[String], ago: f64) -> std::path::PathBuf {
    let path = dir.write(rel, &(lines.join("\n") + "\n"));
    age(&path, ago);
    path
}

fn roots(dir: &TempDir) -> IndexRoots {
    IndexRoots {
        claude: vec![ClaudeRoot {
            projects: dir.path("claude/projects"),
            config_dir: None,
        }],
        codex_sessions: Some(dir.path("codex/sessions")),
        titled: Vec::new(),
    }
}

fn keys(index: &SessionIndex, query: &[&str]) -> Vec<String> {
    index
        .search(query, now(), 30)
        .into_iter()
        .map(|h| h.meta.key)
        .collect()
}

#[test]
fn claude_codex_and_antigravity_sessions_are_found() {
    let dir = TempDir::new("idx-all");
    transcript(
        &dir,
        "claude/projects/-w-app/c1.jsonl",
        &[
            r#"{"type":"ai-title","aiTitle":"Token cache work"}"#.into(),
            user("fix the token refresh", "/w/app"),
            assistant("Found the stale token cache."),
        ],
        3600.0,
    );
    // The rollout's day folder is today's, so the walk enters it.
    let day = civil_day_path(now());
    transcript(
        &dir,
        &format!("codex/sessions/{day}/{CODEX_FILE}"),
        &[
            r#"{"type":"session_meta","payload":{"id":"01a1ac75","cwd":"/w/codex-app"}}"#.into(),
            r#"{"type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"add a retry to the uploader"}]}}"#.into(),
        ],
        1800.0,
    );
    let mut r = roots(&dir);
    r.titled = vec![TitledSession {
        id: "ag1".into(),
        title: "Write the uploader docs".into(),
        folder: "/w/docs".into(),
        modified: now() - 600.0,
    }];
    let index = SessionIndex::new(None);
    let stats = index.refresh(&r, now());
    assert_eq!((stats.read, stats.reused, stats.dropped), (2, 0, 0));
    assert_eq!(index.len(), 3);

    assert_eq!(keys(&index, &["token"]), vec!["claude:c1"]);
    let hits = index.search(&["uploader"], now(), 30);
    let got: Vec<&str> = hits.iter().map(|h| h.meta.key.as_str()).collect();
    assert!(got.contains(&"agy:ag1") && got.iter().any(|k| k.starts_with("codex:")), "{got:?}");
    let claude = &index.search(&["stale"], now(), 30)[0];
    assert_eq!(claude.meta.title, "Token cache work");
    assert_eq!(claude.meta.folder, "/w/app");
    assert!(claude.snippet.as_deref().unwrap_or("").contains("stale token cache"));
    let codex = hits.iter().find(|h| h.meta.tool_key == "codex").unwrap();
    assert_eq!(codex.meta.folder, "/w/codex-app");
    assert_eq!(codex.meta.session_id, "01a1ac75-2a19-7750-a0d3-2df380733486");
    // No words: the newest first, with no snippet.
    let latest = index.search::<&str>(&[], now(), 2);
    assert_eq!(latest.len(), 2);
    assert!(latest.iter().all(|h| h.snippet.is_none()));
    assert_eq!(latest[0].meta.key, "agy:ag1");
}

/// `YYYY/MM/DD` of a time, in UTC.
fn civil_day_path(ts: f64) -> String {
    let c = crate::pages::clock::civil_at(ts, 0);
    format!("{:04}/{:02}/{:02}", c.year, c.month, c.day)
}

#[test]
fn only_changed_files_are_read_again() {
    let dir = TempDir::new("idx-changed");
    let a = transcript(&dir, "claude/projects/p/a.jsonl", &[user("alpha one", "/w")], 100.0);
    transcript(&dir, "claude/projects/p/b.jsonl", &[user("beta two", "/w")], 100.0);
    let r = roots(&dir);
    let index = SessionIndex::new(None);
    assert_eq!(index.refresh(&r, now()).read, 2);
    let again = index.refresh(&r, now());
    assert_eq!((again.read, again.reused), (0, 2));

    // A change to one file (new size and time) reads that file only.
    transcript(
        &dir,
        "claude/projects/p/a.jsonl",
        &[user("alpha one", "/w"), assistant("gamma three")],
        10.0,
    );
    let third = index.refresh(&r, now());
    assert_eq!((third.read, third.reused), (1, 1));
    assert_eq!(keys(&index, &["gamma"]), vec!["claude:a"]);
    let _ = a;
}

#[test]
fn files_outside_the_window_are_left_out_and_aged_out_ones_dropped() {
    let dir = TempDir::new("idx-window");
    transcript(&dir, "claude/projects/p/old.jsonl", &[user("ancient", "/w")], 20.0 * DAY);
    let fresh = transcript(&dir, "claude/projects/p/new.jsonl", &[user("recent", "/w")], 60.0);
    let r = roots(&dir);
    let index = SessionIndex::new(None);
    assert_eq!(index.refresh(&r, now()).read, 1);
    assert_eq!(keys(&index, &["ancient"]), Vec::<String>::new());
    // Later, the fresh file ages out of the window: dropped.
    age(&fresh, 15.0 * DAY);
    let stats = index.refresh(&r, now());
    assert_eq!((stats.read, stats.dropped), (0, 1));
    assert!(index.is_empty());
    // And a deleted file is dropped too.
    let gone = transcript(&dir, "claude/projects/p/gone.jsonl", &[user("vanishing", "/w")], 5.0);
    assert_eq!(index.refresh(&r, now()).read, 1);
    std::fs::remove_file(gone).unwrap();
    assert_eq!(index.refresh(&r, now()).dropped, 1);
}

#[test]
fn a_big_file_is_read_by_its_start_and_its_end() {
    let dir = TempDir::new("idx-big");
    let path = dir.path("claude/projects/p/big.jsonl");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let mut f = File::create(&path).unwrap();
    writeln!(f, "{}", user("headmarker start", "/w/big")).unwrap();
    let filler = assistant(&"filler text ".repeat(100));
    for i in 0..9_000 {
        // ~1.2 KB a line: about 11 MB in all.
        if i == 4_500 {
            writeln!(f, "{}", assistant("middlemarker only here")).unwrap();
        }
        writeln!(f, "{filler}").unwrap();
    }
    writeln!(f, "{}", assistant("tailmarker the end")).unwrap();
    drop(f);
    age(&path, 30.0);
    assert!(std::fs::metadata(&path).unwrap().len() > 10_000_000);
    let index = SessionIndex::new(None);
    index.refresh(&roots(&dir), now());
    assert_eq!(keys(&index, &["headmarker"]), vec!["claude:big"]);
    assert_eq!(keys(&index, &["tailmarker"]), vec!["claude:big"]);
    assert_eq!(keys(&index, &["middlemarker"]), Vec::<String>::new(), "the middle is not read");
    assert_eq!(index.search(&["tailmarker"], now(), 5)[0].meta.folder, "/w/big");
}

// Sparse files: where the file system has none (NTFS), the test would write the gap.
#[cfg(unix)]
#[test]
fn a_two_gigabyte_file_costs_two_reads() {
    let dir = TempDir::new("idx-2gb");
    let path = dir.path("claude/projects/p/huge.jsonl");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let mut f = File::create(&path).unwrap();
    writeln!(f, "{}", user("headmarker first", "/w")).unwrap();
    // A sparse gap: 2 GB of nothing, then a newline and the last message.
    f.seek(SeekFrom::Start(2 * 1024 * 1024 * 1024)).unwrap();
    writeln!(f).unwrap();
    writeln!(f, "{}", assistant("tailmarker last")).unwrap();
    drop(f);
    age(&path, 10.0);
    let index = SessionIndex::new(None);
    let started = std::time::Instant::now();
    index.refresh(&roots(&dir), now());
    assert!(started.elapsed() < Duration::from_secs(20), "{:?}", started.elapsed());
    assert_eq!(keys(&index, &["headmarker"]), vec!["claude:huge"]);
    assert_eq!(keys(&index, &["tailmarker"]), vec!["claude:huge"]);
}

// Sparse files: where the file system has none (NTFS), the test would write the gap.
#[cfg(unix)]
#[test]
fn a_line_of_two_hundred_megabytes_is_never_held() {
    let dir = TempDir::new("idx-line");
    let path = dir.path("claude/projects/p/line.jsonl");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let mut f = File::create(&path).unwrap();
    writeln!(f, "{}", user("headmarker one", "/w")).unwrap();
    // One 200 MB "line" (no newline in it), then a normal one.
    f.seek(SeekFrom::Start(200 * 1024 * 1024)).unwrap();
    writeln!(f).unwrap();
    writeln!(f, "{}", assistant("tailmarker after the monster")).unwrap();
    drop(f);
    age(&path, 10.0);
    let index = SessionIndex::new(None);
    index.refresh(&roots(&dir), now());
    assert_eq!(keys(&index, &["headmarker"]), vec!["claude:line"]);
    assert_eq!(keys(&index, &["tailmarker"]), vec!["claude:line"]);
}

#[test]
fn a_long_line_in_a_small_file_is_skipped_and_the_rest_kept() {
    let dir = TempDir::new("idx-skip");
    let huge = format!(
        r#"{{"type":"user","message":{{"role":"user","content":"{}"}}}}"#,
        "x".repeat(150_000)
    );
    transcript(
        &dir,
        "claude/projects/p/s.jsonl",
        &[user("before skipme", "/w"), huge, assistant("after skipme")],
        10.0,
    );
    let index = SessionIndex::new(None);
    index.refresh(&roots(&dir), now());
    let hit = &index.search(&["skipme"], now(), 5)[0];
    assert_eq!(hit.meta.key, "claude:s");
    assert!(index.search(&["xxxxxxxx"], now(), 5).is_empty());
}

#[test]
fn a_file_that_vanishes_or_cannot_be_read_does_not_stop_the_scan() {
    let dir = TempDir::new("idx-gone");
    transcript(&dir, "claude/projects/p/ok.jsonl", &[user("still here", "/w")], 10.0);
    // A dangling link, a folder named like a transcript and an empty file.
    #[cfg(unix)]
    std::os::unix::fs::symlink(dir.path("nowhere"), dir.path("claude/projects/p/dangling.jsonl"))
        .unwrap();
    std::fs::create_dir_all(dir.path("claude/projects/p/dir.jsonl")).unwrap();
    dir.write("claude/projects/p/empty.jsonl", "");
    let index = SessionIndex::new(None);
    let stats = index.refresh(&roots(&dir), now());
    assert_eq!(stats.read, 1);
    assert_eq!(keys(&index, &["still"]), vec!["claude:ok"]);
    // Reading a path that has gone is "gone", not a crash.
    let meta = SessionMeta {
        key: "claude:x".into(),
        tool_key: "claude".into(),
        session_id: "x".into(),
        title: String::new(),
        folder: String::new(),
        path: dir.path("claude/projects/p/never.jsonl").to_string_lossy().into_owned(),
        config_dir: None,
        modified: 0.0,
        size: 10,
    };
    assert!(matches!(read::read(&meta, SessionTool::Claude), read::Read1::Gone));
    // A missing root is no error either.
    let index = SessionIndex::new(None);
    let nothing = IndexRoots {
        claude: vec![ClaudeRoot { projects: dir.path("absent"), config_dir: None }],
        codex_sessions: Some(dir.path("absent2")),
        titled: vec![],
    };
    assert_eq!(index.refresh(&nothing, now()), Refresh::default());
}

#[test]
fn the_cache_carries_the_index_to_the_next_start() {
    let dir = TempDir::new("idx-cache");
    transcript(&dir, "claude/projects/p/a.jsonl", &[user("remember me please", "/w/a")], 10.0);
    let cache = dir.path("daemon/search-index.json");
    let r = roots(&dir);
    let first = SessionIndex::new(Some(cache.clone()));
    first.refresh(&r, now());
    assert!(cache.exists());
    // The Swift app's shape.
    let raw: serde_json::Value = serde_json::from_slice(&std::fs::read(&cache).unwrap()).unwrap();
    assert_eq!(raw["version"], CACHE_VERSION);
    assert_eq!(raw["sessions"][0]["meta"]["toolKey"], "claude");
    assert_eq!(raw["sessions"][0]["meta"]["sessionID"], "a");
    assert!(raw["sessions"][0]["text"].as_str().unwrap().contains("remember me"));

    // A new index answers from the cache before it reads anything, and reuses the entry.
    let second = SessionIndex::new(Some(cache.clone()));
    second.load_cache();
    assert_eq!(keys(&second, &["remember"]), vec!["claude:a"]);
    let stats = second.refresh(&r, now());
    assert_eq!((stats.read, stats.reused), (0, 1));
}

#[test]
fn a_cache_of_another_version_or_damaged_is_rebuilt() {
    let dir = TempDir::new("idx-version");
    transcript(&dir, "claude/projects/p/a.jsonl", &[user("rebuild me", "/w")], 10.0);
    let cache = dir.path("daemon/search-index.json");
    let r = roots(&dir);
    SessionIndex::new(Some(cache.clone())).refresh(&r, now());

    let mut raw: serde_json::Value = serde_json::from_slice(&std::fs::read(&cache).unwrap()).unwrap();
    raw["version"] = serde_json::json!(CACHE_VERSION + 1);
    // A stale entry the rebuild must not trust.
    raw["sessions"][0]["text"] = serde_json::json!("stale words");
    std::fs::write(&cache, raw.to_string()).unwrap();
    let rebuilt = SessionIndex::new(Some(cache.clone()));
    let stats = rebuilt.refresh(&r, now());
    assert_eq!(stats.read, 1, "read again, not reused");
    assert_eq!(keys(&rebuilt, &["stale"]), Vec::<String>::new());
    assert_eq!(keys(&rebuilt, &["rebuild"]), vec!["claude:a"]);

    std::fs::write(&cache, "{ not json").unwrap();
    let damaged = SessionIndex::new(Some(cache));
    damaged.load_cache();
    assert!(damaged.is_empty());
    assert_eq!(damaged.refresh(&r, now()).read, 1);
}

#[test]
fn a_session_in_two_lanes_keeps_the_newer_copy_and_a_linked_projects_folder_counts_once() {
    let dir = TempDir::new("idx-lanes");
    transcript(&dir, "main/projects/p/s.jsonl", &[user("shared session", "/w")], 500.0);
    transcript(&dir, "lane/projects/p/s.jsonl", &[user("shared session newer", "/w")], 50.0);
    let mut r = roots(&dir);
    r.claude = vec![
        ClaudeRoot { projects: dir.path("main/projects"), config_dir: None },
        ClaudeRoot {
            projects: dir.path("lane/projects"),
            config_dir: Some(dir.path("lane").to_string_lossy().into_owned()),
        },
    ];
    #[cfg(unix)]
    {
        // A second lane whose projects folder is a link to the main one.
        std::fs::create_dir_all(dir.path("lane2")).unwrap();
        std::os::unix::fs::symlink(dir.path("main/projects"), dir.path("lane2/projects")).unwrap();
        r.claude.push(ClaudeRoot {
            projects: dir.path("lane2/projects"),
            config_dir: Some("lane2".into()),
        });
    }
    let index = SessionIndex::new(None);
    let stats = index.refresh(&r, now());
    assert_eq!(stats.read, 1, "one session, one read");
    let hit = &index.search(&["shared"], now(), 5)[0];
    assert!(hit.meta.config_dir.as_deref().unwrap_or("").ends_with("lane"));
    assert!(hit.meta.path.contains("lane/projects"));
}

#[test]
fn codex_rollouts_are_found_only_under_recent_day_folders() {
    let dir = TempDir::new("idx-codex-days");
    let line = |id: &str| -> Vec<String> {
        vec![
            format!(r#"{{"type":"session_meta","payload":{{"id":"{id}","cwd":"/w"}}}}"#),
            r#"{"type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"codex words here"}]}}"#.into(),
        ]
    };
    let today = civil_day_path(now());
    let old = "2020/01/15";
    let name = |n: u32| format!("rollout-2026-11-05T23-05-22-01a1ac75-2a19-7750-a0d3-2df3807334{n:02}.jsonl");
    transcript(&dir, &format!("codex/sessions/{today}/{}", name(11)), &line("a"), 60.0);
    // Written to recently but started long ago: not found, like the Swift app.
    transcript(&dir, &format!("codex/sessions/{old}/{}", name(22)), &line("b"), 60.0);
    // A file that isn't a rollout, and a folder that isn't a date.
    transcript(&dir, &format!("codex/sessions/{today}/notes.jsonl"), &line("c"), 60.0);
    transcript(&dir, &format!("codex/sessions/misc/{}", name(33)), &line("d"), 60.0);
    let index = SessionIndex::new(None);
    index.refresh(&roots(&dir), now());
    assert_eq!(
        keys(&index, &["codex"]),
        vec!["codex:01a1ac75-2a19-7750-a0d3-2df380733411"]
    );
}

#[test]
fn antigravity_titles_come_and_go_with_the_rows() {
    let dir = TempDir::new("idx-agy");
    let row = |id: &str, title: &str| TitledSession {
        id: id.into(),
        title: title.into(),
        folder: "/w".into(),
        modified: now(),
    };
    let mut r = roots(&dir);
    r.titled = vec![row("a", "Plan the migration"), row("b", "Review the schema")];
    let index = SessionIndex::new(None);
    index.refresh(&r, now());
    assert_eq!(keys(&index, &["migration"]), vec!["agy:a"]);
    r.titled = vec![row("b", "Review the schema")];
    let stats = index.refresh(&r, now());
    assert_eq!(stats.dropped, 1);
    assert_eq!(keys(&index, &["migration"]), Vec::<String>::new());
    assert_eq!(keys(&index, &["schema"]), vec!["agy:b"]);
}

#[test]
fn a_second_refresh_while_one_runs_does_nothing() {
    let dir = TempDir::new("idx-busy");
    let index = SessionIndex::new(None);
    index.refreshing.store(true, Ordering::SeqCst);
    let stats = index.refresh(&roots(&dir), now());
    assert!(stats.skipped);
    assert!(index.is_refreshing(), "the running refresh's flag is not cleared by the skipped one");
    index.refreshing.store(false, Ordering::SeqCst);
    assert!(!index.refresh(&roots(&dir), now()).skipped);
    assert!(!index.is_refreshing());
}

#[test]
fn due_follows_the_last_refresh() {
    let dir = TempDir::new("idx-due");
    let index = SessionIndex::new(None);
    assert!(index.due(1000.0, 8.0));
    index.refresh(&roots(&dir), 1000.0);
    assert!(!index.due(1004.0, 8.0));
    assert!(index.due(1008.0, 8.0));
}
