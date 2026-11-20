use super::*;
use std::cell::Cell;

const ROW: &str = "row-1";

fn read(watch: &mut AnswerWatch, status: &str, now_ms: i64, seen: Seen) -> bool {
    watch.open(ROW, Some(status), now_ms, || seen)
}

#[test]
fn unknown_reads_never_settle_the_row() {
    let mut watch = AnswerWatch::default();
    for step in 0..200 {
        let now = step * ANSWER_RECHECK_MS;
        assert!(read(&mut watch, "idle", now, Seen::Unknown), "read {step}");
    }
}

#[test]
fn one_read_without_a_dialog_is_not_enough() {
    let mut watch = AnswerWatch::default();
    let mut now = 0;
    for _ in 1..ANSWER_CLOSED_READS {
        assert!(read(&mut watch, "idle", now, Seen::Closed));
        now += 1_000;
    }
    assert!(
        !read(&mut watch, "idle", now, Seen::Closed),
        "settled after {ANSWER_CLOSED_READS} reads in a row"
    );
}

#[test]
fn a_dialog_seen_again_starts_the_count_over() {
    let mut watch = AnswerWatch::default();
    assert!(read(&mut watch, "idle", 0, Seen::Closed));
    assert!(read(&mut watch, "idle", 1_000, Seen::Closed));
    assert!(read(&mut watch, "idle", 2_000, Seen::Open));
    let mut now = 2_000 + ANSWER_RECHECK_MS;
    for _ in 1..ANSWER_CLOSED_READS {
        assert!(read(&mut watch, "idle", now, Seen::Closed));
        now += 1_000;
    }
    assert!(!read(&mut watch, "idle", now, Seen::Closed));
}

#[test]
fn an_unknown_read_neither_counts_nor_starts_over() {
    let mut watch = AnswerWatch::default();
    assert!(read(&mut watch, "idle", 0, Seen::Closed));
    assert!(read(&mut watch, "idle", 1_000, Seen::Closed));
    assert!(read(&mut watch, "idle", 2_000, Seen::Unknown));
    let next = 2_000 + ANSWER_RECHECK_MS;
    assert_eq!(ANSWER_CLOSED_READS, 3);
    assert!(!read(&mut watch, "idle", next, Seen::Closed));
}

#[test]
fn waiting_then_idle_with_no_dialog_on_screen_is_answered() {
    let mut watch = AnswerWatch::default();
    assert!(read(&mut watch, "waiting", 0, Seen::Open));
    assert!(read(&mut watch, "waiting", 10_000, Seen::Open));
    assert!(!read(&mut watch, "idle", 11_000, Seen::Closed));
}

#[test]
fn waiting_then_idle_still_needs_the_screen_to_agree() {
    let mut watch = AnswerWatch::default();
    assert!(read(&mut watch, "waiting", 0, Seen::Open));
    assert!(read(&mut watch, "idle", 1_000, Seen::Open));
    assert!(read(
        &mut watch,
        "idle",
        1_000 + ANSWER_RECHECK_MS,
        Seen::Unknown
    ));
}

#[test]
fn waiting_then_busy_counts_reads_as_usual() {
    let mut watch = AnswerWatch::default();
    assert!(read(&mut watch, "waiting", 0, Seen::Open));
    assert!(read(&mut watch, "busy", 1_000, Seen::Closed));
}

#[test]
fn the_screen_is_not_read_while_the_session_waits() {
    let mut watch = AnswerWatch::default();
    let looked = Cell::new(0);
    let look = || {
        looked.set(looked.get() + 1);
        Seen::Open
    };
    assert!(watch.open(ROW, Some("waiting"), 0, look));
    assert_eq!(looked.get(), 0);
}

#[test]
fn a_dialog_seen_or_a_failed_read_is_not_read_again_for_a_while() {
    for first in [Seen::Open, Seen::Unknown] {
        let mut watch = AnswerWatch::default();
        let looked = Cell::new(0);
        let look = || {
            looked.set(looked.get() + 1);
            first
        };
        let mut now = 0;
        while now < ANSWER_RECHECK_MS {
            assert!(watch.open(ROW, Some("idle"), now, look));
            now += 1_000;
        }
        assert_eq!(
            looked.get(),
            1,
            "{first:?} read once in {ANSWER_RECHECK_MS} ms"
        );
        assert!(watch.open(ROW, Some("idle"), now, look));
        assert_eq!(looked.get(), 2);
    }
}

#[test]
fn another_row_starts_a_fresh_watch() {
    let mut watch = AnswerWatch::default();
    assert!(read(&mut watch, "idle", 0, Seen::Closed));
    assert!(read(&mut watch, "idle", 1_000, Seen::Closed));
    assert!(watch.open("row-2", Some("idle"), 2_000, || Seen::Closed));
}
