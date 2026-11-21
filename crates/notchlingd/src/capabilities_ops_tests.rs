use super::*;
use crate::acting_ops_tests::FakeRoom;
use notchling_local_api::ClientKind;
use std::collections::BTreeSet;
use std::sync::atomic::Ordering;

fn caller(tier: Tier, pid: Option<u32>) -> Caller {
    Caller {
        tier,
        client: if tier == Tier::App {
            ClientKind::App
        } else {
            ClientKind::Cli
        },
        peer: None,
        pid,
    }
}

fn group(on: &'static [&'static str], room: Arc<FakeRoom>) -> CapabilitiesOps {
    CapabilitiesOps::new(
        "9.9.9",
        Arc::new(move |name| on.contains(&name)),
        Arc::new(|pid| (pid == 7).then(|| "session-seven".to_string())),
        room,
    )
}

fn ask(group: &CapabilitiesOps, caller: &Caller) -> Value {
    group
        .call(caller, ops::DAEMON_CAPABILITIES, &Value::Null)
        .unwrap()
}

fn now_of(answer: &Value, op: &str) -> Value {
    answer["ops"]
        .as_array()
        .unwrap()
        .iter()
        .find(|o| o["op"] == op)
        .unwrap_or_else(|| panic!("{op} is not listed"))
        .clone()
}

#[test]
fn it_is_an_agent_read_op_and_nothing_else() {
    let g = group(&[], Arc::new(FakeRoom::default()));
    assert_eq!(g.spec(ops::DAEMON_CAPABILITIES), Some(OpSpec::AGENT_READ));
    assert_eq!(g.spec(ops::DAEMON_STATUS), None);
    assert!(g
        .call(&caller(Tier::Agent, None), "nope", &Value::Null)
        .is_err());
}

#[test]
fn a_verified_session_gets_its_id_and_the_switches() {
    let g = group(&["vault"], Arc::new(FakeRoom::default()));
    let answer = ask(&g, &caller(Tier::Agent, Some(7)));
    assert_eq!(answer["api"], API_VERSION);
    assert_eq!(answer["daemon"], "9.9.9");
    assert_eq!(answer["caller"]["tier"], "agent");
    assert_eq!(answer["caller"]["tool"], "claude");
    assert_eq!(answer["caller"]["verified"], true);
    assert_eq!(answer["caller"]["session"], "session-seven");
    assert_eq!(answer["switches"]["vault"], true);
    assert_eq!(answer["switches"]["cluster"], false);
    assert_eq!(
        answer["switches"].as_object().unwrap().len(),
        SWITCH_NAMES.len()
    );
}

#[test]
fn a_shell_is_unverified_and_has_no_session() {
    let g = group(&[], Arc::new(FakeRoom::default()));
    for pid in [Some(8), None] {
        let answer = ask(&g, &caller(Tier::Agent, pid));
        assert_eq!(answer["caller"]["tool"], "shell");
        assert_eq!(answer["caller"]["verified"], false);
        assert!(answer["caller"].get("session").is_none());
        assert!(answer["caller"].get("folder").is_none());
        assert!(answer["caller"].get("organization").is_none());
    }
}

#[test]
fn the_app_sees_every_op_open_and_no_session() {
    let g = group(&[], Arc::new(FakeRoom::default()));
    let answer = ask(&g, &caller(Tier::App, Some(7)));
    assert_eq!(answer["caller"]["tier"], "app");
    assert_eq!(answer["caller"]["tool"], "app");
    assert!(answer["caller"].get("session").is_none());
    assert_eq!(now_of(&answer, "session.permission")["now"], "open");
    assert_eq!(now_of(&answer, "vault.find")["now"], "open");
}

#[test]
fn the_app_block_follows_the_room() {
    let room = Arc::new(FakeRoom::default());
    let g = group(&[], room.clone());
    let answer = ask(&g, &caller(Tier::Agent, None));
    assert_eq!(
        answer["app"],
        json!({"attached": false, "followsInbox": false})
    );
    room.app.store(true, Ordering::SeqCst);
    room.following
        .lock()
        .unwrap()
        .push(topics::INBOX.to_string());
    let answer = ask(&g, &caller(Tier::Agent, None));
    assert_eq!(
        answer["app"],
        json!({"attached": true, "followsInbox": true})
    );
}

#[test]
fn each_op_says_what_it_is_for_this_caller() {
    let off = group(&[], Arc::new(FakeRoom::default()));
    let on = group(&["vault"], Arc::new(FakeRoom::default()));
    let agent = caller(Tier::Agent, Some(7));
    let a = ask(&off, &agent);
    let b = ask(&on, &agent);
    assert_eq!(
        now_of(&a, "vault.find"),
        json!({"op": "vault.find", "now": "needs_switch", "switch": "vault"})
    );
    assert_eq!(now_of(&b, "vault.find")["now"], "open");
    assert_eq!(now_of(&a, "settings.get")["now"], "open");
    assert_eq!(now_of(&a, "session.permission")["now"], "never");
    assert_eq!(
        now_of(&a, "session.new"),
        json!({"op": "session.new", "now": "not_yet", "phase": 4})
    );
    assert_eq!(now_of(&a, "git.status")["now"], "app_only");
    assert_eq!(now_of(&a, "hook.request")["now"], "internal");
    assert_eq!(now_of(&a, "daemon.capabilities")["now"], "open");
}

#[test]
fn every_op_is_listed_once_with_a_closed_now() {
    let g = group(&[], Arc::new(FakeRoom::default()));
    let answer = ask(&g, &caller(Tier::Agent, Some(7)));
    let listed = answer["ops"].as_array().unwrap();
    let names: BTreeSet<&str> = listed.iter().map(|o| o["op"].as_str().unwrap()).collect();
    assert_eq!(names.len(), listed.len());
    assert_eq!(names.len(), crate::commands::all_ops().len());
    let closed = [
        "open",
        "needs_consent",
        "needs_switch",
        "app_only",
        "not_yet",
        "never",
        "internal",
    ];
    for o in listed {
        assert!(closed.contains(&o["now"].as_str().unwrap()), "{o}");
    }
}

#[test]
fn the_answer_has_only_the_documented_parts() {
    let g = group(&["vault"], Arc::new(FakeRoom::default()));
    let answer = ask(&g, &caller(Tier::Agent, Some(7)));
    let keys: BTreeSet<&str> = answer
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(
        keys,
        BTreeSet::from(["api", "app", "caller", "daemon", "ops", "switches"])
    );
    let who: BTreeSet<&str> = answer["caller"]
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(who, BTreeSet::from(["session", "tier", "tool", "verified"]));
}
