use std::sync::{Arc, Mutex};
use std::time::Duration;

use notchling_engine::vault::entry::{Kind, VaultEntry};
use notchling_local_api::{ops, ApiError, ClientError, ErrorCode};
use serde_json::{json, Value};

use super::*;
use crate::actions::Transport;
use crate::switches::Switches;
use crate::vault::consent_save::{Failure, Flow, Refusal};
use crate::vault::wire::PendingRequest;

const WAIT: Duration = Duration::from_secs(5);

type Calls = Arc<Mutex<Vec<(String, Value)>>>;

struct Daemon {
    calls: Calls,
    fail: bool,
    delay: Duration,
}

impl Transport for Daemon {
    fn call(&self, op: &str, args: Value) -> Result<Value, ClientError> {
        std::thread::sleep(self.delay);
        self.calls.lock().unwrap().push((op.to_owned(), args));
        if self.fail {
            return Err(ClientError::Api(ApiError::new(
                ErrorCode::Failed,
                "the daemon is busy",
            )));
        }
        Ok(json!({"answered": true}))
    }
}

fn daemon(fail: bool) -> (Arc<Vault>, Calls) {
    daemon_after(fail, Duration::ZERO)
}

fn daemon_after(fail: bool, delay: Duration) -> (Arc<Vault>, Calls) {
    let calls: Calls = Arc::default();
    let vault = Arc::new(Vault::over(Box::new(Daemon {
        calls: calls.clone(),
        fail,
        delay,
    })));
    (vault, calls)
}

fn event(id: &str, session: &str, revision: &str) -> Value {
    json!({"id": id, "session": session, "request": {"name": "dashboards-basic-auth"},
           "entry": VaultEntry::new("dashboards-basic-auth", Kind::Login),
           "existing": false, "revision": revision})
}

fn asked(id: &str) -> PendingRequest {
    PendingRequest::from_event(&event(id, "s-1", "r1")).unwrap()
}

fn feed_with(events: &[Value], switches: Switches) -> SwitchFeed {
    let feed = SwitchFeed::fixed(switches);
    for e in events {
        feed.asked_for_test(e.clone());
    }
    feed
}

fn sent(calls: &Calls) -> Vec<(String, Value)> {
    calls.lock().unwrap().clone()
}

fn cancel(id: &str) -> (String, Value) {
    (ops::VAULT_REQUEST_CANCEL.to_owned(), json!({"id": id}))
}

#[test]
fn cancelling_the_form_ends_its_request_on_the_daemon() {
    let feed = feed_with(&[event("c1", "s-1", "r1")], Switches::all_on());
    let (vault, calls) = daemon(false);
    let mut cover = Cover::new(asked("c1"));
    assert!(cancel_form(&mut cover, false, &feed, &vault, WAIT));
    assert_eq!(sent(&calls), [cancel("c1")]);
    assert_eq!(cover.stage(), Stage::Done);
    assert!(feed.requests().is_empty(), "the form is gone from the feed");
}

#[test]
fn cancelling_a_form_ends_the_twins_it_covered_and_nothing_else() {
    let feed = feed_with(
        &[
            event("c1", "s-1", "r1"),
            event("c2", "s-1", "r1"),
            event("c3", "s-1", "r2"),
            event("c4", "s-2", "r1"),
        ],
        Switches::all_on(),
    );
    let (vault, calls) = daemon(false);
    let mut cover = Cover::new(asked("c1"));
    assert!(cancel_form(&mut cover, false, &feed, &vault, WAIT));
    assert_eq!(sent(&calls), [cancel("c1"), cancel("c2")]);
}

#[test]
fn cancelling_still_ends_the_request_while_the_vault_is_off() {
    let feed = feed_with(&[event("c1", "s-1", "r1")], Switches::all_off());
    let (vault, calls) = daemon(false);
    let mut cover = Cover::new(asked("c1"));
    assert!(cancel_form(&mut cover, false, &feed, &vault, WAIT));
    assert_eq!(sent(&calls), [cancel("c1")]);
}

#[test]
fn a_daemon_that_refuses_one_end_does_not_stop_the_others() {
    let feed = feed_with(
        &[event("c1", "s-1", "r1"), event("c2", "s-1", "r1")],
        Switches::all_on(),
    );
    let (vault, calls) = daemon(true);
    let mut cover = Cover::new(asked("c1"));
    assert!(cancel_form(&mut cover, false, &feed, &vault, WAIT));
    assert_eq!(sent(&calls), [cancel("c1"), cancel("c2")]);
    assert_eq!(cover.stage(), Stage::Done);
}

#[test]
fn a_form_that_is_saving_is_left_to_the_save() {
    let feed = feed_with(&[event("c1", "s-1", "r1")], Switches::all_on());
    let (vault, calls) = daemon(false);
    let mut cover = Cover::new(asked("c1"));
    assert!(!cancel_form(&mut cover, true, &feed, &vault, WAIT));
    assert!(sent(&calls).is_empty());
    assert_eq!(cover.stage(), Stage::Open);
    assert_eq!(feed.requests().len(), 1);
}

#[test]
fn a_form_that_already_ended_its_request_sends_nothing() {
    for stage in [Stage::Done, Stage::Refused] {
        let feed = feed_with(&[event("c1", "s-1", "r1")], Switches::all_on());
        let (vault, calls) = daemon(false);
        let mut cover = Cover::new(asked("c1"));
        cover.settle(stage, &feed);
        assert!(cancel_form(&mut cover, false, &feed, &vault, WAIT));
        assert!(sent(&calls).is_empty(), "{stage:?}");
        assert_eq!(cover.stage(), Stage::Done);
    }
}

#[test]
fn a_refusal_the_user_can_fix_still_ends_the_request_when_the_form_closes() {
    let feed = feed_with(&[event("c1", "s-1", "r1")], Switches::all_on());
    let (vault, calls) = daemon(false);
    let mut cover = Cover::new(asked("c1"));
    let mut flow = Flow::default();
    assert!(flow.begin());
    flow.settle(Err(Failure::Refused(Refusal::NameTaken)));
    cover.settle(Stage::after(&flow), &feed);
    assert_eq!(cover.stage(), Stage::Open);
    assert!(cancel_form(&mut cover, false, &feed, &vault, WAIT));
    assert_eq!(sent(&calls), [cancel("c1")]);
}

#[test]
fn closing_twice_ends_the_request_once() {
    let feed = feed_with(&[event("c1", "s-1", "r1")], Switches::all_on());
    let (vault, calls) = daemon(false);
    let mut cover = Cover::new(asked("c1"));
    assert!(cancel_form(&mut cover, false, &feed, &vault, WAIT));
    assert!(cancel_form(&mut cover, false, &feed, &vault, WAIT));
    assert_eq!(sent(&calls), [cancel("c1")]);
}

#[test]
fn a_fixed_vault_has_no_daemon_to_tell() {
    let feed = feed_with(&[event("c1", "s-1", "r1")], Switches::all_on());
    let vault = Arc::new(Vault::fixed(Vec::new()));
    let mut cover = Cover::new(asked("c1"));
    assert!(cancel_form(&mut cover, false, &feed, &vault, WAIT));
    assert_eq!(cover.stage(), Stage::Done);
}

#[test]
fn a_form_that_is_dropped_while_its_request_is_pending_ends_it() {
    let feed = feed_with(
        &[event("c1", "s-1", "r1"), event("c2", "s-1", "r1")],
        Switches::all_on(),
    );
    let (vault, calls) = daemon(false);
    let mut cover = Cover::new(asked("c1"));
    assert!(lapse(&mut cover, false, &feed, &vault, WAIT));
    assert_eq!(sent(&calls), [cancel("c1"), cancel("c2")]);
}

#[test]
fn a_form_dropped_after_the_feed_lost_its_request_ends_nothing() {
    let feed = feed_with(&[], Switches::all_on());
    let (vault, calls) = daemon(false);
    let mut cover = Cover::new(asked("c1"));
    assert!(!lapse(&mut cover, false, &feed, &vault, WAIT));
    assert!(sent(&calls).is_empty());
}

#[test]
fn a_form_dropped_while_saving_ends_nothing() {
    let feed = feed_with(&[event("c1", "s-1", "r1")], Switches::all_on());
    let (vault, calls) = daemon(false);
    let mut cover = Cover::new(asked("c1"));
    assert!(!lapse(&mut cover, true, &feed, &vault, WAIT));
    assert!(sent(&calls).is_empty());
}

#[test]
fn a_form_that_already_closed_ends_nothing_when_it_is_dropped() {
    let feed = feed_with(&[event("c1", "s-1", "r1")], Switches::all_on());
    let (vault, calls) = daemon(false);
    let mut cover = Cover::new(asked("c1"));
    assert!(cancel_form(&mut cover, false, &feed, &vault, WAIT));
    let first = sent(&calls);
    lapse(&mut cover, false, &feed, &vault, WAIT);
    assert_eq!(sent(&calls), first);
}

#[test]
fn a_form_kept_open_for_a_twin_ends_the_twin_when_its_own_request_is_gone() {
    let feed = feed_with(&[event("c2", "s-1", "r1")], Switches::all_on());
    let (vault, calls) = daemon(false);
    let mut cover = Cover::new(asked("c1"));
    let pool: Vec<PendingRequest> = feed
        .requests()
        .iter()
        .filter_map(PendingRequest::from_event)
        .collect();
    assert!(cover.stays(&pool));
    assert!(lapse(&mut cover, false, &feed, &vault, WAIT));
    assert_eq!(sent(&calls), [cancel("c1"), cancel("c2")]);
}

#[test]
fn a_cancel_that_did_not_wait_is_finished_when_the_app_flushes_before_it_quits() {
    let feed = feed_with(&[event("c1", "s-1", "r1")], Switches::all_on());
    let (vault, calls) = daemon_after(false, Duration::from_millis(200));
    let mut cover = Cover::new(asked("c1"));
    assert!(cancel_form(
        &mut cover,
        false,
        &feed,
        &vault,
        Duration::ZERO
    ));
    assert!(sent(&calls).is_empty(), "the call is still on its way");
    assert!(vault.flush_cancels(WAIT));
    assert_eq!(sent(&calls), [cancel("c1")]);
}

#[test]
fn a_flush_gives_up_after_its_wait_and_does_not_wait_for_nothing() {
    let (vault, calls) = daemon_after(false, Duration::from_millis(300));
    let feed = feed_with(&[event("c1", "s-1", "r1")], Switches::all_on());
    assert!(vault.flush_cancels(Duration::ZERO));
    let mut cover = Cover::new(asked("c1"));
    assert!(cancel_form(
        &mut cover,
        false,
        &feed,
        &vault,
        Duration::ZERO
    ));
    assert!(!vault.flush_cancels(Duration::from_millis(10)));
    assert!(vault.flush_cancels(WAIT));
    assert_eq!(sent(&calls).len(), 1);
}

#[test]
fn a_failed_call_still_lets_the_flush_finish() {
    let feed = feed_with(&[event("c1", "s-1", "r1")], Switches::all_on());
    let (vault, calls) = daemon_after(true, Duration::from_millis(50));
    let mut cover = Cover::new(asked("c1"));
    assert!(cancel_form(
        &mut cover,
        false,
        &feed,
        &vault,
        Duration::ZERO
    ));
    assert!(vault.flush_cancels(WAIT));
    assert_eq!(sent(&calls), [cancel("c1")]);
}

struct Panicking;

impl Transport for Panicking {
    fn call(&self, _: &str, _: Value) -> Result<Value, ClientError> {
        panic!("the transport blew up");
    }
}

fn gave_up(feed: &SwitchFeed, ids: &[&str]) {
    for id in ids {
        feed.dismiss_request(id);
    }
}

fn name_taken() -> Result<(), Failure> {
    Err(Failure::Refused(Refusal::NameTaken))
}

fn owned(ids: &[&str]) -> Vec<String> {
    ids.iter().map(|id| (*id).to_owned()).collect()
}

#[test]
fn a_save_refused_for_its_name_after_its_form_is_gone_ends_the_request_and_its_twins() {
    let feed = feed_with(
        &[event("c1", "s-1", "r1"), event("c2", "s-1", "r1")],
        Switches::all_on(),
    );
    gave_up(&feed, &["c1", "c2"]);
    let (vault, calls) = daemon(false);
    end_unsaved(&vault, &feed, owned(&["c1", "c2"]), &name_taken(), WAIT);
    assert_eq!(sent(&calls), [cancel("c1"), cancel("c2")]);
}

#[test]
fn a_save_that_worked_after_its_form_is_gone_ends_nothing() {
    let feed = feed_with(&[event("c1", "s-1", "r1")], Switches::all_on());
    gave_up(&feed, &["c1"]);
    let (vault, calls) = daemon(false);
    end_unsaved(&vault, &feed, owned(&["c1"]), &Ok(()), WAIT);
    assert!(sent(&calls).is_empty());
}

#[test]
fn a_save_that_failed_for_any_reason_but_its_name_leaves_the_request_alone() {
    let feed = feed_with(&[event("c1", "s-1", "r1")], Switches::all_on());
    gave_up(&feed, &["c1"]);
    let (vault, calls) = daemon(false);
    for failure in [
        Failure::Refused(Refusal::VaultOff),
        Failure::Refused(Refusal::Expired),
        Failure::Refused(Refusal::Changed),
        Failure::Refused(Refusal::Withdrawn),
        Failure::Other("the daemon is down".to_owned()),
    ] {
        end_unsaved(&vault, &feed, owned(&["c1"]), &Err(failure), WAIT);
    }
    assert!(sent(&calls).is_empty());
}

#[test]
fn a_save_refused_for_its_name_leaves_a_request_the_feed_no_longer_has() {
    let feed = feed_with(&[event("c1", "s-1", "r1")], Switches::all_on());
    feed.ended_for_test("c1");
    let (vault, calls) = daemon(false);
    end_unsaved(&vault, &feed, owned(&["c1"]), &name_taken(), WAIT);
    assert!(sent(&calls).is_empty());
}

#[test]
fn a_save_refused_for_its_name_leaves_a_request_that_another_form_holds_as_a_twin() {
    let feed = feed_with(
        &[event("c1", "s-1", "r1"), event("c2", "s-1", "r1")],
        Switches::all_on(),
    );
    gave_up(&feed, &["c2"]);
    assert!(!feed.unwanted_request("c2"));
    let (vault, calls) = daemon(false);
    end_unsaved(&vault, &feed, owned(&["c1", "c2"]), &name_taken(), WAIT);
    assert!(sent(&calls).is_empty());
}

#[test]
fn a_save_refused_for_its_name_still_ends_a_request_no_form_holds_even_as_a_twin() {
    let feed = feed_with(
        &[
            event("c1", "s-1", "r1"),
            event("c2", "s-2", "r1"),
            event("c3", "s-1", "r2"),
        ],
        Switches::all_on(),
    );
    gave_up(&feed, &["c2", "c3"]);
    assert!(!feed.unwanted_request("c1"));
    assert!(feed.unwanted_request("c2"));
    assert!(feed.unwanted_request("c3"));
    let (vault, calls) = daemon(false);
    end_unsaved(
        &vault,
        &feed,
        owned(&["c1", "c2", "c3"]),
        &name_taken(),
        WAIT,
    );
    assert_eq!(sent(&calls), [cancel("c2"), cancel("c3")]);
}

#[test]
fn a_request_nobody_holds_is_unwanted_once_every_form_that_covered_it_gave_up() {
    let feed = feed_with(
        &[event("c1", "s-1", "r1"), event("c2", "s-1", "r1")],
        Switches::all_on(),
    );
    gave_up(&feed, &["c1", "c2"]);
    assert!(feed.unwanted_request("c1"));
    assert!(feed.unwanted_request("c2"));
}

#[test]
fn a_form_dropped_while_saving_hands_its_requests_back_to_the_inbox() {
    let feed = feed_with(
        &[event("c1", "s-1", "r1"), event("c2", "s-1", "r1")],
        Switches::all_on(),
    );
    let (vault, calls) = daemon(false);
    let mut cover = Cover::new(asked("c1"));
    cover.begin(&feed);
    assert!(!lapse(&mut cover, true, &feed, &vault, WAIT));
    assert!(feed.requests().is_empty());
    assert!(feed.unwanted_request("c1"));
    assert!(feed.unwanted_request("c2"));
    assert!(sent(&calls).is_empty());
}

#[test]
fn a_cancel_that_panics_still_lets_the_flush_finish() {
    let vault = Arc::new(Vault::over(Box::new(Panicking)));
    let feed = feed_with(&[event("c1", "s-1", "r1")], Switches::all_on());
    let mut cover = Cover::new(asked("c1"));
    assert!(cancel_form(
        &mut cover,
        false,
        &feed,
        &vault,
        Duration::ZERO
    ));
    assert!(vault.flush_cancels(Duration::from_millis(500)));
}

#[test]
fn beginning_a_save_names_the_request_and_the_twins_it_will_press() {
    let feed = feed_with(
        &[
            event("c1", "s-1", "r1"),
            event("c2", "s-1", "r1"),
            event("c3", "s-2", "r1"),
        ],
        Switches::all_on(),
    );
    let mut cover = Cover::new(asked("c1"));
    assert_eq!(cover.begin(&feed), ["c1", "c2"]);
}
