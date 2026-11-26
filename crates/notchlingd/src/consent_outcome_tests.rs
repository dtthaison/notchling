use super::tests::rig;
use super::*;
use crate::consent_failure::Failure;
use crate::consent_testkit::item_id_for;
use notchling_engine::consent::SETTING;
use std::sync::mpsc::{channel, Receiver};
use std::sync::Mutex;

struct Fails(&'static str);

impl Handler for Fails {
    fn apply_failing(
        &self,
        _: &Consent,
        _: Option<Answerer>,
        _: Option<&str>,
    ) -> Result<Value, Failure> {
        Err(Failure::new("step_failed", self.0))
    }
}

struct Holds(Mutex<Receiver<()>>);

impl Handler for Holds {
    fn apply(&self, _: &Consent, _: Option<Answerer>, _: Option<&str>) -> Result<Value, String> {
        let _ = self.0.lock().unwrap().recv();
        Ok(json!({"applied": true}))
    }
}

fn allow(r: &super::tests::Rig, id: &str) {
    let item = item_id_for(&r.inbox, id);
    r.inbox.decide(&item, ActionId::Allow, "mac").unwrap();
}

#[test]
fn an_allowed_request_whose_apply_fails_ends_with_a_failed_outcome_and_its_reason() {
    let r = rig("outcome-failed");
    r.consent.register(
        SETTING,
        Arc::new(Fails("The machine is busy at /Users/x/key.")),
    );
    let row = r.open(1, SETTING, "limitMode");
    allow(&r, &row.id);
    r.eventually_state(&row.id, "failed");
    let item = r.eventually_item(&row.id, "answered");
    assert_eq!(item["answer"], "allow");
    assert_eq!(item["outcome"]["state"], "failed");
    assert_eq!(item["outcome"]["reason"], "The machine is busy at …");
    assert!(item["outcome"]["at_ms"].as_i64().unwrap() > 0);
}

#[test]
fn an_allowed_request_that_applies_ends_with_an_applied_outcome() {
    let r = rig("outcome-applied");
    let row = r.open(1, SETTING, "limitMode");
    allow(&r, &row.id);
    r.eventually_state(&row.id, "applied");
    let mut item = r.eventually_item(&row.id, "answered");
    for _ in 0..500 {
        if item["outcome"]["state"] == "applied" {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
        item = r.eventually_item(&row.id, "answered");
    }
    assert_eq!(item["outcome"]["state"], "applied");
    assert!(item["outcome"].get("reason").is_none());
}

#[test]
fn an_answered_request_shows_applying_while_its_apply_runs() {
    let r = rig("outcome-applying");
    let (release, held) = channel();
    r.consent
        .register(SETTING, Arc::new(Holds(Mutex::new(held))));
    let row = r.open(1, SETTING, "limitMode");
    allow(&r, &row.id);
    let mut item = crate::consent_testkit::item_for(&r.inbox, &row.id);
    for _ in 0..500 {
        if item["outcome"]["state"] == "applying" {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
        item = crate::consent_testkit::item_for(&r.inbox, &row.id);
    }
    assert_eq!(item["outcome"]["state"], "applying");
    release.send(()).unwrap();
    r.eventually_state(&row.id, "applied");
}

fn half_states(seen: &Mutex<Vec<notchling_engine::inbox::Change>>) -> Vec<String> {
    use notchling_engine::inbox::{Change, State};
    let mut half = Vec::new();
    for change in seen.lock().unwrap().iter() {
        let Change::Updated(item) = change else {
            continue;
        };
        let shown = item.outcome.as_ref().map(|outcome| outcome.state);
        if item.state == State::Answered && shown.is_none() {
            half.push("answered with no outcome".to_string());
        }
    }
    half
}

#[test]
fn an_answer_through_consent_answer_keeps_the_item_open_while_it_applies_and_closes_it_once() {
    let r = rig("outcome-atomic");
    let seen = Arc::new(Mutex::new(Vec::new()));
    let tap = seen.clone();
    r.inbox.set_tap(Arc::new(move |changes| {
        tap.lock().unwrap().extend(changes);
    }));
    let (release, held) = channel();
    r.consent
        .register(SETTING, Arc::new(Holds(Mutex::new(held))));
    let row = r.open(1, SETTING, "limitMode");
    r.consent.decide(&row.id, true, None, "mac", None).unwrap();
    let mut item = crate::consent_testkit::item_for(&r.inbox, &row.id);
    for _ in 0..500 {
        if item["outcome"]["state"] == "applying" {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
        item = crate::consent_testkit::item_for(&r.inbox, &row.id);
    }
    assert_eq!(item["state"], "open");
    assert_eq!(item["outcome"]["state"], "applying");
    release.send(()).unwrap();
    r.eventually_state(&row.id, "applied");
    let mut item = crate::consent_testkit::item_for(&r.inbox, &row.id);
    for _ in 0..500 {
        if item["outcome"]["state"] == "applied" {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
        item = crate::consent_testkit::item_for(&r.inbox, &row.id);
    }
    assert_eq!(item["state"], "answered");
    assert_eq!(item["outcome"]["state"], "applied");
    assert_eq!(half_states(&seen), Vec::<String>::new());
}

#[test]
fn a_failed_apply_never_shows_an_answered_item_without_its_outcome() {
    let r = rig("outcome-atomic-failed");
    let seen = Arc::new(Mutex::new(Vec::new()));
    let tap = seen.clone();
    r.inbox.set_tap(Arc::new(move |changes| {
        tap.lock().unwrap().extend(changes);
    }));
    r.consent.register(SETTING, Arc::new(Fails("Busy.")));
    let row = r.open(1, SETTING, "limitMode");
    r.consent.decide(&row.id, true, None, "mac", None).unwrap();
    r.eventually_state(&row.id, "failed");
    let mut item = r.eventually_item(&row.id, "answered");
    for _ in 0..500 {
        if item["outcome"]["state"] == "failed" {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
        item = r.eventually_item(&row.id, "answered");
    }
    assert_eq!(item["outcome"]["state"], "failed");
    assert_eq!(half_states(&seen), Vec::<String>::new());
}
