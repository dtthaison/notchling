use super::*;
use crate::acting_ops_tests::FakeRoom;
use crate::consent_testkit::{agent_caller, app_caller, verified};
use crate::event_assert::{of, text};
use crate::late_emit::Events;
use crate::limits_ops_tests::TempDir;
use notchling_engine::consent::ConsentStore;
use notchling_events::{MemorySink, Salt};
use notchling_local_api::ErrorCode;
use std::collections::HashMap;
use std::sync::Mutex;

struct Rig {
    _tmp: TempDir,
    inbox: Arc<InboxOps>,
    sink: Arc<MemorySink>,
    store: Arc<ConsentStore>,
    consent: Arc<ConsentOps>,
    asks: Arc<SwitchAsks>,
    on: Arc<Mutex<HashMap<String, bool>>>,
    turns: Arc<Mutex<Vec<(String, bool, Value)>>>,
}

impl Drop for Rig {
    fn drop(&mut self) {
        self.consent.close();
    }
}

fn wire(tmp: &TempDir, store: &Arc<ConsentStore>, sink: &Arc<MemorySink>) -> Rig {
    let room = Arc::new(FakeRoom::default());
    let inbox = InboxOps::new(tmp.path("inbox"), room.clone());
    let consent = ConsentOps::with_clock(
        store.clone(),
        room,
        Some(inbox.clone()),
        Some(Events::fixed(sink.clone(), Salt([3; 32]))),
        Arc::new(now_ms),
    );
    consent.bind_identity(Arc::new(|_| verified("s-test")));
    consent.start();
    let on: Arc<Mutex<HashMap<String, bool>>> = Arc::new(Mutex::new(HashMap::new()));
    let turns = Arc::new(Mutex::new(Vec::new()));
    let seen = on.clone();
    let states: States =
        Arc::new(move |name: &str| Some(*seen.lock().unwrap().get(name).unwrap_or(&false)));
    let (flip, log) = (on.clone(), turns.clone());
    let turn: Turn = Arc::new(move |name: &str, value: bool, extra: &Value| {
        flip.lock().unwrap().insert(name.to_string(), value);
        log.lock()
            .unwrap()
            .push((name.to_string(), value, extra.clone()));
        Ok(json!({"switch": name, "on": value}))
    });
    let asks = SwitchAsks::new(consent.clone(), states, turn.clone(), Some(inbox.clone()));
    Rig {
        _tmp: TempDir::new("switch-asks-unused"),
        inbox,
        sink: sink.clone(),
        store: store.clone(),
        consent,
        asks,
        on,
        turns,
    }
}

fn rig(tag: &str) -> Rig {
    let tmp = TempDir::new(&format!("switch-asks-{tag}"));
    let store = Arc::new(ConsentStore::in_memory_for_tests());
    let sink = Arc::new(MemorySink::new());
    let mut r = wire(&tmp, &store, &sink);
    r._tmp = tmp;
    r
}

impl Rig {
    fn ask(&self, name: &str) -> String {
        let reply = self
            .asks
            .call(
                &agent_caller(),
                ops::SWITCH_REQUEST,
                &json!({"switch": name, "on": true, "consent": {"reason": "please"}}),
            )
            .unwrap();
        assert_eq!(reply["state"], "waiting");
        reply["request"]["id"].as_str().unwrap().to_string()
    }

    fn app_turns_on(&self, op: &str, name: &str, id: &str) -> Result<Value, ApiError> {
        let args = match op {
            ops::SETTINGS_SET_SWITCH => json!({"switch": name, "on": true, "consentId": id}),
            _ => json!({"on": true, "consentId": id}),
        };
        let on = self.on.clone();
        let target = name.to_string();
        self.asks.apply(&app_caller(), op, &args, &move |_| {
            on.lock().unwrap().insert(target.clone(), true);
            Ok(json!({"on": true}))
        })
    }

    fn state(&self, id: &str) -> String {
        self.store
            .get(id)
            .unwrap()
            .unwrap()
            .state
            .as_str()
            .to_string()
    }

    fn is_on(&self, name: &str) -> bool {
        *self.on.lock().unwrap().get(name).unwrap_or(&false)
    }
}

#[test]
fn a_request_allowed_in_the_app_turns_the_switch_on_once_and_is_recorded() {
    let r = rig("allow");
    let id = r.ask("vault");
    assert!(!r.is_on("vault"), "asking turns nothing on");
    r.app_turns_on(ops::SETTINGS_SET_SWITCH, "vault", &id)
        .unwrap();
    assert!(r.is_on("vault"));
    assert_eq!(r.state(&id), "applied");
    let consents = of(&r.sink, "agent.consent");
    let thens: Vec<String> = consents.iter().filter_map(|e| text(e, "then")).collect();
    assert_eq!(thens.first().map(String::as_str), Some("asked"));
    assert_eq!(thens.last().map(String::as_str), Some("applied"));
    assert!(consents
        .iter()
        .all(|e| text(e, "switch").as_deref() == Some("vault")));
    assert!(consents
        .iter()
        .all(|e| text(e, "kind").as_deref() == Some("switch")));
}

#[test]
fn acting_and_the_bridge_are_applied_by_their_own_ops() {
    let r = rig("own-ops");
    let acting = r.ask("acting");
    r.app_turns_on(ops::SETTINGS_SET_ACTING, "acting", &acting)
        .unwrap();
    let bridge = r.ask("bridge");
    r.app_turns_on(ops::BRIDGE_SET, "bridge", &bridge).unwrap();
    assert!(r.is_on("acting") && r.is_on("bridge"));
}

#[test]
fn an_id_works_once_and_only_for_its_own_switch() {
    let r = rig("once");
    let id = r.ask("install");
    let other = r
        .app_turns_on(ops::SETTINGS_SET_SWITCH, "cluster", &id)
        .unwrap_err();
    assert_eq!(other.code, ErrorCode::Forbidden);
    assert!(!r.is_on("cluster"));
    r.app_turns_on(ops::SETTINGS_SET_SWITCH, "install", &id)
        .unwrap();
    r.on.lock().unwrap().insert("install".into(), false);
    let again = r
        .app_turns_on(ops::SETTINGS_SET_SWITCH, "install", &id)
        .unwrap_err();
    assert_eq!(again.code, ErrorCode::Forbidden);
    assert!(!r.is_on("install"));
}

#[test]
fn only_the_app_applies_and_never_an_agent_with_a_valid_id() {
    let r = rig("agent-tier");
    let id = r.ask("accounts");
    let args = json!({"switch": "accounts", "on": true, "consentId": id});
    let refused = r
        .asks
        .apply(&agent_caller(), ops::SETTINGS_SET_SWITCH, &args, &|_| {
            panic!("an agent never reaches the op")
        })
        .unwrap_err();
    assert_eq!(refused.code, ErrorCode::Forbidden);
    assert_eq!(r.state(&id), "waiting");
    assert!(!r.is_on("accounts"));
}

#[test]
fn a_phone_may_deny_but_its_allow_is_not_taken() {
    let r = rig("phone");
    let id = r.ask("acting");
    let _ = r.consent.decide(&id, true, None, "phone", None);
    assert_eq!(r.state(&id), "waiting");
    let denied = r.consent.decide(&id, false, None, "phone", None).unwrap();
    assert_eq!(denied["answered"], json!(true));
    assert_eq!(r.state(&id), "denied");
    assert!(!r.is_on("acting"));
}

#[test]
fn asking_for_a_switch_that_is_on_is_a_bad_request() {
    let r = rig("already");
    r.on.lock().unwrap().insert("cluster".into(), true);
    let e = r
        .asks
        .call(
            &agent_caller(),
            ops::SWITCH_REQUEST,
            &json!({"switch": "cluster", "on": true}),
        )
        .unwrap_err();
    assert_eq!(e.code, ErrorCode::BadRequest);
    assert_eq!(e.message, ALREADY_ON);
}

#[test]
fn a_yes_for_a_switch_turned_on_meanwhile_is_withdrawn() {
    let r = rig("meanwhile");
    let id = r.ask("vault");
    r.on.lock().unwrap().insert("vault".into(), true);
    let e = r
        .app_turns_on(ops::SETTINGS_SET_SWITCH, "vault", &id)
        .unwrap_err();
    assert_eq!(e.message, ALREADY_ON);
    assert_eq!(r.state(&id), "withdrawn");
}

#[test]
fn a_restart_withdraws_an_applying_yes_and_every_switch_starts_off() {
    let r = rig("restart");
    let id = r.ask("bridge");
    r.consent
        .use_for_app(&app_caller(), &id, SWITCH, "bridge")
        .unwrap();
    assert_eq!(r.state(&id), "applying");
    r.consent.close();
    let tmp = TempDir::new("switch-asks-restart-2");
    let again = wire(&tmp, &r.store, &r.sink);
    again.consent.load();
    assert_eq!(r.state(&id), "withdrawn");
    assert!(crate::switches::Switches::off()
        .all()
        .iter()
        .all(|s| !s.is_on()));
    assert!(NAMES.iter().all(|name| !again.is_on(name)));
}

#[test]
fn an_agent_turns_a_switch_off_without_asking_and_the_user_gets_turn_on() {
    let r = rig("off");
    r.on.lock().unwrap().insert("cluster".into(), true);
    let reply = r
        .asks
        .call(
            &agent_caller(),
            ops::SWITCH_OFF,
            &json!({"switch": "cluster"}),
        )
        .unwrap();
    assert_eq!(
        reply,
        json!({"switch": "cluster", "on": false, "changed": true})
    );
    assert!(!r.is_on("cluster"));
    assert_eq!(r.turns.lock().unwrap()[0].2, json!({"agentOff": true}));
    r.inbox.flush();
    let notice = r
        .inbox
        .items()
        .into_iter()
        .find(|i| i.id.starts_with("switch-off:cluster:"))
        .expect("a notice");
    assert_eq!(notice.title, "Claude Code turned off the cluster");
    assert_eq!(notice.actions[0].label, "Turn On");
    let again = r
        .asks
        .call(
            &agent_caller(),
            ops::SWITCH_OFF,
            &json!({"switch": "cluster"}),
        )
        .unwrap();
    assert_eq!(again["changed"], json!(false));
}

#[test]
fn turn_on_in_the_notice_works_from_the_mac_only() {
    let r = rig("turn-on");
    let responder = TurnOn {
        name: "vault".into(),
        turn: {
            let on = r.on.clone();
            Arc::new(move |name: &str, value: bool, _: &Value| {
                on.lock().unwrap().insert(name.to_string(), value);
                Ok(json!({}))
            })
        },
    };
    let item = off_notice("vault", "claude", None, 1);
    assert!(!responder.answered_by(&item, ActionId::Continue, "phone"));
    assert!(!r.is_on("vault"));
    assert!(responder.answered_by(&item, ActionId::Continue, "mac"));
    assert!(r.is_on("vault"));
}

#[test]
fn the_app_ops_without_an_id_are_not_cited() {
    assert!(!cites(
        ops::SETTINGS_SET_SWITCH,
        &json!({"switch": "vault", "on": true})
    ));
    assert!(cites(
        ops::BRIDGE_SET,
        &json!({"on": true, "consentId": "c1"})
    ));
    assert!(!cites(ops::SETTINGS_SET, &json!({"consentId": "c1"})));
}
