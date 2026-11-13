use super::*;
use crate::sessions::testutil::TempDir;
use std::fs::{File, OpenOptions};
use std::io::Write;
#[cfg(unix)]
use std::io::{Seek, SeekFrom};
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
    format!(r#"{{"type":"user","cwd":"{cwd}","message":{{"role":"user","content":"{text}"}}}}"#)
}

fn assistant(text: &str) -> String {
    format!(
        r#"{{"type":"assistant","message":{{"role":"assistant","content":[{{"type":"text","text":"{text}"}}]}}}}"#
    )
}

fn age(path: &Path, ago: f64) {
    let file = OpenOptions::new().write(true).open(path).unwrap();
    file.set_modified(UNIX_EPOCH + Duration::from_secs_f64((now() - ago).max(0.0)))
        .unwrap();
}

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
    assert!(
        got.contains(&"agy:ag1") && got.iter().any(|k| k.starts_with("codex:")),
        "{got:?}"
    );
    let claude = &index.search(&["stale"], now(), 30)[0];
    assert_eq!(claude.meta.title, "Token cache work");
    assert_eq!(claude.meta.folder, "/w/app");
    assert!(claude
        .snippet
        .as_deref()
        .unwrap_or("")
        .contains("stale token cache"));
    let codex = hits.iter().find(|h| h.meta.tool_key == "codex").unwrap();
    assert_eq!(codex.meta.folder, "/w/codex-app");
    assert_eq!(
        codex.meta.session_id,
        "01a1ac75-2a19-7750-a0d3-2df380733486"
    );
    let latest = index.search::<&str>(&[], now(), 2);
    assert_eq!(latest.len(), 2);
    assert!(latest.iter().all(|h| h.snippet.is_none()));
    assert_eq!(latest[0].meta.key, "agy:ag1");
}

fn civil_day_path(ts: f64) -> String {
    let c = crate::pages::clock::civil_at(ts, 0);
    format!("{:04}/{:02}/{:02}", c.year, c.month, c.day)
}

#[test]
fn only_changed_files_are_read_again() {
    let dir = TempDir::new("idx-changed");
    let a = transcript(
        &dir,
        "claude/projects/p/a.jsonl",
        &[user("alpha one", "/w")],
        100.0,
    );
    transcript(
        &dir,
        "claude/projects/p/b.jsonl",
        &[user("beta two", "/w")],
        100.0,
    );
    let r = roots(&dir);
    let index = SessionIndex::new(None);
    assert_eq!(index.refresh(&r, now()).read, 2);
    let again = index.refresh(&r, now());
    assert_eq!((again.read, again.reused), (0, 2));

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
    transcript(
        &dir,
        "claude/projects/p/old.jsonl",
        &[user("ancient", "/w")],
        20.0 * DAY,
    );
    let fresh = transcript(
        &dir,
        "claude/projects/p/new.jsonl",
        &[user("recent", "/w")],
        60.0,
    );
    let r = roots(&dir);
    let index = SessionIndex::new(None);
    assert_eq!(index.refresh(&r, now()).read, 1);
    assert_eq!(keys(&index, &["ancient"]), Vec::<String>::new());
    age(&fresh, 15.0 * DAY);
    let stats = index.refresh(&r, now());
    assert_eq!((stats.read, stats.dropped), (0, 1));
    assert!(index.is_empty());
    let gone = transcript(
        &dir,
        "claude/projects/p/gone.jsonl",
        &[user("vanishing", "/w")],
        5.0,
    );
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
    assert_eq!(
        keys(&index, &["middlemarker"]),
        Vec::<String>::new(),
        "the middle is not read"
    );
    assert_eq!(
        index.search(&["tailmarker"], now(), 5)[0].meta.folder,
        "/w/big"
    );
}

#[cfg(unix)]
#[test]
fn a_two_gigabyte_file_costs_two_reads() {
    let dir = TempDir::new("idx-2gb");
    let path = dir.path("claude/projects/p/huge.jsonl");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let mut f = File::create(&path).unwrap();
    writeln!(f, "{}", user("headmarker first", "/w")).unwrap();
    f.seek(SeekFrom::Start(2 * 1024 * 1024 * 1024)).unwrap();
    writeln!(f).unwrap();
    writeln!(f, "{}", assistant("tailmarker last")).unwrap();
    drop(f);
    age(&path, 10.0);
    let index = SessionIndex::new(None);
    let started = std::time::Instant::now();
    index.refresh(&roots(&dir), now());
    assert!(
        started.elapsed() < Duration::from_secs(20),
        "{:?}",
        started.elapsed()
    );
    assert_eq!(keys(&index, &["headmarker"]), vec!["claude:huge"]);
    assert_eq!(keys(&index, &["tailmarker"]), vec!["claude:huge"]);
}

#[cfg(unix)]
#[test]
fn a_line_of_two_hundred_megabytes_is_never_held() {
    let dir = TempDir::new("idx-line");
    let path = dir.path("claude/projects/p/line.jsonl");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let mut f = File::create(&path).unwrap();
    writeln!(f, "{}", user("headmarker one", "/w")).unwrap();
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
    transcript(
        &dir,
        "claude/projects/p/ok.jsonl",
        &[user("still here", "/w")],
        10.0,
    );
    #[cfg(unix)]
    std::os::unix::fs::symlink(
        dir.path("nowhere"),
        dir.path("claude/projects/p/dangling.jsonl"),
    )
    .unwrap();
    std::fs::create_dir_all(dir.path("claude/projects/p/dir.jsonl")).unwrap();
    dir.write("claude/projects/p/empty.jsonl", "");
    let index = SessionIndex::new(None);
    let stats = index.refresh(&roots(&dir), now());
    assert_eq!(stats.read, 1);
    assert_eq!(keys(&index, &["still"]), vec!["claude:ok"]);
    let meta = SessionMeta {
        key: "claude:x".into(),
        tool_key: "claude".into(),
        session_id: "x".into(),
        title: String::new(),
        folder: String::new(),
        path: dir
            .path("claude/projects/p/never.jsonl")
            .to_string_lossy()
            .into_owned(),
        config_dir: None,
        modified: 0.0,
        size: 10,
    };
    assert!(matches!(
        read::read(&meta, SessionTool::Claude),
        read::Read1::Gone
    ));
    let index = SessionIndex::new(None);
    let nothing = IndexRoots {
        claude: vec![ClaudeRoot {
            projects: dir.path("absent"),
            config_dir: None,
        }],
        codex_sessions: Some(dir.path("absent2")),
        titled: vec![],
    };
    assert_eq!(index.refresh(&nothing, now()), Refresh::default());
}

#[test]
fn the_database_carries_the_index_to_the_next_start() {
    let dir = TempDir::new("idx-cache");
    transcript(
        &dir,
        "claude/projects/p/a.jsonl",
        &[user("remember me please", "/w/a")],
        10.0,
    );
    let db = dir.path("daemon/search.db");
    let r = roots(&dir);
    let first = SessionIndex::new(Some(db.clone()));
    first.refresh(&r, now());
    assert!(db.exists());

    let second = SessionIndex::new(Some(db.clone()));
    second.load_cache();
    assert_eq!(keys(&second, &["remember"]), vec!["claude:a"]);
    let stats = second.refresh(&r, now());
    assert_eq!((stats.read, stats.reused), (0, 1));
}

#[test]
fn bm25_ranks_first_and_a_word_inside_another_still_finds_by_substring() {
    let dir = TempDir::new("idx-bm25");
    transcript(
        &dir,
        "claude/projects/p/a.jsonl",
        &[user("the ranking test fails", "/w")],
        10.0,
    );
    let index = SessionIndex::new(None);
    index.refresh(&roots(&dir), now());
    assert_eq!(
        keys(&index, &["rank"]),
        vec!["claude:a"],
        "a prefix, by bm25"
    );
    assert_eq!(keys(&index, &["anking"]), vec!["claude:a"], "inside a word");
    let hit = &index.search(&["ranking"], now(), 5)[0];
    assert!(hit.snippet.as_deref().unwrap_or("").contains("ranking"));
}

#[test]
fn a_damaged_database_is_set_aside_and_rebuilt() {
    let dir = TempDir::new("idx-version");
    transcript(
        &dir,
        "claude/projects/p/a.jsonl",
        &[user("rebuild me", "/w")],
        10.0,
    );
    let db = dir.path("daemon/search.db");
    let r = roots(&dir);
    std::fs::create_dir_all(dir.path("daemon")).unwrap();
    std::fs::write(&db, "{ not a database").unwrap();
    let damaged = SessionIndex::new(Some(db.clone()));
    damaged.load_cache();
    assert!(damaged.is_empty());
    assert_eq!(damaged.refresh(&r, now()).read, 1);
    assert_eq!(keys(&damaged, &["rebuild"]), vec!["claude:a"]);
    let again = SessionIndex::new(Some(db));
    again.load_cache();
    assert_eq!(again.len(), 1, "the new database was kept");
}

#[test]
fn a_session_in_two_lanes_keeps_the_newer_copy_and_a_linked_projects_folder_counts_once() {
    let dir = TempDir::new("idx-lanes");
    transcript(
        &dir,
        "main/projects/p/s.jsonl",
        &[user("shared session", "/w")],
        500.0,
    );
    transcript(
        &dir,
        "lane/projects/p/s.jsonl",
        &[user("shared session newer", "/w")],
        50.0,
    );
    let mut r = roots(&dir);
    r.claude = vec![
        ClaudeRoot {
            projects: dir.path("main/projects"),
            config_dir: None,
        },
        ClaudeRoot {
            projects: dir.path("lane/projects"),
            config_dir: Some(dir.path("lane").to_string_lossy().into_owned()),
        },
    ];
    #[cfg(unix)]
    {
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
    assert!(hit
        .meta
        .config_dir
        .as_deref()
        .unwrap_or("")
        .ends_with("lane"));
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
    let name = |n: u32| {
        format!("rollout-2026-11-05T23-05-22-01a1ac75-2a19-7750-a0d3-2df3807334{n:02}.jsonl")
    };
    transcript(
        &dir,
        &format!("codex/sessions/{today}/{}", name(11)),
        &line("a"),
        60.0,
    );
    transcript(
        &dir,
        &format!("codex/sessions/{old}/{}", name(22)),
        &line("b"),
        60.0,
    );
    transcript(
        &dir,
        &format!("codex/sessions/{today}/notes.jsonl"),
        &line("c"),
        60.0,
    );
    transcript(
        &dir,
        &format!("codex/sessions/misc/{}", name(33)),
        &line("d"),
        60.0,
    );
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
    r.titled = vec![
        row("a", "Plan the migration"),
        row("b", "Review the schema"),
    ];
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
    assert!(
        index.is_refreshing(),
        "the running refresh's flag is not cleared by the skipped one"
    );
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
