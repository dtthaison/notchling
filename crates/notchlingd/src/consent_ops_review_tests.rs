use super::tests::{at, build, opening, rig};
use super::*;
use crate::acting_ops_tests::FakeRoom;
use crate::consent_testkit::{app_caller, call_op, item_for, item_id_for, shell};
use crate::event_assert::{of, text};
use crate::limits_ops_tests::TempDir;
use notchling_engine::consent::{ConsentStore, Taken, SECRET_REQUEST, SETTING};
use notchling_events::MemorySink;
use std::sync::atomic::{AtomicI64, AtomicUsize, Ordering};

struct Counts(Arc<AtomicUsize>);

impl Handler for Counts {
    fn apply(&self, _: &Consent, _: Option<Answerer>, _: Option<&str>) -> Result<Value, String> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Ok(json!({"applied": true}))
    }
}

fn from(client: ClientKind) -> Caller {
    Caller {
        client,
        ..app_caller()
    }
}

#[test]
fn only_the_app_client_answers_opens_or_lists_a_request() {
    let r = rig("app-client");
    let row = r.open(1, SECRET_REQUEST, "npm");
    for client in [ClientKind::Cli, ClientKind::Hook, ClientKind::Node] {
        let caller = from(client);
        for (op, args) in [
            (
                ops::CONSENT_ANSWER,
                json!({"consentId": row.id, "decision": "deny"}),
            ),
            (ops::CONSENT_OPENED, json!({"consentId": row.id})),
            (ops::CONSENT_LIST, json!({})),
        ] {
            let err = r.consent.call(&caller, op, &args).unwrap_err();
            assert_eq!(err.data.unwrap()["reason"], "app_only", "{op} {client:?}");
        }
    }
    assert_eq!(r.state(&row.id), "waiting");
    assert_eq!(r.thens(), ["asked"]);
}

#[test]
fn opening_the_sheet_changes_nothing_for_a_request_the_daemon_applies() {
    let r = rig("sheet-kinds");
    let row = r.open(1, SETTING, "limitMode");
    let state = call_op(
        &r.consent,
        &app_caller(),
        ops::CONSENT_OPENED,
        json!({"consentId": row.id}),
    );
    assert_eq!(state["state"], "waiting");
    let stored = r.store.get(&row.id).unwrap().unwrap();
    assert_eq!(stored.expires_ms, row.expires_ms);
    assert!(!stored.extended);
    assert_eq!(r.thens(), ["asked"]);
}

#[test]
fn an_answer_that_lands_after_the_end_is_withdrawn_and_never_applied() {
    let r = rig("late-apply");
    let hits = Arc::new(AtomicUsize::new(0));
    r.consent.register(SETTING, Arc::new(Counts(hits.clone())));
    let row = r.open(1, SETTING, "limitMode");
    let now = r.clock.load(Ordering::SeqCst);
    let taken = r
        .store
        .answer(&row.id, State::Applying, "mac", None, now)
        .unwrap();
    assert!(matches!(taken, Taken::Won(_)));
    r.clock
        .fetch_add(row.expires_ms - now + 1, Ordering::SeqCst);
    r.consent.apply(r.store.get(&row.id).unwrap().unwrap());
    assert_eq!(r.state(&row.id), "withdrawn");
    assert_eq!(hits.load(Ordering::SeqCst), 0);
    assert!(!r.thens().contains(&"applied".to_string()));
}

#[test]
fn a_restart_after_the_end_closes_the_request_as_expired_and_raises_nothing() {
    let r = rig("late-restart");
    let row = r.open(1, SETTING, "limitMode");
    r.consent.close();
    r.clock
        .fetch_add(row.expires_ms - row.created_ms + 1, Ordering::SeqCst);
    let tmp = TempDir::new("consent-late-restart-2");
    let room = Arc::new(FakeRoom::default());
    let sink = Arc::new(MemorySink::new());
    let (inbox, again) = build(&tmp, &room, &r.store, &sink, &r.clock);
    again.load();
    assert_eq!(r.state(&row.id), "expired");
    assert_eq!(item_for(&inbox, &row.id), Value::Null);
    let thens: Vec<String> = of(&sink, "agent.consent")
        .iter()
        .filter_map(|e| text(e, "then"))
        .collect();
    assert_eq!(thens, ["expired"]);
    again.close();
}

#[test]
fn a_store_that_cannot_open_refuses_every_request_and_raises_nothing() {
    let tmp = TempDir::new("consent-store-down");
    let room = Arc::new(FakeRoom::default());
    let sink = Arc::new(MemorySink::new());
    let clock = Arc::new(AtomicI64::new(1_000_000));
    let store = Arc::new(ConsentStore::unavailable());
    let (inbox, consent) = build(&tmp, &room, &store, &sink, &clock);
    let refused = consent.open(&at(1), opening(SETTING, "limitMode", None));
    assert!(refused.is_err());
    assert!(of(&sink, "agent.consent").is_empty());
    inbox.flush();
    assert!(inbox.items().is_empty());
    consent.close();
}

#[test]
fn cancelling_is_recorded_and_the_call_reports_the_state_it_really_ended_in() {
    let r = rig("cancel-record");
    scope::take_consent();
    let row = r.open(1, SETTING, "limitMode");
    assert_eq!(
        scope::take_consent(),
        Some((row.id.clone(), ConsentNote::Pending))
    );
    assert_eq!(r.sink.commits(), 1);
    call_op(
        &r.consent,
        &at(1),
        ops::CONSENT_CANCEL,
        json!({"id": row.id}),
    );
    assert_eq!(
        scope::take_consent(),
        Some((row.id.clone(), ConsentNote::Used))
    );
    assert_eq!(r.sink.commits(), 2);
    assert_eq!(r.thens(), ["asked", "cancelled"]);
}

#[test]
fn a_retry_after_the_answer_returns_the_original_and_notes_it_used() {
    let r = rig("retry-after");
    let mut first = opening(SETTING, "limitMode", None);
    first.key = Some("k1".into());
    let row = r.consent.open(&at(1), first).unwrap().row().clone();
    let item = item_id_for(&r.inbox, &row.id);
    assert!(
        r.inbox
            .decide(&item, ActionId::Allow, "mac")
            .unwrap()
            .answered
    );
    r.eventually_state(&row.id, "applied");
    scope::take_consent();
    let mut again = opening(SETTING, "limitMode", None);
    again.key = Some("k1".into());
    again.change = json!({"key": "limitMode", "after": 99});
    let same = r.consent.open(&at(1), again).unwrap();
    assert!(matches!(same, Opened::Same(_)));
    assert_eq!(same.row().id, row.id);
    assert_eq!(same.row().state.as_str(), "applied");
    assert_eq!(
        scope::take_consent(),
        Some((row.id.clone(), ConsentNote::Used))
    );
}

#[test]
fn the_same_key_for_a_different_request_is_refused() {
    let r = rig("key-reused");
    let mut first = opening(SETTING, "limitMode", None);
    first.key = Some("k1".into());
    r.consent.open(&at(1), first).unwrap();
    let mut other = opening(SETTING, "showPet", None);
    other.key = Some("k1".into());
    other.request = json!({"op": "test", "target": "showPet"});
    let err = r.consent.open(&at(1), other).err().unwrap();
    assert_eq!(err.data.unwrap()["reason"], "key_reused");
}

#[test]
fn an_unverified_caller_meets_the_cooldown_a_verified_denial_left() {
    let r = rig("cooldown");
    let row = r.open(1, SETTING, "limitMode");
    assert!(r.consent.deny_by_app(&row.id));
    let forked = r
        .consent
        .open(&at(3), opening(SETTING, "limitMode", None))
        .err()
        .unwrap();
    assert_eq!(forked.data.unwrap()["reason"], "busy");
}

#[test]
fn unverified_callers_share_one_waiting_cap_whatever_their_root() {
    let r = rig("shared-cap");
    for n in 0..notchling_engine::consent::UNVERIFIED_WAITING {
        let _bound = notchling_local_api::bind_identity(Some(shell(&format!("root{n}"))));
        let row = opening(SETTING, &format!("target{n}"), None);
        r.consent.open(&at(3), row).unwrap();
    }
    let _bound = notchling_local_api::bind_identity(Some(shell("another-root")));
    let over = r
        .consent
        .open(&at(3), opening(SETTING, "one-more", None))
        .err()
        .unwrap();
    assert_eq!(over.data.unwrap()["reason"], "busy");
}

#[test]
fn the_view_names_the_folder_and_root_the_request_came_from() {
    let r = rig("view-where");
    let _bound = notchling_local_api::bind_identity(Some(AgentIdentity {
        folder: Some("/w/app".into()),
        ..crate::consent_testkit::verified("s-a")
    }));
    let row = r.open(1, SETTING, "limitMode");
    let seen = call_op(&r.consent, &at(1), ops::CONSENT_GET, json!({"id": row.id}));
    assert_eq!(seen["folder"], "/w/app");
    assert_eq!(seen["root"], "Terminal");
}

struct LateCheck {
    clock: Arc<AtomicI64>,
    to: i64,
    hits: Arc<AtomicUsize>,
}

impl Handler for LateCheck {
    fn recheck(&self, _: &Consent) -> Result<(), String> {
        self.clock.store(self.to, Ordering::SeqCst);
        Ok(())
    }

    fn apply(&self, _: &Consent, _: Option<Answerer>, _: Option<&str>) -> Result<Value, String> {
        self.hits.fetch_add(1, Ordering::SeqCst);
        Ok(json!({"applied": true}))
    }
}

#[test]
fn an_end_that_passes_after_the_audit_commit_withdraws_before_the_change() {
    let r = rig("late-commit");
    let hits = Arc::new(AtomicUsize::new(0));
    let row = r.open(1, SETTING, "limitMode");
    r.consent.register(
        SETTING,
        Arc::new(LateCheck {
            clock: r.clock.clone(),
            to: row.expires_ms + 1,
            hits: hits.clone(),
        }),
    );
    let now = r.clock.load(Ordering::SeqCst);
    r.store
        .answer(&row.id, State::Applying, "mac", None, now)
        .unwrap();
    r.consent.apply(r.store.get(&row.id).unwrap().unwrap());
    assert_eq!(hits.load(Ordering::SeqCst), 0);
    assert_eq!(r.state(&row.id), "withdrawn");
}

#[test]
fn an_app_tier_caller_that_is_not_the_app_sees_only_what_an_agent_sees() {
    let r = rig("app-tier-cli");
    let row = r.open(1, SETTING, "limitMode");
    for client in [ClientKind::Cli, ClientKind::Hook, ClientKind::Node] {
        let caller = Caller {
            pid: Some(2),
            ..from(client)
        };
        let err = r
            .consent
            .call(&caller, ops::CONSENT_GET, &json!({"id": row.id}))
            .unwrap_err();
        assert_eq!(err.data.unwrap()["reason"], "other_session", "{client:?}");
        assert!(r
            .consent
            .pending_for(&caller, SETTING, "limitMode")
            .is_empty());
    }
    assert_eq!(
        r.consent
            .pending_for(&app_caller(), SETTING, "limitMode")
            .len(),
        1
    );
}
