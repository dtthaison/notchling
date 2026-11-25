use super::children::*;
use crate::sessions::testutil::TempDir;
use crate::transcript::codex::testutil::{tree, FORKED, OLD_FORK, PARENT, PLAIN};
use std::io::Write;
use std::path::PathBuf;

fn paths(dir: &TempDir) -> Vec<PathBuf> {
    [&PARENT, &FORKED, &PLAIN, &OLD_FORK]
        .iter()
        .map(|f| PathBuf::from(f.path_in(dir, "sessions")))
        .collect()
}

fn day(dir: &TempDir, d: &str) -> PathBuf {
    dir.path(&format!("sessions/2026/11/{d}"))
}

#[test]
fn a_top_level_rollout_is_a_row_with_no_parent() {
    let dir = tree("children-top");
    let all = paths(&dir);
    let index = Index::open_in_memory().unwrap();
    refresh(&index, &all);
    let head = head(&all[0]).unwrap();
    assert_eq!(head.thread, PARENT.thread);
    assert_eq!(head.parent, None);
    assert!(!index.is_child(PARENT.thread));
    assert!(index.is_child(FORKED.thread));
}

#[test]
fn children_are_found_by_the_parent_thread() {
    let dir = tree("children-found");
    let index = Index::open_in_memory().unwrap();
    refresh(&index, &paths(&dir));
    let found = index.children(PARENT.thread);
    let mut threads: Vec<&str> = found.iter().map(|h| h.thread.as_str()).collect();
    threads.sort_unstable();
    assert_eq!(threads, [FORKED.thread, PLAIN.thread, OLD_FORK.thread]);
    assert!(found.windows(2).all(|w| w[0].started <= w[1].started));
    let euler = found.iter().find(|h| h.thread == FORKED.thread).unwrap();
    assert_eq!(euler.depth, 1);
    assert_eq!(euler.nickname.as_deref(), Some("Euler"));
    assert_eq!(euler.role.as_deref(), Some("worker"));
    assert!(euler.started > 0.0);
    assert!(index.children(FORKED.thread).is_empty());
    assert!(index.children("").is_empty());
}

#[test]
fn a_changed_rollout_is_read_once_and_an_unchanged_one_never() {
    let dir = tree("children-stamp");
    let two = [
        PathBuf::from(PARENT.path_in(&dir, "sessions")),
        PathBuf::from(FORKED.path_in(&dir, "sessions")),
    ];
    let index = Index::open_in_memory().unwrap();
    assert_eq!(refresh(&index, &two), Refreshed { read: 2, kept: 0 });
    assert_eq!(refresh(&index, &two), Refreshed { read: 0, kept: 2 });
    let mut file = std::fs::OpenOptions::new()
        .append(true)
        .open(&two[1])
        .unwrap();
    writeln!(file, "{{\"type\":\"event_msg\",\"payload\":{{}}}}").unwrap();
    assert_eq!(refresh(&index, &two), Refreshed { read: 1, kept: 1 });
}

#[test]
fn a_gone_rollout_is_swept_when_its_day_is_read_again() {
    let dir = tree("children-sweep");
    let all = paths(&dir);
    let index = Index::open_in_memory().unwrap();
    refresh(&index, &all);
    std::fs::remove_file(&all[1]).unwrap();
    let present = rollout_files(&day(&dir, "13"));
    assert_eq!(index.sweep(&day(&dir, "13"), &present), 1);
    assert_eq!(index.children(PARENT.thread).len(), 2);
    assert_eq!(index.sweep(&day(&dir, "13"), &present), 0);
    assert_eq!(index.sweep(&day(&dir, "12"), &[]), 0);
}

#[test]
fn a_head_past_64_kb_is_not_read() {
    let dir = TempDir::new("children-long");
    let long = format!(
        "{{\"type\":\"session_meta\",\"payload\":{{\"id\":\"x\",\"pad\":\"{}\"}}}}\n",
        "a".repeat(70_000)
    );
    let file = dir.write(
        "sessions/2026/11/13/rollout-2026-11-13T09-00-00-x.jsonl",
        &long,
    );
    assert!(head(&file).is_none());
    let index = Index::open_in_memory().unwrap();
    assert_eq!(refresh(&index, &[file]).read, 1);
    assert!(!index.is_child("x"));
}

#[test]
fn the_backfill_reads_thirty_days_newest_first() {
    let dir = tree("children-days");
    for d in ["12", "01", "14"] {
        std::fs::create_dir_all(day(&dir, d)).unwrap();
    }
    std::fs::create_dir_all(dir.path("sessions/2026/09/01")).unwrap();
    let now_ms = crate::tools::codex::list::days_from_civil(2026, 11, 14) * 86_400_000 + 5_000;
    let found = day_folders(&dir.path("sessions"), now_ms, BACKFILL_DAYS);
    assert_eq!(
        found,
        [
            day(&dir, "14"),
            day(&dir, "13"),
            day(&dir, "12"),
            day(&dir, "01")
        ]
    );
    assert_eq!(BACKFILL_DAYS, 30);
    assert_eq!(day_folders(&dir.path("sessions"), now_ms, 2).len(), 2);
}

#[test]
fn day_folders_reads_two_folders_at_most() {
    let dir = tree("children-two");
    let next = FORKED
        .text
        .replace(FORKED.thread, "01a1d5da-0000-7000-8000-0000000000e1");
    dir.write(
        "sessions/2026/11/14/rollout-2026-11-14T00-00-00-01a1d5da-0000-7000-8000-0000000000e1.jsonl",
        &next,
    );
    let third = FORKED
        .text
        .replace(FORKED.thread, "01a1d5da-0000-7000-8000-0000000000e2");
    dir.write(
        "sessions/2026/11/15/rollout-2026-11-15T00-00-00-01a1d5da-0000-7000-8000-0000000000e2.jsonl",
        &third,
    );
    let parent = PathBuf::from(PARENT.path_in(&dir, "sessions"));
    let found = DayFolders.children(PARENT.thread, &parent);
    let mut threads: Vec<&str> = found.iter().map(|h| h.thread.as_str()).collect();
    threads.sort_unstable();
    assert_eq!(
        threads,
        [
            FORKED.thread,
            PLAIN.thread,
            OLD_FORK.thread,
            "01a1d5da-0000-7000-8000-0000000000e1"
        ]
    );
    assert!(DayFolders.children("", &parent).is_empty());
    assert!(DayFolders
        .children(PARENT.thread, std::path::Path::new("/nowhere/x.jsonl"))
        .is_empty());
}
