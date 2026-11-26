use super::*;
use crate::sessions::testutil::TempDir;
use std::time::{Duration, SystemTime};

fn user(text: &str, cwd: &str) -> String {
    format!(r#"{{"type":"user","cwd":"{cwd}","message":{{"role":"user","content":"{text}"}}}}"#)
}

fn assistant(text: &str) -> String {
    format!(
        r#"{{"type":"assistant","message":{{"role":"assistant","content":[{{"type":"text","text":"{text}"}}]}}}}"#
    )
}

fn put(dir: &TempDir, config: &str, folder: &str, id: &str, lines: &[String], age_s: u64) {
    let path = dir.write(
        &format!("{config}/projects/{folder}/{id}.jsonl"),
        &(lines.join("\n") + "\n"),
    );
    let at = SystemTime::now() - Duration::from_secs(age_s);
    fs::File::options()
        .write(true)
        .open(path)
        .unwrap()
        .set_modified(at)
        .unwrap();
}

fn sources(dir: &TempDir) -> Vec<Source> {
    vec![
        Source {
            projects: dir.path("main/projects"),
            config_dir: None,
        },
        Source {
            projects: dir.path("lane/projects"),
            config_dir: Some(dir.path("lane").to_string_lossy().into_owned()),
        },
    ]
}

fn all(_: &str, _: i64) -> bool {
    true
}

fn run(history: &History, dir: &TempDir, skip: &[&str], after: Option<&str>, limit: usize) -> Page {
    let skip: HashSet<String> = skip.iter().map(|s| s.to_string()).collect();
    history.page(&Query {
        sources: &sources(dir),
        ended: Vec::new(),
        skip: &skip,
        since_ms: None,
        keep: &all,
        after,
        limit,
    })
}

fn ids(page: &Page) -> Vec<&str> {
    page.items.iter().map(|i| i.id.as_str()).collect()
}

#[test]
fn past_sessions_come_newest_first_across_config_dirs() {
    let dir = TempDir::new("hist-order");
    put(
        &dir,
        "main",
        "-w-a",
        "old",
        &[user("old one", "/w/a")],
        5_000,
    );
    put(&dir, "lane", "-w-b", "new", &[user("new one", "/w/b")], 100);
    put(&dir, "main", "-w-a", "mid", &[user("mid one", "/w/a")], 900);
    let page = run(&History::new(), &dir, &[], None, 30);
    assert_eq!(ids(&page), ["new", "mid", "old"]);
    assert_eq!(page.next, None);
    assert_eq!(page.items[0].folder, "/w/b");
    assert_eq!(
        page.items[0].config_dir.as_deref(),
        Some(dir.path("lane").to_string_lossy().as_ref())
    );
    assert_eq!(page.items[1].config_dir, None);
}

#[test]
fn live_and_ended_ids_are_not_listed_twice() {
    let dir = TempDir::new("hist-dedupe");
    put(
        &dir,
        "main",
        "-w-a",
        "live",
        &[user("live one", "/w/a")],
        100,
    );
    put(
        &dir,
        "main",
        "-w-a",
        "gone",
        &[user("gone one", "/w/a")],
        200,
    );
    put(
        &dir,
        "main",
        "-w-a",
        "past",
        &[user("past one", "/w/a")],
        300,
    );
    let ended = Item {
        kind: Kind::Ended,
        id: "gone".into(),
        title: "gone one".into(),
        folder: "/w/a".into(),
        config_dir: None,
        path: None,
        modified_ms: 1,
    };
    let skip: HashSet<String> = ["live", "gone"].map(String::from).into();
    let page = History::new().page(&Query {
        sources: &sources(&dir),
        ended: vec![ended],
        skip: &skip,
        since_ms: None,
        keep: &all,
        after: None,
        limit: 30,
    });
    assert_eq!(ids(&page), ["gone", "past"]);
    assert_eq!(page.items[0].kind, Kind::Ended);
    assert_eq!(page.items[1].kind, Kind::Past);
}

#[test]
fn paging_continues_after_the_cursor_without_repeats() {
    let dir = TempDir::new("hist-paging");
    for n in 0..7u64 {
        put(
            &dir,
            "main",
            "-w-a",
            &format!("s{n}"),
            &[user(&format!("task {n}"), "/w/a")],
            100 + n * 100,
        );
    }
    let history = History::new();
    let first = run(&history, &dir, &[], None, 3);
    assert_eq!(ids(&first), ["s0", "s1", "s2"]);
    let second = run(&history, &dir, &[], first.next.as_deref(), 3);
    assert_eq!(ids(&second), ["s3", "s4", "s5"]);
    let third = run(&history, &dir, &[], second.next.as_deref(), 3);
    assert_eq!(ids(&third), ["s6"]);
    assert_eq!(third.next, None);
}

#[test]
fn the_cursor_resumes_after_ended_rows_too() {
    let dir = TempDir::new("hist-cursor-ended");
    put(&dir, "main", "-w-a", "p1", &[user("p one", "/w/a")], 100);
    let ended = |id: &str, ms: i64| Item {
        kind: Kind::Ended,
        id: id.into(),
        title: id.into(),
        folder: "/w/a".into(),
        config_dir: None,
        path: None,
        modified_ms: ms,
    };
    let skip = HashSet::new();
    let query = |after: Option<&str>| {
        History::new().page(&Query {
            sources: &sources(&dir),
            ended: vec![ended("e1", 20), ended("e2", 10)],
            skip: &skip,
            since_ms: None,
            keep: &all,
            after,
            limit: 2,
        })
    };
    let first = query(None);
    assert_eq!(ids(&first), ["e1", "e2"]);
    let second = query(first.next.as_deref());
    assert_eq!(ids(&second), ["p1"]);
    assert!(valid_cursor(first.next.as_deref().unwrap()));
    assert!(!valid_cursor("zzz"));
}

#[test]
fn the_title_is_the_custom_title_then_the_summary_then_the_first_prompt() {
    let dir = TempDir::new("hist-title");
    let custom = r#"{"type":"custom-title","customTitle":"My Name"}"#.to_string();
    let ai = r#"{"type":"ai-title","aiTitle":"Generated name"}"#.to_string();
    let summary = r#"{"type":"summary","summary":"Old summary"}"#.to_string();
    put(
        &dir,
        "main",
        "-w-a",
        "c",
        &[user("first", "/w/a"), ai.clone(), custom],
        100,
    );
    put(&dir, "main", "-w-a", "a", &[user("first", "/w/a"), ai], 200);
    put(
        &dir,
        "main",
        "-w-a",
        "s",
        &[summary, user("first", "/w/a")],
        300,
    );
    put(
        &dir,
        "main",
        "-w-a",
        "p",
        &[
            user("<command-name>/clear</command-name>", "/w/a"),
            user("Fix the login page\\nthen more words", "/w/a"),
        ],
        400,
    );
    let page = run(&History::new(), &dir, &[], None, 30);
    let title = |id: &str| {
        page.items
            .iter()
            .find(|i| i.id == id)
            .map(|i| i.title.clone())
            .unwrap()
    };
    assert_eq!(title("c"), "My Name");
    assert_eq!(title("a"), "Generated name");
    assert_eq!(title("s"), "Old summary");
    assert_eq!(title("p"), "Fix the login page");
}

#[test]
fn a_long_prompt_is_cut_to_one_line() {
    let dir = TempDir::new("hist-cut");
    let long = "word ".repeat(60);
    put(&dir, "main", "-w-a", "l", &[user(&long, "/w/a")], 100);
    let page = run(&History::new(), &dir, &[], None, 30);
    assert_eq!(page.items[0].title.chars().count(), 80);
    assert!(page.items[0].title.ends_with('…'));
}

#[test]
fn sidechains_subagent_files_and_empty_transcripts_are_left_out() {
    let dir = TempDir::new("hist-skip");
    put(
        &dir,
        "main",
        "-w-a",
        "real",
        &[user("real work", "/w/a")],
        100,
    );
    let side = r#"{"type":"user","isSidechain":true,"cwd":"/w/a","message":{"role":"user","content":"subagent task"}}"#;
    put(&dir, "main", "-w-a", "side", &[side.to_string()], 110);
    put(
        &dir,
        "main",
        "-w-a",
        "agent-ab12",
        &[user("old subagent", "/w/a")],
        120,
    );
    put(
        &dir,
        "main",
        "-w-a",
        "bare",
        &[r#"{"type":"ai-title","aiTitle":"Only a title"}"#.to_string()],
        130,
    );
    dir.write("main/projects/-w-a/empty.jsonl", "");
    dir.write(
        "main/projects/-w-a/real/subagents/agent-1.jsonl",
        &(user("nested", "/w/a") + "\n"),
    );
    let page = run(&History::new(), &dir, &[], None, 30);
    assert_eq!(ids(&page), ["real"]);
}

#[test]
fn a_head_is_read_once_until_the_file_changes() {
    let dir = TempDir::new("hist-cache");
    put(&dir, "main", "-w-a", "one", &[user("one", "/w/a")], 100);
    put(&dir, "main", "-w-a", "two", &[user("two", "/w/a")], 200);
    let history = History::new();
    run(&history, &dir, &[], None, 30);
    assert_eq!(history.head_reads(), 2);
    run(&history, &dir, &[], None, 30);
    assert_eq!(history.head_reads(), 2);
    put(
        &dir,
        "main",
        "-w-a",
        "one",
        &[user("one again", "/w/a")],
        50,
    );
    let page = run(&history, &dir, &[], None, 30);
    assert_eq!(history.head_reads(), 3);
    assert_eq!(page.items[0].title, "one again");
}

#[test]
fn a_big_transcript_is_read_by_its_head_and_tail_only() {
    let dir = TempDir::new("hist-big");
    let filler = "x".repeat(900);
    let mut lines = vec![user("the first prompt", "/w/a")];
    lines.extend((0..1_000).map(|_| assistant(&filler)));
    lines.push(r#"{"type":"custom-title","customTitle":"Renamed late"}"#.to_string());
    put(&dir, "main", "-w-a", "big", &lines, 100);
    let size = fs::metadata(dir.path("main/projects/-w-a/big.jsonl"))
        .unwrap()
        .len();
    assert!(size > HEAD_BYTES + TAIL_BYTES);
    let page = run(&History::new(), &dir, &[], None, 30);
    assert_eq!(page.items[0].title, "Renamed late");
    assert_eq!(page.items[0].folder, "/w/a");
}

#[test]
fn the_window_and_the_folder_check_cut_the_list() {
    let dir = TempDir::new("hist-cut-reach");
    put(&dir, "main", "-w-a", "mine", &[user("mine", "/w/a")], 100);
    put(
        &dir,
        "main",
        "-w-b",
        "theirs",
        &[user("theirs", "/w/b")],
        200,
    );
    put(
        &dir,
        "main",
        "-w-a",
        "stale",
        &[user("stale", "/w/a")],
        20 * 86_400,
    );
    let skip = HashSet::new();
    let now_ms = i64::try_from(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis(),
    )
    .unwrap();
    let only_a = |folder: &str, _: i64| folder == "/w/a";
    let page = History::new().page(&Query {
        sources: &sources(&dir),
        ended: Vec::new(),
        skip: &skip,
        since_ms: Some(now_ms - 14 * 86_400_000),
        keep: &only_a,
        after: None,
        limit: 30,
    });
    assert_eq!(ids(&page), ["mine"]);
}

#[cfg(unix)]
#[test]
fn symlinks_out_of_the_projects_root_are_not_followed() {
    use std::os::unix::fs::symlink;
    let dir = TempDir::new("hist-escape");
    put(
        &dir,
        "main",
        "-w-a",
        "inside",
        &[user("inside", "/w/a")],
        100,
    );
    put(
        &dir,
        "elsewhere",
        "-w-out",
        "folder-leak",
        &[user("folder leak", "/w/out")],
        100,
    );
    put(
        &dir,
        "elsewhere",
        "-w-out",
        "file-leak",
        &[user("file leak", "/w/out")],
        100,
    );
    symlink(
        dir.path("elsewhere/projects/-w-out"),
        dir.path("main/projects/-w-link"),
    )
    .unwrap();
    symlink(
        dir.path("elsewhere/projects/-w-out/file-leak.jsonl"),
        dir.path("main/projects/-w-a/file-leak.jsonl"),
    )
    .unwrap();
    let page = run(&History::new(), &dir, &[], None, 30);
    assert_eq!(ids(&page), ["inside"]);
}

#[cfg(unix)]
#[test]
fn a_symlinked_projects_root_keeps_working() {
    use std::os::unix::fs::symlink;
    let dir = TempDir::new("hist-root-link");
    put(&dir, "main", "-w-a", "one", &[user("one", "/w/a")], 100);
    put(&dir, "main", "-w-a", "two", &[user("two", "/w/a")], 200);
    std::fs::create_dir_all(dir.path("lane")).unwrap();
    symlink(dir.path("main/projects"), dir.path("lane/projects")).unwrap();
    let only_lane = vec![Source {
        projects: dir.path("lane/projects"),
        config_dir: Some(dir.path("lane").to_string_lossy().into_owned()),
    }];
    let page = History::new().page(&Query {
        sources: &only_lane,
        ended: Vec::new(),
        skip: &HashSet::new(),
        since_ms: None,
        keep: &all,
        after: None,
        limit: 30,
    });
    assert_eq!(ids(&page), ["one", "two"]);
}

#[cfg(unix)]
#[test]
fn a_head_is_never_read_through_a_symlink() {
    use std::os::unix::fs::symlink;
    let dir = TempDir::new("hist-nofollow");
    let real = dir.write("outside/secret.jsonl", &(user("secret", "/w/x") + "\n"));
    let link = dir.path("inside.jsonl");
    symlink(&real, &link).unwrap();
    let size = fs::metadata(&real).unwrap().len();
    assert!(window(&real, size).is_some());
    assert!(window(&link, size).is_none());
}

#[cfg(unix)]
#[test]
fn items_carry_the_real_path_even_under_a_symlinked_projects_root() {
    use std::os::unix::fs::symlink;
    let dir = TempDir::new("hist-real-path");
    put(&dir, "main", "-w-a", "one", &[user("one", "/w/a")], 100);
    std::fs::create_dir_all(dir.path("lane")).unwrap();
    symlink(dir.path("main/projects"), dir.path("lane/projects")).unwrap();
    let only_lane = vec![Source {
        projects: dir.path("lane/projects"),
        config_dir: None,
    }];
    let page = History::new().page(&Query {
        sources: &only_lane,
        ended: Vec::new(),
        skip: &HashSet::new(),
        since_ms: None,
        keep: &all,
        after: None,
        limit: 30,
    });
    let want = fs::canonicalize(dir.path("main/projects/-w-a/one.jsonl")).unwrap();
    assert_eq!(page.items[0].path.as_deref(), want.to_str());
}
