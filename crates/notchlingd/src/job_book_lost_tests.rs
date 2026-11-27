use super::*;
use std::sync::atomic::AtomicU32;

fn record(started: i64) -> Record {
    Record {
        session: "s".to_string(),
        organization: "Personal".to_string(),
        kind: "delegate".to_string(),
        tool: "agy".to_string(),
        folder: "/work/a".to_string(),
        started_ms: started,
        ended_ms: None,
        exit: None,
    }
}

fn file(tag: &str) -> PathBuf {
    static NEXT: AtomicU32 = AtomicU32::new(0);
    let n = NEXT.fetch_add(1, Ordering::SeqCst);
    let dir = std::env::temp_dir().join(format!("nl-book-{tag}-{}-{n}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir.join("jobs.json")
}

#[test]
fn a_book_saved_before_pids_and_lost_marks_still_opens() {
    let path = file("legacy");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let old = r#"{"jobs":{"d-1":{"session":"s","organization":"Personal","kind":"delegate","tool":"agy","folder":"/work/a","started_ms":5}}}"#;
    std::fs::write(&path, old).unwrap();
    let book = JobBook::open(path);
    assert_eq!(book.get("d-1").unwrap().started_ms, 5);
    assert_eq!(book.unfinished("delegate").len(), 1);
    assert!(!book.is_lost("d-1"));
}

#[test]
fn a_lost_job_and_a_process_survive_a_restart_and_a_normal_end_drops_the_process() {
    let path = file("kept");
    let book = JobBook::open(path.clone());
    book.record_with_pid("d-a", record(1), Some(77));
    book.record_with_pid("d-b", record(2), Some(78));
    assert!(book.end_lost("d-a", 9));
    assert!(book.end("d-b", 10, Some(0)));
    assert!(!book.end_lost("d-b", 11));
    let again = JobBook::open(path);
    assert!(again.is_lost("d-a"));
    assert!(!again.is_lost("d-b"));
    assert_eq!(again.get("d-a").unwrap().ended_ms, Some(9));
    assert_eq!(again.get("d-a").unwrap().exit, None);
    assert!(again.unfinished("delegate").is_empty());
}

#[test]
fn only_a_job_without_an_end_is_unfinished_and_it_carries_its_process() {
    let book = JobBook::in_memory();
    book.record_with_pid("d-a", record(1), Some(77));
    book.record("d-b", record(2));
    let mut open = book.unfinished("delegate");
    open.sort_by(|a, b| a.0.cmp(&b.0));
    assert_eq!(open[0].2, Some(77));
    assert_eq!(open[1].2, None);
    assert!(book.unfinished("dispatch").is_empty());
}
