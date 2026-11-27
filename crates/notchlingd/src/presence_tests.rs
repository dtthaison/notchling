use super::*;
use std::sync::atomic::{AtomicI64, Ordering};

fn rig() -> (Arc<Presence>, Arc<AtomicI64>) {
    let clock = Arc::new(AtomicI64::new(1_000));
    let ticking = clock.clone();
    let presence = Presence::new(GRACE_MS, Arc::new(move || ticking.load(Ordering::SeqCst)));
    (presence, clock)
}

fn at(clock: &AtomicI64, ms: i64) {
    clock.store(ms, Ordering::SeqCst);
}

#[test]
fn a_key_is_present_while_a_guard_lives_and_for_the_grace_after() {
    let (presence, clock) = rig();
    assert!(!presence.present("a", 1_000));
    let guard = presence.enter("a", 1_000);
    assert!(presence.present("a", 1_000));
    assert!(presence.present("a", 500_000));
    at(&clock, 2_000);
    drop(guard);
    assert!(presence.present("a", 2_000));
    assert!(presence.present("a", 6_999));
    assert!(!presence.present("a", 7_000));
}

#[test]
fn two_overlapping_waiters_keep_the_key_until_both_leave() {
    let (presence, clock) = rig();
    let first = presence.enter("a", 1_000);
    at(&clock, 2_000);
    let second = presence.enter("a", 2_000);
    assert_eq!(presence.since("a"), Some(1_000));
    at(&clock, 3_000);
    drop(first);
    assert!(presence.present("a", 60_000));
    at(&clock, 4_000);
    drop(second);
    assert!(!presence.present("a", 9_000));
    assert!(presence.present("a", 8_999));
}

#[test]
fn since_stays_at_the_first_enter_across_back_to_back_slices() {
    let (presence, clock) = rig();
    let slice = presence.enter("a", 1_000);
    at(&clock, 26_000);
    drop(slice);
    let next = presence.enter("a", 27_000);
    assert_eq!(presence.since("a"), Some(1_000));
    at(&clock, 52_000);
    drop(next);
    let again = presence.enter("a", 53_000);
    assert_eq!(presence.since("a"), Some(1_000));
    drop(again);
}

#[test]
fn a_new_stretch_after_the_grace_starts_a_new_since() {
    let (presence, clock) = rig();
    let first = presence.enter("a", 1_000);
    at(&clock, 2_000);
    drop(first);
    let later = presence.enter("a", 20_000);
    assert_eq!(presence.since("a"), Some(20_000));
    drop(later);
}

#[test]
fn sweep_reports_each_change_once() {
    let (presence, clock) = rig();
    assert!(presence.sweep(1_000).is_empty());
    let guard = presence.enter("a", 1_000);
    let other = presence.enter("b", 1_000);
    assert_eq!(
        presence.sweep(1_000),
        vec![("a".to_string(), true), ("b".to_string(), true)]
    );
    assert!(presence.sweep(1_500).is_empty());
    at(&clock, 3_000);
    drop(guard);
    assert!(presence.sweep(3_000).is_empty());
    assert!(presence.sweep(7_999).is_empty());
    assert_eq!(presence.sweep(8_000), vec![("a".to_string(), false)]);
    assert!(presence.sweep(9_000).is_empty());
    drop(other);
}

#[test]
fn a_stretch_that_never_reached_a_sweep_leaves_nothing_behind() {
    let (presence, clock) = rig();
    let guard = presence.enter("a", 1_000);
    at(&clock, 1_100);
    drop(guard);
    assert!(presence.sweep(60_000).is_empty());
    assert!(!presence.present("a", 60_000));
    assert_eq!(presence.since("a"), None);
}

#[test]
fn a_new_wait_after_an_unswept_expiry_is_reported_again() {
    let (presence, clock) = rig();
    let first = presence.enter("a", 1_000);
    assert_eq!(presence.sweep(1_000), vec![("a".to_string(), true)]);
    at(&clock, 2_000);
    drop(first);
    let second = presence.enter("a", 10_000);
    assert_eq!(presence.since("a"), Some(10_000));
    assert_eq!(presence.sweep(10_000), vec![("a".to_string(), true)]);
    drop(second);
}

#[test]
fn forgetting_a_key_drops_it_at_once() {
    let (presence, _clock) = rig();
    let guard = presence.enter("a", 1_000);
    assert_eq!(presence.sweep(1_000), vec![("a".to_string(), true)]);
    drop(guard);
    presence.forget("a");
    assert!(!presence.present("a", 1_001));
    assert_eq!(presence.since("a"), None);
    assert!(presence.sweep(1_001).is_empty());
}
