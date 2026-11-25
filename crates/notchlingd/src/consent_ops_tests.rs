use super::*;
use crate::acting_ops_tests::FakeRoom;
use crate::consent_testkit::{
    agent_caller, app_caller, call_op, item_for, item_id_for, shell, verified,
};
use crate::event_assert::{of, text};
use crate::limits_ops_tests::TempDir;
use notchling_engine::consent::{ConsentStore, SECRET_REQUEST, SETTING};
use notchling_events::{MemorySink, Salt};
use std::sync::atomic::{AtomicI64, Ordering};

struct Applies;

impl Handler for Applies {
    fn apply(&self, _: &Consent, _: Option<Answerer>, _: Option<&str>) -> Result<Value, String> {
        Ok(json!({"applied": true}))
    }
}

pub(super) struct Rig {
    pub(super) _tmp: TempDir,
    pub(super) inbox: Arc<InboxOps>,
    pub(super) sink: Arc<MemorySink>,
    pub(super) store: Arc<ConsentStore>,
    pub(super) clock: Arc<AtomicI64>,
    pub(super) consent: Arc<ConsentOps>,
}

pub(super) fn at(pid: u32) -> Caller {
    Caller {
        pid: Some(pid),
        ..agent_caller()
    }
}

fn identity(pid: u32) -> AgentIdentity {
    match pid {
        1 | 11 => verified("s-a"),
        2 => verified("s-b"),
        _ => shell("Terminal"),
    }
}

pub(super) fn build(
    tmp: &TempDir,
    room: &Arc<FakeRoom>,
    store: &Arc<ConsentStore>,
    sink: &Arc<MemorySink>,
    clock: &Arc<AtomicI64>,
) -> (Arc<InboxOps>, Arc<ConsentOps>) {
    let inbox = InboxOps::new(tmp.path("inbox"), room.clone());
    let ticking = clock.clone();
    let consent = ConsentOps::with_clock(
        store.clone(),
        room.clone(),
        Some(inbox.clone()),
        Some(Events::fixed(sink.clone(), Salt([3; 32]))),
        Arc::new(move || ticking.load(Ordering::SeqCst)),
    );
    consent.bind_identity(Arc::new(identity));
    consent.register(SETTING, Arc::new(Applies));
    consent.start();
    (inbox, consent)
}

pub(super) fn rig(tag: &str) -> Rig {
    let tmp = TempDir::new(&format!("consent-{tag}"));
    let room = Arc::new(FakeRoom::default());
    let store = Arc::new(ConsentStore::in_memory_for_tests());
    let sink = Arc::new(MemorySink::new());
    let clock = Arc::new(AtomicI64::new(1_000_000));
    let (inbox, consent) = build(&tmp, &room, &store, &sink, &clock);
    Rig {
        _tmp: tmp,
        inbox,
        sink,
        store,
        clock,
        consent,
    }
}

impl Drop for Rig {
    fn drop(&mut self) {
        self.consent.close();
    }
}

pub(super) fn opening(kind: &'static str, target: &str, reason: Option<&str>) -> Opening {
    Opening {
        parts: None,
        kind,
        target: target.to_string(),
        change: json!({"key": target, "title": target, "before": 1, "after": 2, "text": false,
                       "name": target}),
        facts: json!([{"line": "consent-test", "args": {"target": target}}]),
        request: json!({"op": "test", "target": target}),
        reason: reason.map(str::to_string),
        expires_in_ms: None,
        key: None,
    }
}

impl Rig {
    pub(super) fn open(&self, pid: u32, kind: &'static str, target: &str) -> Consent {
        self.consent
            .open(&at(pid), opening(kind, target, None))
            .unwrap()
            .row()
            .clone()
    }

    pub(super) fn state(&self, id: &str) -> String {
        self.store
            .get(id)
            .unwrap()
            .unwrap()
            .state
            .as_str()
            .to_string()
    }

    pub(super) fn eventually_state(&self, id: &str, want: &str) {
        for _ in 0..500 {
            if self.state(id) == want {
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(self.state(id), want);
    }

    pub(super) fn eventually_item(&self, id: &str, want: &str) -> Value {
        for _ in 0..500 {
            let item = item_for(&self.inbox, id);
            if item["state"] == want {
                return item;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        item_for(&self.inbox, id)
    }

    pub(super) fn thens(&self) -> Vec<String> {
        of(&self.sink, "agent.consent")
            .iter()
            .filter_map(|e| text(e, "then"))
            .collect()
    }
}

#[test]
fn a_request_opens_with_no_app_attached_and_waits() {
    let r = rig("no-app");
    assert!(!r.consent.app_attached());
    let row = r.open(1, SETTING, "limitMode");
    assert_eq!(row.state.as_str(), "waiting");
    assert!(row.id.starts_with('c') && row.id.len() == 33);
    assert_eq!(item_for(&r.inbox, &row.id)["state"], "open");
    assert_eq!(r.thens(), ["asked"]);
}

#[test]
fn only_the_asking_verified_session_reads_or_withdraws_its_request() {
    let r = rig("who");
    let row = r.open(1, SETTING, "limitMode");
    let id = json!({"id": row.id});
    for pid in [1, 11] {
        let got = call_op(&r.consent, &at(pid), ops::CONSENT_GET, id.clone());
        assert_eq!(got["state"], "waiting", "pid {pid} is in the same session");
    }
    for pid in [2, 3] {
        let got = call_op(&r.consent, &at(pid), ops::CONSENT_GET, id.clone());
        assert_eq!(got["code"], "forbidden", "pid {pid}");
        let got = call_op(&r.consent, &at(pid), ops::CONSENT_CANCEL, id.clone());
        assert_eq!(got["code"], "forbidden", "pid {pid}");
    }
    let err = r.consent.call(&at(2), ops::CONSENT_GET, &id).unwrap_err();
    assert_eq!(err.data.unwrap()["reason"], "other_session");
    assert_eq!(r.state(&row.id), "waiting");
    let got = call_op(&r.consent, &at(11), ops::CONSENT_CANCEL, id);
    assert_eq!(got, json!({"cancelled": true, "state": "cancelled"}));
}

#[test]
fn a_request_from_no_verified_session_is_read_by_its_id_alone() {
    let r = rig("id-only");
    let row = r.open(3, SETTING, "limitMode");
    let id = json!({"id": row.id});
    for pid in [1, 2, 3] {
        let got = call_op(&r.consent, &at(pid), ops::CONSENT_GET, id.clone());
        assert_eq!(got["state"], "waiting", "pid {pid}");
    }
    for pid in [1, 3] {
        let mine = call_op(&r.consent, &at(pid), ops::CONSENT_MINE, json!({}));
        assert_eq!(mine, json!({"requests": []}), "no list for pid {pid}");
    }
    let got = call_op(&r.consent, &at(2), ops::CONSENT_CANCEL, id);
    assert_eq!(got["cancelled"], true);
}

#[test]
fn mine_lists_the_sessions_requests_newest_first_and_filters_by_state() {
    let r = rig("mine");
    let first = r.open(1, SETTING, "limitMode");
    r.clock.fetch_add(10, Ordering::SeqCst);
    let second = r.open(1, SETTING, "limitSwitchAt");
    r.open(2, SETTING, "showPet");
    call_op(
        &r.consent,
        &at(1),
        ops::CONSENT_CANCEL,
        json!({"id": first.id}),
    );
    let all = call_op(&r.consent, &at(11), ops::CONSENT_MINE, json!({}));
    let ids: Vec<&str> = all["requests"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v["id"].as_str().unwrap())
        .collect();
    assert_eq!(ids, [second.id.as_str(), first.id.as_str()]);
    let waiting = call_op(
        &r.consent,
        &at(1),
        ops::CONSENT_MINE,
        json!({"states": ["waiting"]}),
    );
    assert_eq!(waiting["requests"].as_array().unwrap().len(), 1);
    assert_eq!(waiting["requests"][0]["id"], second.id.as_str());
}

#[test]
fn cancelling_a_closed_request_says_so_and_withdraws_nothing() {
    let r = rig("cancel-twice");
    let row = r.open(1, SETTING, "limitMode");
    let id = json!({"id": row.id});
    let first = call_op(&r.consent, &at(1), ops::CONSENT_CANCEL, id.clone());
    assert_eq!(first["cancelled"], true);
    let again = call_op(&r.consent, &at(1), ops::CONSENT_CANCEL, id);
    assert_eq!(again, json!({"cancelled": false, "state": "cancelled"}));
    let item = r.eventually_item(&row.id, "withdrawn");
    assert_eq!(item["state"], "withdrawn");
    assert_eq!(r.thens(), ["asked", "cancelled"]);
}

#[test]
fn the_first_answer_wins_whether_it_comes_from_the_inbox_or_the_app() {
    let r = rig("first");
    let row = r.open(1, SETTING, "limitMode");
    let item = item_id_for(&r.inbox, &row.id);
    let won = r.inbox.decide(&item, ActionId::Allow, "phone").unwrap();
    assert!(won.answered);
    let late = call_op(
        &r.consent,
        &app_caller(),
        ops::CONSENT_ANSWER,
        json!({"consentId": row.id, "decision": "deny"}),
    );
    assert_eq!(late["answered"], false);
    r.eventually_state(&row.id, "applied");
    let stored = r.store.get(&row.id).unwrap().unwrap();
    assert_eq!(stored.by.as_deref(), Some("phone"));
    assert_eq!(stored.result, Some(json!({"applied": true})));
    let again = r.inbox.decide(&item, ActionId::Deny, "mac").unwrap();
    assert!(!again.answered);
    assert_eq!(r.thens(), ["asked", "allowed", "applied"]);
}

#[test]
fn a_mac_only_request_has_no_allow_action_and_a_phone_allow_is_not_taken() {
    let r = rig("mac-only");
    let row = r.open(1, SECRET_REQUEST, "npm");
    let item = item_for(&r.inbox, &row.id);
    let actions: Vec<&str> = item["actions"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|a| a["id"].as_str())
        .collect();
    assert!(!actions.contains(&"allow"), "{actions:?}");
    assert!(actions.contains(&"deny"));
    let taken = r
        .inbox
        .decide(&item_id_for(&r.inbox, &row.id), ActionId::Allow, "phone");
    assert!(taken.is_err(), "the item offers no allow");
    assert_eq!(r.state(&row.id), "waiting");
    let refused = call_op(
        &r.consent,
        &app_caller(),
        ops::CONSENT_ANSWER,
        json!({"consentId": row.id, "decision": "allow"}),
    );
    assert_eq!(refused["code"], "forbidden");
    assert_eq!(r.state(&row.id), "waiting");
    let denied = r
        .inbox
        .decide(&item_id_for(&r.inbox, &row.id), ActionId::Deny, "phone")
        .unwrap();
    assert!(denied.answered);
    r.eventually_state(&row.id, "denied");
}

#[test]
fn no_agent_tier_op_answers_a_request() {
    let r = rig("agent-answers");
    for op in [ops::CONSENT_ANSWER, ops::CONSENT_OPENED, ops::CONSENT_LIST] {
        assert_eq!(r.consent.spec(op).unwrap().tier, Tier::App, "{op}");
    }
    for op in [ops::CONSENT_GET, ops::CONSENT_MINE, ops::CONSENT_CANCEL] {
        assert_eq!(r.consent.spec(op).unwrap().tier, Tier::Agent, "{op}");
    }
    assert_eq!(
        r.consent.spec(ops::CONSENT_CANCEL),
        Some(OpSpec::AGENT_ACTS)
    );
    assert_eq!(r.consent.spec(ops::CONSENT_ANSWER), Some(OpSpec::APP));
}

#[test]
fn an_unanswered_request_expires_at_its_end_and_closes_its_item() {
    let r = rig("expires");
    let row = r.open(1, SETTING, "limitMode");
    let span = row.expires_ms - row.created_ms;
    assert_eq!(span, 15 * 60 * 1000);
    r.clock.fetch_add(span - 1, Ordering::SeqCst);
    r.consent.tick();
    assert_eq!(r.state(&row.id), "waiting");
    r.clock.fetch_add(2, Ordering::SeqCst);
    r.consent.tick();
    assert_eq!(r.state(&row.id), "expired");
    assert_eq!(r.eventually_item(&row.id, "expired")["state"], "expired");
    let late = call_op(
        &r.consent,
        &app_caller(),
        ops::CONSENT_ANSWER,
        json!({"consentId": row.id, "decision": "allow"}),
    );
    assert_eq!(late["answered"], false);
    assert_eq!(r.thens(), ["asked", "expired"]);
}

#[test]
fn opening_the_sheet_moves_the_end_out_once() {
    let r = rig("opened");
    let row = r.open(1, SECRET_REQUEST, "npm");
    r.clock.fetch_add(14 * 60 * 1000, Ordering::SeqCst);
    let now = r.clock.load(Ordering::SeqCst);
    let state = call_op(
        &r.consent,
        &app_caller(),
        ops::CONSENT_OPENED,
        json!({"consentId": row.id}),
    );
    assert_eq!(state["state"], "opened");
    let moved = r.store.get(&row.id).unwrap().unwrap();
    assert!(
        moved.expires_ms >= now + 10 * 60 * 1000,
        "{}",
        moved.expires_ms
    );
    r.clock.fetch_add(5 * 60 * 1000, Ordering::SeqCst);
    call_op(
        &r.consent,
        &app_caller(),
        ops::CONSENT_OPENED,
        json!({"consentId": row.id}),
    );
    let again = r.store.get(&row.id).unwrap().unwrap();
    assert_eq!(again.expires_ms, moved.expires_ms, "moved once");
    assert_eq!(r.thens(), ["asked", "opened"]);
}

#[test]
fn a_restart_keeps_a_waiting_request_sends_opened_back_and_raises_fresh_items() {
    let r = rig("restart");
    let waiting = r.open(1, SETTING, "limitMode");
    let sheet = r.open(1, SECRET_REQUEST, "npm");
    call_op(
        &r.consent,
        &app_caller(),
        ops::CONSENT_OPENED,
        json!({"consentId": sheet.id}),
    );
    assert_eq!(r.state(&sheet.id), "opened");
    r.consent.close();
    let tmp = TempDir::new("consent-restart-2");
    let room = Arc::new(FakeRoom::default());
    let sink = Arc::new(MemorySink::new());
    let (inbox, again) = build(&tmp, &room, &r.store, &sink, &r.clock);
    again.load();
    assert_eq!(r.state(&waiting.id), "waiting");
    assert_eq!(r.state(&sheet.id), "waiting");
    for id in [&waiting.id, &sheet.id] {
        assert_eq!(item_for(&inbox, id)["state"], "open", "{id}");
    }
    let thens: Vec<String> = of(&sink, "agent.consent")
        .iter()
        .filter_map(|e| text(e, "then"))
        .collect();
    assert_eq!(thens, ["restored", "restored"]);
    again.close();
}

#[test]
fn a_reason_is_cut_to_the_limit_and_never_reaches_the_item_or_an_event() {
    let r = rig("reason");
    let planted = "TOKEN-123 ".repeat(60);
    let row = r
        .consent
        .open(&at(1), opening(SETTING, "limitMode", Some(&planted)))
        .unwrap()
        .row()
        .clone();
    assert_eq!(row.reason.as_ref().unwrap().chars().count(), 300);
    let outside = format!("{}{:?}", item_for(&r.inbox, &row.id), r.sink.events());
    assert!(!outside.contains("TOKEN-123"), "{outside}");
    call_op(
        &r.consent,
        &at(1),
        ops::CONSENT_CANCEL,
        json!({"id": row.id}),
    );
    assert_eq!(r.store.get(&row.id).unwrap().unwrap().reason, None);
}
