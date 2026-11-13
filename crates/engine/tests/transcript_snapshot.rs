mod common;

use common::*;
use notchling_engine::transcript::errors::limit_hit_in_file;
use notchling_engine::transcript::locate::{
    job_transcript, predecessor, project_folder, successor,
};
use notchling_engine::transcript::snapshot::{self, LineKind};
use notchling_engine::transcript::tail::{Cursors, LineParser};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

#[test]
fn notch_snapshot() {
    let snap = snapshot::read(&s(&notchling_fixture("notch.jsonl")));
    assert_eq!(
        snap.lines.iter().map(|l| l.kind).collect::<Vec<_>>(),
        [
            LineKind::User,
            LineKind::Tools,
            LineKind::Assistant,
            LineKind::Tools,
            LineKind::Assistant,
            LineKind::User,
            LineKind::Tools
        ]
    );
    assert_eq!(
        snap.lines
            .iter()
            .map(|l| l.text.as_str())
            .collect::<Vec<_>>(),
        [
            "Find the config [Pasted text +2 lines]",
            "Read 2 files",
            "I'll look.",
            "Searched for 1 pattern, used search",
            "Found it in b.swift.\n\nSecond paragraph.",
            "Now fix it",
            "Edited 1 file, asked you",
        ]
    );
    assert_eq!(
        snap.lines.iter().map(|l| l.id).collect::<Vec<_>>(),
        (0..7).collect::<Vec<_>>()
    );
    assert_eq!(snap.last_prompt.as_deref(), Some("Now fix it"));
    assert!(snap.activity.is_some());
    let q = snap.question.expect("question");
    assert_eq!(q.tool_use_id, "q1");
    assert_eq!(q.items[0].options, ["Yes", "No"]);
    assert_eq!(q.items[0].details, ["both files", ""]);
    assert!(q.answerable());
}

#[test]
fn appending_line_by_line_matches_a_fresh_read() {
    let scratch = Scratch::new("snap");
    let path = scratch.write("live.jsonl", "");
    for line in lines_of(&notchling_fixture("notch.jsonl")) {
        let mid = line
            .char_indices()
            .nth(line.chars().count() / 2)
            .map(|(i, _)| i)
            .unwrap_or(0);
        scratch.append("live.jsonl", &line[..mid]);
        let _ = snapshot::read(&s(&path));
        scratch.append("live.jsonl", &format!("{}\n", &line[mid..]));
        let fresh = scratch.path(&format!("fresh-{}.jsonl", line.len()));
        std::fs::copy(&path, &fresh).expect("copy");
        assert_eq!(
            snapshot::read(&s(&path)),
            snapshot::read(&s(&fresh)),
            "after: {}",
            &line[..line.len().min(60)]
        );
    }
    assert_eq!(snapshot::read(&s(&path)).lines.len(), 7);
}

#[test]
fn replaced_or_truncated_file_is_read_again() {
    let lines = lines_of(&notchling_fixture("notch.jsonl"));
    let scratch = Scratch::new("trunc");
    let path = scratch.write("t.jsonl", &(lines.join("\n") + "\n"));
    assert_eq!(snapshot::read(&s(&path)).lines.len(), 7);

    scratch.write("t.jsonl", &(lines[0].clone() + "\n"));
    assert_eq!(
        snapshot::read(&s(&path))
            .lines
            .iter()
            .map(|l| l.kind)
            .collect::<Vec<_>>(),
        [LineKind::User]
    );

    std::fs::remove_file(&path).expect("remove");
    scratch.write("t.jsonl", &(lines[9].clone() + "\n"));
    assert_eq!(
        snapshot::read(&s(&path)).last_prompt.as_deref(),
        Some("Now fix it")
    );
}

#[test]
fn long_assistant_runs_stay_bounded() {
    let scratch = Scratch::new("bounded");
    let path = scratch.write("long.jsonl", "");
    let chunk = "x".repeat(900);
    for i in 0..40 {
        scratch.append(
            "long.jsonl",
            &format!(r#"{{"type":"assistant","message":{{"content":[{{"type":"text","text":"{i} {chunk}"}}]}}}}"#),
        );
        scratch.append("long.jsonl", "\n");
        let _ = snapshot::read(&s(&path));
    }
    let live = snapshot::read(&s(&path));
    let fresh = scratch.path("fresh.jsonl");
    std::fs::copy(&path, &fresh).expect("copy");
    assert_eq!(live, snapshot::read(&s(&fresh)));
    assert_eq!(
        live.lines[0].text.chars().count(),
        3001,
        "shown as … and the last 3000 characters"
    );
}

#[test]
fn limit_hit() {
    let hit = limit_hit_in_file(&s(&notchling_fixture("limit-hit.jsonl"))).expect("hit");
    assert_eq!(hit.id, "u-limit");
    assert!(hit.text.starts_with("You've hit your session limit"));
    assert!(hit.at.is_some());
}

#[test]
fn a_429_is_not_a_limit_hit() {
    assert!(limit_hit_in_file(&s(&notchling_fixture("rate-limited-429.jsonl"))).is_none());
}

fn cleared(agent: &str) -> String {
    format!(r#"{{"type":"system","agentName":"{agent}","content":"SessionStart:clear"}}"#) + "\n"
}

fn tick() {
    std::thread::sleep(Duration::from_millis(50));
}

#[test]
fn finds_the_transcript_started_by_clear() {
    let scratch = Scratch::new("succ");
    let old = s(&scratch.write("old.jsonl", "{}\n"));
    tick();
    let new = s(&scratch.write("new.jsonl", &cleared("worker")));
    assert_eq!(successor(&old, "worker"), Some(new));
    assert_eq!(successor(&old, "someone-else"), None);
}

#[test]
fn a_new_file_is_seen_after_a_cached_miss() {
    let scratch = Scratch::new("succ");
    let old = s(&scratch.write("old.jsonl", "{}\n"));
    assert_eq!(successor(&old, "worker"), None);
    tick();
    tick();
    let new = s(&scratch.write("new.jsonl", &cleared("worker")));
    assert_eq!(
        successor(&old, "worker"),
        Some(new),
        "creating a file must invalidate the cache"
    );
}

#[test]
fn removed_successor_is_dropped() {
    let scratch = Scratch::new("succ");
    let old = s(&scratch.write("old.jsonl", "{}\n"));
    tick();
    let new = s(&scratch.write("new.jsonl", &cleared("worker")));
    assert_eq!(successor(&old, "worker"), Some(new.clone()));
    tick();
    std::fs::remove_file(&new).expect("remove");
    assert_eq!(successor(&old, "worker"), None);
}

#[test]
fn follows_several_clears_in_a_row() {
    let scratch = Scratch::new("succ");
    let old = s(&scratch.write("old.jsonl", "{}\n"));
    tick();
    scratch.write("b.jsonl", &cleared("worker"));
    tick();
    let c = s(&scratch.write("c.jsonl", &cleared("worker")));
    assert_eq!(successor(&old, "worker"), Some(c));
}

#[test]
fn a_name_with_quotes_is_matched_as_json() {
    let scratch = Scratch::new("succ-quote");
    let old = s(&scratch.write("old.jsonl", "{}\n"));
    tick();
    let new = s(&scratch.write("new.jsonl", &cleared(r#"say \"hi\""#)));
    assert_eq!(successor(&old, "say \"hi\""), Some(new));
}

#[test]
fn two_matching_files_are_followed_oldest_created_first() {
    let scratch = Scratch::new("succ-order");
    let old = s(&scratch.write("old.jsonl", "{}\n"));
    tick();
    scratch.write("z.jsonl", &cleared("worker"));
    tick();
    let newest = s(&scratch.write("a.jsonl", &cleared("worker")));
    assert_eq!(successor(&old, "worker"), Some(newest));
}

#[test]
fn folder_names() {
    assert_eq!(
        project_folder("/Users/sondoan/go/src/pp/notchling"),
        "-Users-sondoan-go-src-pp-notchling"
    );
    assert_eq!(
        project_folder("/Users/sondoan/.claude/jobs/45ee33db/tmp"),
        "-Users-sondoan--claude-jobs-45ee33db-tmp"
    );
    assert_eq!(
        project_folder("/private/tmp/claude-501/x_y"),
        "-private-tmp-claude-501-x-y"
    );
}

const START: &str = "/h/.claude/projects/-Users-me-app/abc.jsonl";
const MOVED: &str = "/h/.claude/projects/-Users-me-app--claude-worktrees-music/abc.jsonl";
const WORKTREE: &str = "/Users/me/app/.claude/worktrees/music";

#[test]
fn follows_the_session_into_its_worktree() {
    let found = job_transcript(Some(START), Some("abc"), Some(WORKTREE), |p| {
        (p == MOVED).then_some(1.0)
    });
    assert_eq!(found.as_deref(), Some(MOVED));
}

#[test]
fn the_newer_file_wins_when_both_exist() {
    let dates = |p: &str| match p {
        START => Some(100.0),
        MOVED => Some(200.0),
        _ => None,
    };
    assert_eq!(
        job_transcript(Some(START), Some("abc"), Some(WORKTREE), dates).as_deref(),
        Some(MOVED)
    );
    let older = |p: &str| match p {
        START => Some(300.0),
        MOVED => Some(200.0),
        _ => None,
    };
    assert_eq!(
        job_transcript(Some(START), Some("abc"), Some(WORKTREE), older).as_deref(),
        Some(START)
    );
}

#[test]
fn equal_dates_keep_the_recorded_path() {
    assert_eq!(
        job_transcript(Some(START), Some("abc"), Some(WORKTREE), |_| Some(7.0)).as_deref(),
        Some(START)
    );
}

#[test]
fn without_a_worktree_it_is_the_recorded_path() {
    assert_eq!(
        job_transcript(Some(START), Some("abc"), None, |_| None).as_deref(),
        Some(START)
    );
    assert_eq!(
        job_transcript(None, Some("abc"), Some("/w"), |_| Some(1.0)),
        None
    );
}

#[derive(Default)]
struct LineCount {
    lines: usize,
}

impl LineParser for LineCount {
    fn consume(&mut self, _line: &[u8]) {
        self.lines += 1;
    }
}

fn counting(limit: usize) -> (Cursors<LineCount, usize>, Arc<AtomicUsize>) {
    let renders = Arc::new(AtomicUsize::new(0));
    let seen = renders.clone();
    let cursors = Cursors::new(1_000_000, limit, move |p: &LineCount| {
        seen.fetch_add(1, Ordering::SeqCst);
        p.lines
    });
    (cursors, renders)
}

#[test]
fn full_cache_drops_only_the_least_recently_read() {
    let scratch = Scratch::new("cursors");
    let a = s(&scratch.write("a.jsonl", "{}\n"));
    let b = s(&scratch.write("b.jsonl", "{}\n"));
    let c = s(&scratch.write("c.jsonl", "{}\n"));
    let (cursors, renders) = counting(2);
    let n = || renders.load(Ordering::SeqCst);

    assert_eq!(cursors.read(&a), Some(1));
    assert_eq!(cursors.read(&b), Some(1));
    assert_eq!(cursors.read(&a), Some(1));
    assert_eq!(n(), 2);
    assert_eq!(cursors.read(&c), Some(1));
    assert_eq!(n(), 3);
    assert_eq!(cursors.read(&a), Some(1));
    assert_eq!(n(), 3, "a was kept");
    assert_eq!(cursors.read(&b), Some(1));
    assert_eq!(n(), 4, "b was dropped and read again");
}

#[test]
fn replacing_a_file_in_a_full_cache_drops_nothing_else() {
    let scratch = Scratch::new("cursors");
    let a = s(&scratch.write("a.jsonl", "{}\n"));
    let b = s(&scratch.write("b.jsonl", "{}\n"));
    let (cursors, renders) = counting(2);
    let n = || renders.load(Ordering::SeqCst);
    let _ = cursors.read(&a);
    let _ = cursors.read(&b);
    std::fs::remove_file(&a).expect("remove");
    scratch.write("a.jsonl", "{}\n{}\n");
    assert_eq!(cursors.read(&a), Some(2));
    assert_eq!(n(), 3);
    assert_eq!(cursors.read(&b), Some(1));
    assert_eq!(n(), 3, "b was kept");
}

#[test]
fn a_half_written_line_waits_for_its_newline() {
    let scratch = Scratch::new("cursors");
    let a = s(&scratch.write("a.jsonl", "{}\n{"));
    let (cursors, _) = counting(2);
    assert_eq!(cursors.read(&a), Some(1));
    scratch.append("a.jsonl", "}\n");
    assert_eq!(cursors.read(&a), Some(2));
}

#[test]
fn unreadable_file_is_none() {
    let (cursors, _) = counting(2);
    assert_eq!(cursors.read("/nonexistent/x.jsonl"), None);
}

fn named(agent: &str) -> String {
    format!(r#"{{"type":"agent-name","agentName":"{agent}"}}"#) + "\n"
}

#[test]
fn a_cleared_transcript_finds_the_one_the_clear_ended() {
    let scratch = Scratch::new("pred");
    scratch.write("older.jsonl", &named("worker"));
    tick();
    let old = s(&scratch.write("old.jsonl", &named("worker")));
    tick();
    scratch.write("other.jsonl", &named("someone-else"));
    tick();
    let new = s(&scratch.write("new.jsonl", &cleared("worker")));
    assert_eq!(predecessor(&new, "worker"), Some(old.clone()));
    assert_eq!(predecessor(&old, "worker"), None);
    assert_eq!(predecessor(&new, "nobody"), None);
}
