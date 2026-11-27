use super::*;
use crate::sessions::testutil::TempDir;
use std::fs::OpenOptions;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const DAY: f64 = 86_400.0;

fn now() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

fn write(dir: &TempDir, name: &str, word: &str, ago: f64) {
    let user = format!(
        r#"{{"type":"user","cwd":"/work/app","message":{{"role":"user","content":"talk about {word}"}}}}"#
    );
    let assistant = format!(
        r#"{{"type":"assistant","message":{{"role":"assistant","content":[{{"type":"text","text":"noted {word}"}}]}}}}"#
    );
    let path = dir.write(
        &format!("claude/projects/-work-app/{name}.jsonl"),
        &format!("{user}\n{assistant}\n"),
    );
    let file = OpenOptions::new().write(true).open(path).unwrap();
    file.set_modified(UNIX_EPOCH + Duration::from_secs_f64((now() - ago).max(0.0)))
        .unwrap();
}

fn roots(dir: &TempDir) -> IndexRoots {
    IndexRoots {
        claude: vec![ClaudeRoot {
            projects: dir.path("claude/projects"),
            config_dir: None,
        }],
        codex_sessions: None,
        titled: Vec::new(),
    }
}

fn found(index: &SessionIndex, word: &str) -> usize {
    index.search(&[word], now(), 30).len()
}

#[test]
fn a_session_older_than_14_days_is_found() {
    let dir = TempDir::new("idx-old");
    write(&dir, "recent", "zebrafish", DAY);
    write(&dir, "ancient", "quokka", 400.0 * DAY);
    let everything = SessionIndex::with_range(None, None);
    everything.refresh(&roots(&dir), now());
    assert_eq!(found(&everything, "quokka"), 1);
    assert_eq!(found(&everything, "zebrafish"), 1);
    let windowed = SessionIndex::with_range(None, Some(SESSION_WINDOW_SECS));
    windowed.refresh(&roots(&dir), now());
    assert_eq!(found(&windowed, "quokka"), 0);
    assert_eq!(found(&windowed, "zebrafish"), 1);
}

#[test]
fn indexed_since_is_null_when_everything_is_indexed() {
    let dir = TempDir::new("idx-since");
    write(&dir, "ancient", "quokka", 400.0 * DAY);
    let at = now();
    let everything = SessionIndex::with_range(None, None);
    assert_eq!(everything.indexed_since(at), Some(at));
    everything.refresh(&roots(&dir), at);
    assert_eq!(everything.indexed_since(at), None);
    let windowed = SessionIndex::with_range(None, Some(30.0 * DAY));
    windowed.refresh(&roots(&dir), at);
    assert_eq!(windowed.indexed_since(at), Some(at - 30.0 * DAY));
}

#[test]
fn a_second_pass_over_old_transcripts_reads_none_of_them() {
    let dir = TempDir::new("idx-stamps");
    for n in 0..5 {
        write(
            &dir,
            &format!("old-{n}"),
            "quokka",
            (100.0 + n as f64) * DAY,
        );
    }
    let index = SessionIndex::with_range(None, None);
    let first = index.refresh(&roots(&dir), now());
    assert_eq!(first.read, 5);
    let second = index.refresh(&roots(&dir), now());
    assert_eq!((second.read, second.reused), (0, 5));
}

#[test]
fn a_pass_is_saved_as_it_goes_so_a_restart_keeps_it() {
    let dir = TempDir::new("idx-saved");
    for n in 0..30 {
        write(
            &dir,
            &format!("old-{n}"),
            "quokka",
            (100.0 + n as f64) * DAY,
        );
    }
    let db = dir.path("search.db");
    {
        let index = SessionIndex::with_range(Some(db.clone()), None);
        index.refresh(&roots(&dir), now());
    }
    let again = SessionIndex::with_range(Some(db), None);
    again.load_cache();
    assert_eq!(again.len(), 30);
}

#[test]
#[ignore]
fn the_first_full_index_cost_is_measured() {
    let dir = TempDir::new("idx-cost");
    let count = 2000;
    let filler = "a line of ordinary talk about the build and the tests ".repeat(40);
    for n in 0..count {
        let mut lines = Vec::new();
        for turn in 0..20 {
            lines.push(format!(
                r#"{{"type":"user","cwd":"/work/app","message":{{"role":"user","content":"turn {turn} {filler}"}}}}"#
            ));
            lines.push(format!(
                r#"{{"type":"assistant","message":{{"role":"assistant","content":[{{"type":"text","text":"reply {turn} {filler}"}}]}}}}"#
            ));
        }
        let path = dir.write(
            &format!("claude/projects/-work-app/s{n}.jsonl"),
            &(lines.join("\n") + "\n"),
        );
        let file = OpenOptions::new().write(true).open(path).unwrap();
        file.set_modified(
            UNIX_EPOCH + Duration::from_secs_f64(now() - (30.0 + n as f64) * DAY / 10.0),
        )
        .unwrap();
    }
    let index = SessionIndex::with_range(Some(dir.path("search.db")), None);
    let start = std::time::Instant::now();
    let first = index.refresh(&roots(&dir), now());
    let first_time = start.elapsed();
    let start = std::time::Instant::now();
    let second = index.refresh(&roots(&dir), now());
    let second_time = start.elapsed();
    let bytes = std::fs::metadata(dir.path("search.db")).unwrap().len();
    println!(
        "COST {count} transcripts: first pass {first_time:?} (read {}), second pass {second_time:?} (read {}, reused {}), db {bytes} bytes",
        first.read, second.read, second.reused
    );
    assert_eq!(first.read, count);
    assert_eq!(second.read, 0);
}
