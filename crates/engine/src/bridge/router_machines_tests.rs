use super::connection::Connection;
use super::relay::Relay;
use super::test_support::{data, error_code, is_ok, Plan, RecordingIo, Rig, ScriptedRelay};
use super::wire::BridgeFailure;
use notchling_protocol::generated::ClusterMachine;
use serde_json::{json, Value};
use std::sync::Arc;

fn machine(id: &str, name: &str, can_be_master: bool, set_at: Option<i64>) -> ClusterMachine {
    ClusterMachine {
        id: id.into(),
        name: name.into(),
        online: true,
        can_be_master,
        ready: true,
        set_at,
        can_resume: can_be_master && set_at.is_some(),
    }
}

fn ask(rig: &Rig, connection: &Arc<Connection>, io: &RecordingIo, body: Value) -> Value {
    rig.send(connection, &body.to_string());
    io.last_json()
}

fn last_audit(rig: &Rig) -> Option<String> {
    rig.audit.recent().first().map(|e| e.op.clone())
}

#[test]
fn machines_op_lists_online_machines_with_set_time() {
    let rig = Rig::new();
    rig.backend.with(|s| {
        s.machines = vec![
            machine("1111111111111111", "m1", true, Some(1_793_541_465_000)),
            machine("3333333333333333", "m3", true, None),
            machine("4444444444444444", "pi", false, None),
        ]
    });
    let (connection, io, _) = rig.login("phone");
    let reply = ask(
        &rig,
        &connection,
        &io,
        json!({"id": 2, "op": "cluster.machines"}),
    );
    assert!(is_ok(&reply), "{reply}");
    let machines = data(&reply)["machines"]
        .as_array()
        .expect("machines")
        .clone();
    assert_eq!(machines.len(), 3);
    assert_eq!(machines[0]["name"], "m1");
    assert_eq!(machines[0]["setAt"], 1_793_541_465_000_i64);
    assert_eq!(machines[0]["canResume"], true);
    assert!(machines[1].get("setAt").is_none());
    assert_eq!(machines[1]["canResume"], false);
    assert_eq!(machines[2]["canBeMaster"], false);
}

#[test]
fn old_phone_never_asks_machines_and_still_works() {
    let rig = Rig::new();
    let (connection, io, _) = rig.login("phone");
    for (id, op) in [(2, "sessions.list"), (3, "inbox.list"), (4, "held.list")] {
        let reply = ask(&rig, &connection, &io, json!({"id": id, "op": op}));
        assert!(
            is_ok(&reply) || error_code(&reply) == Some("invalid"),
            "{reply}"
        );
    }
    let sessions = ask(
        &rig,
        &connection,
        &io,
        json!({"id": 5, "op": "sessions.list"}),
    );
    assert!(is_ok(&sessions), "{sessions}");
    assert_eq!(rig.backend.with(|s| s.machine_calls), 0);
}

#[test]
fn a_mac_in_no_cluster_lists_no_machines() {
    let rig = Rig::new();
    let (connection, io, _) = rig.login("phone");
    let reply = ask(
        &rig,
        &connection,
        &io,
        json!({"id": 2, "op": "cluster.machines"}),
    );
    assert_eq!(data(&reply), json!({"machines": []}));
}

#[test]
fn promote_needs_acting() {
    let rig = Rig::new();
    rig.backend.with(|s| s.acting_off = true);
    let (connection, io, _) = rig.login("phone");
    let reply = ask(
        &rig,
        &connection,
        &io,
        json!({"id": 2, "op": "cluster.promote", "node": "1111111111111111"}),
    );
    assert_eq!(error_code(&reply), Some("invalid"), "{reply}");
    assert!(rig.backend.with(|s| s.promotes.is_empty()));
    assert_eq!(
        last_audit(&rig).as_deref(),
        Some("cluster.promote:refused:acting_off")
    );
}

#[test]
fn promote_is_audited_with_its_refusal_reason() {
    let rig = Rig::new();
    let (connection, io, _) = rig.login("phone");
    for (body, reason) in [
        (json!({"id": 2, "op": "cluster.promote"}), "invalid"),
        (
            json!({"id": 3, "op": "cluster.promote", "node": "n", "confirmed": "yes"}),
            "invalid",
        ),
    ] {
        let reply = ask(&rig, &connection, &io, body);
        assert_eq!(error_code(&reply), Some("invalid"), "{reply}");
        assert_eq!(
            last_audit(&rig),
            Some(format!("cluster.promote:refused:{reason}"))
        );
    }
    rig.backend.with(|s| {
        s.promote_answer = Some(Err(BridgeFailure::not_reachable("m1 isn't reachable.")))
    });
    let reply = ask(
        &rig,
        &connection,
        &io,
        json!({"id": 4, "op": "cluster.promote", "node": "1111111111111111"}),
    );
    assert_eq!(error_code(&reply), Some("not_reachable"), "{reply}");
    assert_eq!(
        last_audit(&rig).as_deref(),
        Some("cluster.promote:refused:not_reachable")
    );
    assert!(rig.audit.recent().iter().all(|e| e.session.is_none()));
}

#[test]
fn promote_answers_pass_through() {
    let rig = Rig::new();
    let (connection, io, _) = rig.login("phone");
    let answers = [
        json!({"step": "bump"}),
        json!({"relayed": true, "text": "m5 is reachable through m1; nothing moved"}),
        json!({"needsConfirm": "m5 can't be reached."}),
    ];
    for (n, answer) in answers.into_iter().enumerate() {
        rig.backend
            .with(|s| s.promote_answer = Some(Ok(answer.clone())));
        let reply = ask(
            &rig,
            &connection,
            &io,
            json!({"id": n + 2, "op": "cluster.promote", "node": "1111111111111111"}),
        );
        assert!(is_ok(&reply), "{reply}");
        assert_eq!(data(&reply), answer);
        assert_eq!(last_audit(&rig).as_deref(), Some("cluster.promote"));
    }
}

#[test]
fn promote_sends_confirmed_only_when_the_phone_did() {
    let rig = Rig::new();
    let (connection, io, _) = rig.login("phone");
    rig.backend
        .with(|s| s.promote_answer = Some(Ok(json!({"step": "bump"}))));
    let body = |id: u32, confirmed: Option<bool>| {
        let mut body = json!({"id": id, "op": "cluster.promote", "node": "1111111111111111"});
        if let Some(c) = confirmed {
            body["confirmed"] = json!(c);
        }
        body
    };
    ask(&rig, &connection, &io, body(2, None));
    ask(&rig, &connection, &io, body(3, Some(true)));
    assert_eq!(
        rig.backend.with(|s| s.promotes.clone()),
        [
            ("1111111111111111".to_string(), false),
            ("1111111111111111".to_string(), true)
        ]
    );
}

#[test]
fn promote_is_never_held_or_relayed() {
    for plan in [Plan::Relayed, Plan::Hold] {
        let relay = ScriptedRelay::new(None);
        *relay.plan.lock().expect("plan") = plan;
        let rig = Rig::new();
        let shared: Arc<dyn Relay> = relay.clone();
        rig.backend.with(|s| {
            s.relay = Some(shared);
            s.promote_answer = Some(Ok(json!({"step": "bump"})));
        });
        let (connection, io, _) = rig.login("phone");
        let reply = ask(
            &rig,
            &connection,
            &io,
            json!({"id": 2, "op": "cluster.promote", "node": "1111111111111111"}),
        );
        assert_eq!(data(&reply), json!({"step": "bump"}), "{reply}");
        assert_eq!(rig.backend.with(|s| s.promotes.len()), 1);
        assert!(relay.calls.lock().expect("calls").is_empty());
    }
}
