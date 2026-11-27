use notchling_engine::vault::entry::{Kind, VaultEntry};
use serde_json::{json, Value};

use super::*;
use crate::switches::Switches;
use crate::vault::consent_save::{Failure, Refusal};

fn event(id: &str, revision: &str) -> Value {
    json!({"id": id, "session": "s-1", "request": {"name": "github"},
           "entry": VaultEntry::new("github", Kind::Token),
           "existing": true, "revision": revision})
}

fn asked(id: &str, revision: &str) -> PendingRequest {
    PendingRequest::from_event(&event(id, revision)).unwrap()
}

fn feed_with(events: &[Value]) -> SwitchFeed {
    let feed = SwitchFeed::fixed(Switches::all_on());
    for e in events {
        feed.asked_for_test(e.clone());
    }
    feed
}

fn pool(feed: &SwitchFeed) -> Vec<PendingRequest> {
    feed.requests()
        .iter()
        .filter_map(PendingRequest::from_event)
        .collect()
}

fn new_forms(forms: &[&Cover], pool: &[PendingRequest]) -> Vec<String> {
    pool.iter()
        .filter(|r| !forms.iter().any(|f| f.holds(r)))
        .map(|r| r.id.clone())
        .collect()
}

fn after(result: Result<(), Failure>) -> Stage {
    let mut flow = Flow::default();
    assert!(flow.begin());
    flow.settle(result);
    Stage::after(&flow)
}

fn refused(refusal: Refusal) -> Stage {
    after(Err(Failure::Refused(refusal)))
}

#[test]
fn a_saved_form_keeps_the_twins_it_covered_hidden_until_the_daemon_ends_them() {
    let feed = feed_with(&[event("c1", "r1"), event("c2", "r1")]);
    let mut form = Cover::new(asked("c1", "r1"));
    assert!(new_forms(&[&form], &pool(&feed)).is_empty());
    assert_eq!(form.settle(after(Ok(())), &feed), ["c2"]);
    feed.resolve("c1");
    let left = pool(&feed);
    assert!(!form.stays(&left), "the saved form goes");
    assert!(
        new_forms(&[], &left).is_empty(),
        "no form for a twin the save answered: {left:?}"
    );
    feed.ended_for_test("c2");
    assert!(pool(&feed).is_empty());
}

#[test]
fn a_final_refusal_stays_with_its_message_and_hides_only_the_twins_it_covered_then() {
    let feed = feed_with(&[event("c1", "r1"), event("c2", "r1")]);
    let mut form = Cover::new(asked("c1", "r1"));
    feed.ended_for_test("c1");
    assert!(form.stays(&pool(&feed)), "kept for its live twin");
    assert_eq!(form.settle(refused(Refusal::Changed), &feed), ["c2"]);
    assert_eq!(form.stage(), Stage::Refused);
    let left = pool(&feed);
    assert!(form.stays(&left), "the refusal stays until it is closed");
    assert!(
        new_forms(&[&form], &left).is_empty(),
        "no form for the withdrawn twin: {left:?}"
    );
    feed.asked_for_test(event("c3", "r1"));
    feed.asked_for_test(event("c4", "r2"));
    assert_eq!(
        new_forms(&[&form], &pool(&feed)),
        ["c3", "c4"],
        "a refused form covers nothing new"
    );
    assert!(
        form.settle(Stage::Done, &feed).is_empty(),
        "closing it hides nothing new"
    );
    assert!(!form.stays(&pool(&feed)));
    assert_eq!(new_forms(&[], &pool(&feed)), ["c3", "c4"]);
}

#[test]
fn a_cancel_dismisses_the_twins_of_the_same_snapshot_once_and_nothing_else() {
    let feed = feed_with(&[event("c1", "r1"), event("c2", "r1"), event("c3", "r2")]);
    let mut form = Cover::new(asked("c1", "r1"));
    assert_eq!(form.settle(Stage::Done, &feed), ["c2"]);
    feed.dismiss_request("c1");
    assert_eq!(new_forms(&[&form], &pool(&feed)), ["c3"]);
    assert!(form.settle(Stage::Done, &feed).is_empty(), "only once");
}

#[test]
fn a_refusal_the_user_can_fix_keeps_the_form_open_and_covering() {
    let feed = feed_with(&[event("c1", "r1"), event("c2", "r1")]);
    for refusal in [Refusal::VaultOff, Refusal::NameTaken] {
        let mut form = Cover::new(asked("c1", "r1"));
        assert!(form.settle(refused(refusal), &feed).is_empty());
        assert_eq!(form.stage(), Stage::Open);
        assert!(form.covers(&asked("c2", "r1")));
    }
    for refusal in [Refusal::Expired, Refusal::Changed, Refusal::Withdrawn] {
        assert_eq!(refused(refusal), Stage::Refused);
    }
    assert_eq!(after(Err(Failure::Other("offline".into()))), Stage::Open);
    assert_eq!(pool(&feed).len(), 2);
}

#[test]
fn a_form_never_goes_back_to_covering() {
    let feed = feed_with(&[event("c2", "r1")]);
    let mut form = Cover::new(asked("c1", "r1"));
    form.settle(Stage::Refused, &feed);
    assert!(form.settle(Stage::Open, &feed).is_empty());
    assert_eq!(form.stage(), Stage::Refused);
    assert!(!form.covers(&asked("c2", "r1")));
    form.settle(Stage::Done, &feed);
    form.settle(Stage::Refused, &feed);
    assert_eq!(form.stage(), Stage::Done);
}

#[test]
fn an_open_form_stays_while_its_request_or_a_twin_it_covers_waits() {
    let form = Cover::new(asked("c1", "r1"));
    assert!(form.stays(&[asked("c1", "r1")]));
    assert!(form.stays(&[asked("c2", "r1")]));
    assert!(!form.stays(&[asked("c3", "r2")]), "another snapshot");
    assert!(!form.stays(&[]));
    assert!(form.holds(&asked("c1", "r1")));
    assert!(!form.holds(&asked("c3", "r2")));
}

#[test]
fn a_save_settles_only_the_twins_that_waited_when_save_was_pressed() {
    let feed = feed_with(&[event("c1", "r1"), event("c2", "r1")]);
    let mut form = Cover::new(asked("c1", "r1"));
    form.begin(&feed);
    feed.asked_for_test(event("c3", "r1"));
    assert_eq!(form.settle(refused(Refusal::Expired), &feed), ["c2"]);
    assert_eq!(
        new_forms(&[&form], &pool(&feed)),
        ["c3"],
        "a re-ask after Save was pressed gets its own form"
    );
}

#[test]
fn a_retry_after_a_save_that_failed_takes_the_twins_waiting_then() {
    let feed = feed_with(&[event("c1", "r1"), event("c2", "r1")]);
    let mut form = Cover::new(asked("c1", "r1"));
    form.begin(&feed);
    assert!(form
        .settle(after(Err(Failure::Other("offline".into()))), &feed)
        .is_empty());
    feed.asked_for_test(event("c3", "r1"));
    form.begin(&feed);
    assert_eq!(form.settle(after(Ok(())), &feed), ["c2", "c3"]);
    let mut cancelled = Cover::new(asked("c1", "r1"));
    feed.asked_for_test(event("c4", "r1"));
    cancelled.begin(&feed);
    cancelled.settle(refused(Refusal::NameTaken), &feed);
    feed.asked_for_test(event("c5", "r1"));
    assert_eq!(
        cancelled.settle(Stage::Done, &feed),
        ["c4", "c5"],
        "Cancel hides every twin it covers"
    );
}

#[test]
fn a_form_with_a_save_in_flight_stays_when_the_feed_ends_its_requests_first() {
    let feed = feed_with(&[event("c1", "r1"), event("c2", "r1")]);
    let mut form = Cover::new(asked("c1", "r1"));
    form.begin(&feed);
    feed.ended_for_test("c1");
    feed.ended_for_test("c2");
    assert!(
        form.stays(&pool(&feed)),
        "it waits to show how the save ended"
    );
    assert!(form.settle(refused(Refusal::Changed), &feed).is_empty());
    assert!(form.stays(&pool(&feed)), "and then shows the refusal");
    let mut retry = Cover::new(asked("c1", "r1"));
    retry.begin(&feed);
    retry.settle(refused(Refusal::NameTaken), &feed);
    assert!(
        !retry.stays(&pool(&feed)),
        "an open form with nothing in flight and nothing waiting goes"
    );
}
