use serde_json::json;

use super::held::{Found, Held, HeldQueue, HeldTarget, HELD_STALE_MS, MAX_DELIVERED};

fn held(id: &str, device: &str, request: &str, at_ms: i64) -> Held {
    Held {
        id: id.to_string(),
        op: "session.reply".to_string(),
        args: json!({"text": id}),
        target: HeldTarget::Session("s1".to_string()),
        device: device.to_string(),
        request: request.to_string(),
        at_ms,
    }
}

fn ids(items: &[Held]) -> Vec<&str> {
    items.iter().map(|h| h.id.as_str()).collect()
}

#[test]
fn unreachable_primary_holds_in_order() {
    let mut queue = HeldQueue::default();
    for (n, at) in [("a", 10), ("b", 20), ("c", 30)] {
        assert_eq!(queue.hold(held(n, "d1", n, at)), n);
    }
    assert_eq!(ids(&queue.list()), ["a", "b", "c"]);
    let taken = queue.take_all();
    assert_eq!(ids(&taken), ["a", "b", "c"]);
    assert!(queue.list().is_empty());
    assert!(queue.take_all().is_empty());
}

#[test]
fn same_request_twice_is_held_once() {
    let mut queue = HeldQueue::default();
    assert_eq!(queue.hold(held("a", "d1", "r1", 10)), "a");
    assert_eq!(queue.hold(held("b", "d1", "r1", 20)), "a");
    assert_eq!(queue.hold(held("c", "d2", "r1", 30)), "c");
    assert_eq!(ids(&queue.list()), ["a", "c"]);
    assert_eq!(queue.list()[0].at_ms, 10);
}

#[test]
fn a_request_repeated_after_delivery_is_not_held_again() {
    let mut queue = HeldQueue::default();
    queue.hold(held("a", "d1", "r1", 10));
    queue.take_all();
    assert_eq!(queue.hold(held("b", "d1", "r1", 20)), "a");
    assert!(queue.list().is_empty());
    assert_eq!(queue.hold(held("c", "d1", "r2", 30)), "c");
}

#[test]
fn held_over_an_hour_waits_for_confirm() {
    let mut queue = HeldQueue::default();
    queue.hold(held("old", "d1", "r1", 0));
    queue.hold(held("new", "d1", "r2", 1_000));
    let now = HELD_STALE_MS;
    assert!(queue.waiting(now).is_empty());
    let now = HELD_STALE_MS + 1;
    assert_eq!(ids(&queue.waiting(now)), ["old"]);
    assert_eq!(ids(&queue.take_due(now)), ["new"]);
    assert_eq!(ids(&queue.list()), ["old"]);
    assert_eq!(ids(&queue.waiting(now)), ["old"]);
    assert!(queue.take_due(now).is_empty());
}

#[test]
fn confirm_sends_a_waiting_entry() {
    let mut queue = HeldQueue::default();
    queue.hold(held("old1", "d1", "r1", 0));
    queue.hold(held("old2", "d1", "r2", 1));
    queue.hold(held("new", "d1", "r3", HELD_STALE_MS));
    let now = HELD_STALE_MS + 5;
    assert!(queue.confirm("old2"));
    assert!(!queue.confirm("nothing"));
    assert_eq!(ids(&queue.waiting(now)), ["old1"]);
    assert_eq!(ids(&queue.take_due(now)), ["new", "old2"]);
    assert_eq!(ids(&queue.list()), ["old1"]);
    assert!(queue.confirm("old1"));
    assert_eq!(ids(&queue.take_due(now)), ["old1"]);
    assert!(queue.list().is_empty());
}

#[test]
fn waiting_entry_cancels() {
    let mut queue = HeldQueue::default();
    queue.hold(held("old", "d1", "r1", 0));
    let now = HELD_STALE_MS + 1;
    assert_eq!(ids(&queue.waiting(now)), ["old"]);
    assert!(queue.cancel("old"));
    assert!(queue.waiting(now).is_empty());
    assert!(queue.take_due(now).is_empty());
    assert!(!queue.was_delivered("old"));
}

#[test]
fn cancel_removes_a_held_command() {
    let mut queue = HeldQueue::default();
    queue.hold(held("a", "d1", "r1", 1));
    queue.hold(held("b", "d1", "r2", 2));
    assert!(queue.cancel("a"));
    assert_eq!(ids(&queue.list()), ["b"]);
    assert!(!queue.cancel("a"));
    assert!(!queue.cancel("never"));
    assert!(!queue.was_delivered("a"));
}

#[test]
fn cancel_after_delivery_reports_delivered() {
    let mut queue = HeldQueue::default();
    queue.hold(held("a", "d1", "r1", 1));
    queue.hold(held("b", "d1", "r2", 2));
    queue.take_due(3);
    assert!(!queue.cancel("a"));
    assert!(queue.was_delivered("a"));
    assert!(queue.was_delivered("b"));
    assert!(!queue.was_delivered("c"));
}

#[test]
fn the_delivered_record_keeps_only_the_newest() {
    let mut queue = HeldQueue::default();
    for n in 0..MAX_DELIVERED + 2 {
        queue.hold(held(
            &format!("h{n}"),
            &format!("d{}", n % 5),
            &format!("r{n}"),
            1,
        ));
        queue.take_all();
    }
    assert!(!queue.was_delivered("h0"));
    assert!(!queue.was_delivered("h1"));
    assert!(queue.was_delivered("h2"));
    assert!(queue.was_delivered(&format!("h{}", MAX_DELIVERED + 1)));
}

#[test]
fn take_all_sends_a_waiting_entry_too() {
    let mut queue = HeldQueue::default();
    queue.hold(held("old", "d1", "r1", 0));
    queue.hold(held("new", "d1", "r2", HELD_STALE_MS + 9));
    assert_eq!(ids(&queue.take_all()), ["old", "new"]);
}

#[test]
fn kept_back_entries_return_to_the_front_in_order() {
    let mut queue = HeldQueue::default();
    for (n, at) in [("a", 1), ("b", 2), ("c", 3)] {
        queue.hold(held(n, "d1", n, at));
    }
    let due = queue.take_due(5);
    assert_eq!(ids(&due), ["a", "b", "c"]);
    queue.hold(held("d", "d1", "d", 6));
    queue.keep_back(vec![due[0].clone(), due[2].clone()], 7);
    assert_eq!(ids(&queue.list()), ["a", "c", "d"]);
    assert!(!queue.was_delivered("a"));
    assert!(queue.was_delivered("b"));
    assert_eq!(queue.hold(held("a2", "d1", "a", 8)), "a");
    assert_eq!(ids(&queue.take_due(9)), ["a", "c", "d"]);
}

#[test]
fn a_kept_back_stale_entry_stays_confirmed() {
    let mut queue = HeldQueue::default();
    queue.hold(held("old", "d1", "r1", 0));
    let now = HELD_STALE_MS + 1;
    queue.confirm("old");
    let due = queue.take_due(now);
    assert_eq!(ids(&due), ["old"]);
    queue.keep_back(due, now);
    assert!(queue.waiting(now).is_empty());
    assert_eq!(ids(&queue.take_due(now)), ["old"]);
}

#[test]
fn absorb_adds_the_entries_it_lacks_and_the_delivered_record() {
    let mut mine = HeldQueue::default();
    mine.hold(held("a", "d1", "r1", 1));
    mine.hold(held("b", "d1", "r2", 2));
    mine.take_due(3);
    mine.hold(held("c", "d1", "r3", 4));
    let mut theirs = HeldQueue::default();
    theirs.hold(held("x", "d2", "r1", 5));
    theirs.hold(held("c", "d1", "r3", 4));
    theirs.absorb(&mine);
    assert_eq!(ids(&theirs.list()), ["x", "c"]);
    assert!(theirs.was_delivered("a"));
    assert!(theirs.was_delivered("b"));
    assert_eq!(theirs.hold(held("a2", "d1", "r1", 6)), "a");
}

#[test]
fn absorb_keeps_a_waiting_entry_confirmed() {
    let mut mine = HeldQueue::default();
    mine.hold(held("old", "d1", "r1", 0));
    mine.confirm("old");
    let mut theirs = HeldQueue::default();
    theirs.absorb(&mine);
    assert_eq!(ids(&theirs.list()), ["old"]);
    let now = HELD_STALE_MS + 1;
    assert!(theirs.waiting(now).is_empty());
}

#[test]
fn find_answers_from_the_held_and_the_delivered() {
    let mut queue = HeldQueue::default();
    queue.hold(held("a", "d1", "k:one", 10));
    queue.hold(held("b", "d1", "k:two", 20));
    assert!(matches!(queue.find("d1", "k:one"), Some(Found::Held(h)) if h.id == "a"));
    assert_eq!(
        queue.find("d2", "k:one"),
        None,
        "another phone's key is another command"
    );
    let due = queue.take_due(30);
    assert_eq!(ids(&due), ["a", "b"]);
    assert_eq!(
        queue.find("d1", "k:one"),
        Some(Found::Delivered("a".to_string()))
    );
    assert_eq!(queue.find("d1", "k:nine"), None);
}

#[test]
fn a_command_sent_straight_is_remembered_for_its_retry() {
    let mut queue = HeldQueue::default();
    queue.note_sent("x1", "d1", "k:one");
    queue.note_sent("x1", "d1", "k:one");
    assert_eq!(
        queue.find("d1", "k:one"),
        Some(Found::Delivered("x1".to_string()))
    );
    assert!(queue.was_delivered("x1"));
    for n in 0..MAX_DELIVERED + 5 {
        queue.note_sent(&format!("y{n}"), "d1", &format!("k:{n}"));
    }
    assert_eq!(queue.find("d1", "k:one"), None, "the record is bounded");
}

#[test]
fn counts_and_waiting_are_read_without_changing_anything() {
    let mut queue = HeldQueue::default();
    queue.hold(held("a", "d1", "r1", 10));
    queue.hold(held("b", "d2", "r1", 20));
    queue.hold(held("c", "d1", "r2", 30));
    assert_eq!(queue.count(None), 3);
    assert_eq!(queue.count(Some("d1")), 2);
    assert!(!queue.is_waiting("a", 10 + HELD_STALE_MS));
    assert!(queue.is_waiting("a", 11 + HELD_STALE_MS));
    assert!(!queue.is_waiting("zzz", 11 + HELD_STALE_MS));
    queue.confirm("a");
    assert!(
        !queue.is_waiting("a", 11 + HELD_STALE_MS),
        "confirmed is no longer waiting"
    );
}

#[test]
fn ownership_and_forgetting_are_per_phone() {
    let mut queue = HeldQueue::default();
    queue.hold(held("a", "d1", "k:one", 10));
    assert!(queue.owns("a", "d1"));
    assert!(!queue.owns("a", "d2"));
    queue.note_sent("x", "d1", "k:two");
    assert!(queue.delivered_to("x", "d1"));
    assert!(!queue.delivered_to("x", "d2"));
    queue.forget_sent("d2", "k:two");
    assert!(queue.delivered_to("x", "d1"));
    queue.forget_sent("d1", "k:two");
    assert!(!queue.delivered_to("x", "d1"));
    assert_eq!(queue.find("d1", "k:two"), None);
}

#[test]
fn one_device_cannot_push_another_devices_delivered_record_out() {
    use super::held::MAX_DELIVERED_PER_DEVICE;
    let mut queue = HeldQueue::default();
    queue.note_sent("theirs", "d2", "r0");
    for n in 0..(MAX_DELIVERED_PER_DEVICE + 30) {
        queue.note_sent(&format!("m{n}"), "d1", &format!("r{n}"));
    }
    assert!(queue.was_delivered("theirs"));
    assert!(!queue.was_delivered("m0"));
    assert!(queue.was_delivered(&format!("m{}", MAX_DELIVERED_PER_DEVICE + 29)));
    assert!(MAX_DELIVERED_PER_DEVICE < MAX_DELIVERED);
}

#[test]
fn take_due_except_leaves_the_resting_entries_untouched() {
    let mut queue = HeldQueue::default();
    queue.hold(held("a", "d1", "r1", 1));
    queue.hold(held("b", "d1", "r2", 2));
    let resting = std::collections::BTreeSet::from(["a".to_string()]);
    let due = queue.take_due_except(3, &resting);
    assert_eq!(ids(&due), ["b"]);
    assert_eq!(ids(&queue.list()), ["a"]);
    assert!(!queue.was_delivered("a"));
}
