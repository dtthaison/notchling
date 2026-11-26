use super::retry::*;
use super::store_tests::new;
use super::*;
use serde_json::json;

fn denied(store: &ConsentStore, session: &str, target: &str, note: Option<&str>) -> Consent {
    let id = store
        .insert(&new(session, target), 1_000)
        .unwrap()
        .row()
        .id
        .clone();
    store
        .answer_noted(&id, State::Denied, "mac", None, note, 2_000)
        .unwrap();
    store.get(&id).unwrap().unwrap()
}

fn refused(result: Result<serde_json::Value, Refusal>) -> &'static str {
    match result {
        Err(Refusal::RetryRefused(why)) => why,
        other => panic!("expected a refusal, got {other:?}"),
    }
}

#[test]
fn the_same_change_target_and_reason_is_refused() {
    let store = ConsentStore::in_memory_for_tests();
    let old = denied(&store, "s1", "k1", Some("use k2"));
    assert_eq!(refused(check_retry(&old, &new("s1", "k1"))), UNCHANGED);
}

#[test]
fn a_changed_reason_target_or_change_is_accepted_and_carries_the_three_keys() {
    let store = ConsentStore::in_memory_for_tests();
    let old = denied(&store, "s1", "k1", Some("use k2"));
    let mut other_reason = new("s1", "k1");
    other_reason.reason = Some("a better reason".into());
    let change = check_retry(&old, &other_reason).unwrap();
    assert_eq!(change["retryOf"], json!(old.id));
    assert_eq!(change["priorNote"], json!("use k2"));
    assert_eq!(change["priorSummary"], json!("setting k1"));
    assert_eq!(change["after"], json!("b"));
    check_retry(&old, &new("s1", "k2")).unwrap();
    let mut other_change = new("s1", "k1");
    other_change.change["after"] = json!("c");
    check_retry(&old, &other_change).unwrap();
}

#[test]
fn the_retry_keys_are_ignored_when_comparing_changes() {
    let store = ConsentStore::in_memory_for_tests();
    let old = denied(&store, "s1", "k1", Some("use k2"));
    let mut again = new("s1", "k1");
    again.change["retryOf"] = json!("c-old");
    again.change["priorNote"] = json!("x");
    assert_eq!(refused(check_retry(&old, &again)), UNCHANGED);
}

#[test]
fn a_deny_without_a_note_cannot_be_retried() {
    let store = ConsentStore::in_memory_for_tests();
    let old = denied(&store, "s1", "k1", None);
    assert_eq!(refused(check_retry(&old, &new("s1", "k2"))), NOT_NOTED);
}

#[test]
fn a_row_that_was_not_denied_cannot_be_retried() {
    let store = ConsentStore::in_memory_for_tests();
    let id = store.insert(&new("s1", "k1"), 0).unwrap().row().id.clone();
    let old = store.get(&id).unwrap().unwrap();
    assert_eq!(refused(check_retry(&old, &new("s1", "k2"))), NOT_NOTED);
}

#[test]
fn another_session_cannot_retry_it() {
    let store = ConsentStore::in_memory_for_tests();
    let old = denied(&store, "s1", "k1", Some("use k2"));
    assert_eq!(refused(check_retry(&old, &new("s2", "k2"))), OTHER_ASKER);
    let mut stranger = new("s1", "k2");
    stranger.verified = false;
    stranger.session = None;
    assert_eq!(refused(check_retry(&old, &stranger)), OTHER_ASKER);
}

#[test]
fn the_note_is_trimmed_cleaned_and_cut() {
    assert_eq!(
        clean_note("  call it b2 \u{7}\n"),
        Some("call it b2".into())
    );
    assert_eq!(clean_note(" \n\t "), None);
    assert_eq!(clean_note(&"x".repeat(400)).unwrap().chars().count(), 300);
}

#[test]
fn a_noted_deny_keeps_its_reason_and_note_and_old_rows_have_none() {
    let store = ConsentStore::in_memory_for_tests();
    let old = denied(&store, "s1", "k1", Some("use k2"));
    assert_eq!(old.note.as_deref(), Some("use k2"));
    assert_eq!(old.reason.as_deref(), Some("because"));
    let plain = denied(&store, "s2", "k1", None);
    assert_eq!(plain.note, None);
    assert_eq!(plain.reason, None);
    let allowed = store.insert(&new("s3", "k1"), 0).unwrap().row().id.clone();
    let taken = store
        .answer_noted(&allowed, State::Applying, "mac", None, Some("no"), 5)
        .unwrap();
    match taken {
        Taken::Won(row) => assert_eq!(row.note, None),
        other => panic!("{other:?}"),
    }
}

#[test]
fn two_retries_of_one_denial_at_once_insert_exactly_one() {
    let store = std::sync::Arc::new(ConsentStore::in_memory_for_tests());
    let old = denied(&store, "s1", "k1", Some("use k2"));
    let gate = std::sync::Arc::new(std::sync::Barrier::new(2));
    let runs: Vec<_> = ["k2", "k3"]
        .into_iter()
        .map(|target| {
            let (store, gate, old) = (store.clone(), gate.clone(), old.clone());
            std::thread::spawn(move || {
                let mut fresh = new("s1", target);
                fresh.change = check_retry(&old, &fresh).unwrap();
                gate.wait();
                store.insert(&fresh, 3_000).map(|_| ())
            })
        })
        .collect();
    let results: Vec<_> = runs.into_iter().map(|r| r.join().unwrap()).collect();
    assert_eq!(
        results.iter().filter(|r| r.is_ok()).count(),
        1,
        "{results:?}"
    );
    assert!(results
        .iter()
        .any(|r| matches!(r, Err(Refusal::RetryRefused(ALREADY)))));
}

fn with_message(mut fresh: New, message: &str) -> New {
    fresh.change["message"] = json!(message);
    fresh
}

fn denied_with(store: &ConsentStore, asked: New, note: &str) -> Consent {
    let id = store.insert(&asked, 1_000).unwrap().row().id.clone();
    store
        .answer_noted(&id, State::Denied, "mac", None, Some(note), 2_000)
        .unwrap();
    store.get(&id).unwrap().unwrap()
}

#[test]
fn a_retry_that_changes_only_the_message_counts_as_a_change() {
    let store = ConsentStore::in_memory_for_tests();
    let old = denied_with(&store, with_message(new("s1", "k1"), "run rm -rf"), "no");
    assert!(old.change.get("message").is_none());
    assert_eq!(
        refused(check_retry(
            &old,
            &with_message(new("s1", "k1"), "run rm -rf")
        )),
        UNCHANGED
    );
    let changed = check_retry(&old, &with_message(new("s1", "k1"), "run ls")).unwrap();
    assert_eq!(changed["message"], "run ls");
    assert!(changed.get("sentHash").is_none());
}

#[test]
fn an_old_row_without_a_stored_hash_accepts_a_retry_that_has_a_message() {
    let store = ConsentStore::in_memory_for_tests();
    let old = denied(&store, "s1", "k1", Some("no"));
    check_retry(&old, &with_message(new("s1", "k1"), "anything")).unwrap();
}

#[test]
fn a_denied_row_is_retried_once_whatever_became_of_the_retry() {
    let store = ConsentStore::in_memory_for_tests();
    let old = denied(&store, "s1", "k1", Some("use k2"));
    assert!(!store.retried_by(&old.id).unwrap());
    let mut fresh = new("s1", "k2");
    fresh.change = check_retry(&old, &fresh).unwrap();
    let retry = store.insert(&fresh, 3_000).unwrap().row().id.clone();
    assert!(store.retried_by(&old.id).unwrap());
    store
        .answer_noted(&retry, State::Cancelled, "mac", None, None, 4_000)
        .unwrap();
    assert!(store.retried_by(&old.id).unwrap());
    assert!(!store.retried_by(&retry).unwrap());
}
