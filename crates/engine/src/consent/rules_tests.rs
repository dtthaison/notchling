use super::store_tests::new;
use super::*;

fn open_rows(store: &ConsentStore, n: usize, session: &str, at: i64) {
    for i in 0..n {
        store
            .insert(&new(session, &format!("{session}-k{i}")), at + i as i64)
            .unwrap();
    }
}

fn busy(result: Result<Opened, Refusal>) -> i64 {
    match result {
        Err(Refusal::Busy { retry_after_ms }) => retry_after_ms,
        other => panic!("expected busy, got {other:?}"),
    }
}

#[test]
fn three_wait_per_verified_session_and_the_fourth_is_busy() {
    let store = ConsentStore::in_memory_for_tests();
    open_rows(&store, 3, "s1", 1_000);
    let after = busy(store.insert(&new("s1", "more"), 5_000));
    assert!(after > 0);
    store.insert(&new("s2", "other"), 5_000).unwrap();
}

#[test]
fn twenty_wait_in_all() {
    let store = ConsentStore::in_memory_for_tests();
    for n in 0..6 {
        open_rows(&store, 3, &format!("s{n}"), 1_000);
    }
    open_rows(&store, 2, "s6", 1_000);
    assert!(matches!(
        store.insert(&new("s9", "x"), 2_000),
        Err(Refusal::Busy { .. })
    ));
}

#[test]
fn ten_open_an_hour_per_session_counting_closed_ones() {
    let store = ConsentStore::in_memory_for_tests();
    for n in 0..10 {
        let id = store
            .insert(&new("s1", &format!("k{n}")), 1_000 + n)
            .unwrap()
            .row()
            .id
            .clone();
        store.finish(&id, State::Cancelled, None, 2_000).unwrap();
    }
    let after = busy(store.insert(&new("s1", "k99"), 3_000));
    assert!(after > 3_000_000);
    store
        .insert(&new("s1", "k99"), 1_000 + HOUR_MS + 1_000)
        .unwrap();
}

#[test]
fn after_a_deny_the_same_kind_and_target_is_refused_for_thirty_minutes() {
    let store = ConsentStore::in_memory_for_tests();
    let id = store.insert(&new("s1", "k1"), 0).unwrap().row().id.clone();
    store
        .answer(&id, State::Denied, "mac", None, 1_000)
        .unwrap();
    let left = busy(store.insert(&new("s1", "k1"), 2_000));
    assert_eq!(left, COOLDOWN_MS - 1_000);
    store.insert(&new("s1", "k2"), 2_000).unwrap();
    store.insert(&new("s2", "k1"), 2_000).unwrap();
    store.insert(&new("s1", "k1"), COOLDOWN_MS + 1_001).unwrap();
}

#[test]
fn a_caller_in_no_verified_session_counts_against_its_root_app() {
    let store = ConsentStore::in_memory_for_tests();
    let mut shell = new("ignored", "k1");
    shell.verified = false;
    shell.session = None;
    shell.tool = "shell".to_string();
    store.insert(&shell, 1_000).unwrap();
    let mut second = shell.clone();
    second.target = "k2".to_string();
    assert!(matches!(
        store.insert(&second, 2_000),
        Err(Refusal::Busy { .. })
    ));
    let mut elsewhere = second.clone();
    elsewhere.root = Some("Cursor".to_string());
    store.insert(&elsewhere, 2_000).unwrap();
}

#[test]
fn an_unverified_caller_may_not_ask_for_more_than_fifteen_minutes() {
    let store = ConsentStore::in_memory_for_tests();
    let mut shell = new("x", "k1");
    shell.verified = false;
    shell.session = None;
    shell.expires_in_ms = ROOT_MAX_EXPIRY_MS + 1;
    assert_eq!(store.insert(&shell, 0), Err(Refusal::LongExpiry));
    shell.expires_in_ms = ROOT_MAX_EXPIRY_MS;
    store.insert(&shell, 0).unwrap();
}

#[test]
fn a_long_end_is_for_a_verified_session_one_at_a_time() {
    let store = ConsentStore::in_memory_for_tests();
    let mut long = new("s1", "k1");
    long.expires_in_ms = LONG_EXPIRY_MS + 1;
    store.insert(&long, 0).unwrap();
    long.target = "k2".to_string();
    assert!(matches!(store.insert(&long, 1), Err(Refusal::Busy { .. })));
    let mut other = new("s2", "k1");
    other.expires_in_ms = LONG_EXPIRY_MS;
    store.insert(&other, 1).unwrap();
}
