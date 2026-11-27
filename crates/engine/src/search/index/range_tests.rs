use super::*;
use crate::search::range::IndexRange;
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

fn write(dir: &TempDir, name: &str, body: &str, ago: f64) {
    write_lines(dir, name, &[body], ago);
}

fn write_lines(dir: &TempDir, name: &str, bodies: &[&str], ago: f64) {
    let lines: String = bodies
        .iter()
        .map(|body| {
            format!(
                r#"{{"type":"user","cwd":"/work/app","message":{{"role":"user","content":"{body}"}}}}"#
            ) + "\n"
        })
        .collect();
    let path = dir.write(&format!("claude/projects/-work-app/{name}.jsonl"), &lines);
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
fn the_setting_names_a_range() {
    assert_eq!(IndexRange::from_setting("14d"), IndexRange::Days14);
    assert_eq!(IndexRange::from_setting("30d"), IndexRange::Days30);
    assert_eq!(IndexRange::from_setting("all"), IndexRange::All);
    assert_eq!(IndexRange::from_setting("bogus"), IndexRange::All);
    assert_eq!(IndexRange::All.window_secs(), None);
    assert_eq!(IndexRange::Days14.window_secs(), Some(14.0 * DAY));
    assert_eq!(IndexRange::Days30.window_secs(), Some(30.0 * DAY));
    assert_eq!(IndexRange::default(), IndexRange::Days14);
}

#[test]
fn lowering_the_range_drops_older_rows_next_pass() {
    let dir = TempDir::new("range-lower");
    let db = dir.path("search.db");
    write(&dir, "recent", "zebrafish talk", DAY);
    write(&dir, "ancient", "quokka talk", 400.0 * DAY);
    let index = SessionIndex::with_range(Some(db.clone()), None);
    index.refresh(&roots(&dir), now());
    assert_eq!(found(&index, "quokka"), 1);
    index.set_range(IndexRange::Days14);
    let pass = index.refresh(&roots(&dir), now());
    assert_eq!(pass.dropped, 1);
    assert_eq!(found(&index, "quokka"), 0);
    assert_eq!(found(&index, "zebrafish"), 1);
    let reopened = SessionIndex::with_range(Some(db), Some(14.0 * DAY));
    reopened.load_cache();
    assert_eq!(found(&reopened, "quokka"), 0);
    assert_eq!(found(&reopened, "zebrafish"), 1);
}

#[test]
fn raising_the_range_reads_only_what_it_does_not_hold() {
    let dir = TempDir::new("range-raise");
    write(&dir, "recent", "zebrafish talk", DAY);
    write(&dir, "month", "narwhal talk", 20.0 * DAY);
    write(&dir, "ancient", "quokka talk", 400.0 * DAY);
    let index = SessionIndex::with_range(None, Some(14.0 * DAY));
    assert_eq!(index.refresh(&roots(&dir), now()).read, 1);
    index.set_range(IndexRange::Days30);
    let wider = index.refresh(&roots(&dir), now());
    assert_eq!((wider.read, wider.reused), (1, 1));
    assert_eq!(found(&index, "narwhal"), 1);
    index.set_range(IndexRange::All);
    let all = index.refresh(&roots(&dir), now());
    assert_eq!((all.read, all.reused), (1, 2));
    assert_eq!(index.indexed_since(now()), None);
}

#[test]
fn clear_empties_memory_and_disk() {
    let dir = TempDir::new("range-clear");
    let db = dir.path("search.db");
    write(&dir, "recent", "zebrafish talk", DAY);
    let index = SessionIndex::with_range(Some(db.clone()), None);
    index.refresh(&roots(&dir), now());
    assert_eq!(index.len(), 1);
    index.clear().unwrap();
    assert!(index.is_empty());
    assert_eq!(found(&index, "zebrafish"), 0);
    let reopened = SessionIndex::with_range(Some(db), None);
    reopened.load_cache();
    assert!(reopened.is_empty());
}

#[test]
fn memory_per_session_does_not_hold_its_text() {
    let dir = TempDir::new("range-memory");
    let filler = "lorem ipsum dolor sit amet ".repeat(180);
    let first = format!("the needle sits here {filler}");
    for n in 0..5 {
        let mut lines = vec![first.as_str()];
        lines.extend(std::iter::repeat_n(filler.as_str(), 7));
        write_lines(&dir, &format!("s{n}"), &lines, DAY);
    }
    let index = SessionIndex::with_range(None, None);
    index.refresh(&roots(&dir), now());
    assert_eq!(index.len(), 5);
    assert!(
        index.resident_bytes() < 5 * 1_000,
        "held {} bytes",
        index.resident_bytes()
    );
    let hits = index.search(&["needle"], now(), 30);
    assert_eq!(hits.len(), 5);
    assert!(hits
        .iter()
        .all(|h| h.snippet.as_deref().is_some_and(|s| s.contains("needle"))));
    let middle = index.search(&["eedl"], now(), 30);
    assert_eq!(middle.len(), 5);
    assert!(middle[0].snippet.as_deref().unwrap().contains("needle"));
}

fn agy_roots(dir: &TempDir) -> IndexRoots {
    let mut r = roots(dir);
    r.titled = vec![
        TitledSession {
            id: "fresh".into(),
            title: "Plan the notch".into(),
            folder: "/work/app".into(),
            modified: now() - DAY,
        },
        TitledSession {
            id: "ancient".into(),
            title: "Plan the dock".into(),
            folder: "/work/app".into(),
            modified: now() - 400.0 * DAY,
        },
    ];
    r
}

#[test]
fn antigravity_rows_follow_the_range() {
    let dir = TempDir::new("range-agy");
    let index = SessionIndex::with_range(Some(dir.path("search.db")), Some(14.0 * DAY));
    index.refresh(&agy_roots(&dir), now());
    assert!(index.meta_of("agy:fresh").is_some());
    assert!(index.meta_of("agy:ancient").is_none());

    let wide = SessionIndex::with_range(Some(dir.path("wide.db")), None);
    wide.refresh(&agy_roots(&dir), now());
    assert!(wide.meta_of("agy:ancient").is_some());
    wide.set_range(IndexRange::Days14);
    let pass = wide.refresh(&agy_roots(&dir), now());
    assert_eq!(pass.dropped, 1);
    assert!(wide.meta_of("agy:ancient").is_none());
    assert!(wide.meta_of("agy:fresh").is_some());
}

#[test]
fn a_clear_during_a_refresh_is_not_undone_by_it() {
    let dir = TempDir::new("range-fence");
    let db = dir.path("search.db");
    write(&dir, "one", "zebrafish talk", DAY);
    write(&dir, "two", "quokka talk", DAY);
    let index = SessionIndex::with_range(Some(db.clone()), None);
    let pass = index.refresh_with(&roots(&dir), now(), &|| index.clear().unwrap());
    assert_eq!(pass.read, 2);
    assert!(index.is_empty());
    assert_eq!(found(&index, "zebrafish"), 0);
    let reopened = SessionIndex::with_range(Some(db), None);
    reopened.load_cache();
    assert!(reopened.is_empty());
    index.refresh(&roots(&dir), now());
    assert_eq!(index.len(), 2);
}
