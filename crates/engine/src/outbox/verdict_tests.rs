use super::*;
use crate::transcript::sent::Seen;

fn seen(enqueued: usize, taken: usize) -> Option<Seen> {
    Some(Seen { enqueued, taken })
}

const BEFORE: Seen = Seen {
    enqueued: 1,
    taken: 1,
};

#[test]
fn new_taken_copy_is_sent() {
    assert_eq!(verdict(seen(1, 2), BEFORE, 2_000), Verdict::Sent);
}

#[test]
fn enqueue_only_is_in_queue() {
    assert_eq!(verdict(seen(2, 1), BEFORE, 2_000), Verdict::InQueue);
}

#[test]
fn taken_beats_enqueued() {
    assert_eq!(verdict(seen(2, 2), BEFORE, 2_000), Verdict::Sent);
}

#[test]
fn no_copy_at_29s_is_pending() {
    assert_eq!(verdict(seen(1, 1), BEFORE, 29_000), Verdict::Pending);
}

#[test]
fn no_copy_at_30s_is_unsure() {
    assert_eq!(verdict(seen(1, 1), BEFORE, VERIFY_LOOK_MS), Verdict::Unsure);
}

#[test]
fn unknown_window_is_unsure_at_30s() {
    assert_eq!(verdict(None, BEFORE, 29_999), Verdict::Pending);
    assert_eq!(verdict(None, BEFORE, VERIFY_LOOK_MS), Verdict::Unsure);
}

#[test]
fn line_within_needs_a_new_copy() {
    assert!(line_within(seen(2, 1), BEFORE));
    assert!(line_within(seen(1, 2), BEFORE));
    assert!(!line_within(seen(1, 1), BEFORE));
    assert!(!line_within(None, BEFORE));
}

#[test]
fn taken_line_makes_in_queue_sent() {
    for listed in [true, false] {
        assert_eq!(
            queued_verdict(seen(1, 2), BEFORE, listed, Some(0), 1_000),
            QueuedVerdict::Sent
        );
    }
}

#[test]
fn still_listed_stays() {
    assert_eq!(
        queued_verdict(seen(1, 1), BEFORE, true, None, 10_000_000),
        QueuedVerdict::Stay
    );
    assert_eq!(
        queued_verdict(seen(1, 1), BEFORE, true, Some(0), 10_000_000),
        QueuedVerdict::Stay
    );
}

#[test]
fn left_queue_untaken_for_30s_is_unsure_dropped() {
    let t = 5_000;
    assert_eq!(
        queued_verdict(seen(1, 1), BEFORE, false, Some(t), t + 29_999),
        QueuedVerdict::Stay
    );
    assert_eq!(
        queued_verdict(seen(1, 1), BEFORE, false, Some(t), t + VERIFY_LOOK_MS),
        QueuedVerdict::Unsure
    );
    assert_eq!(
        queued_verdict(seen(1, 1), BEFORE, false, None, t + VERIFY_LOOK_MS),
        QueuedVerdict::Stay
    );
}

#[test]
fn none_is_never_a_take() {
    assert_eq!(
        queued_verdict(None, BEFORE, false, Some(0), VERIFY_LOOK_MS),
        QueuedVerdict::Unsure
    );
    assert_eq!(
        queued_verdict(None, BEFORE, true, Some(0), VERIFY_LOOK_MS),
        QueuedVerdict::Stay
    );
}
