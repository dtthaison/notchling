use crate::cluster_relay_testkit::*;
use notchling_engine::bridge::relay::{Cancelled, Relay, Routed};
use notchling_engine::bridge::BridgeFailure;
use notchling_engine::cluster::wire::WireError;
use serde_json::json;

#[test]
fn hello_lists_can_be_master_machines() {
    let rig = rig(M1, M5, true);
    let cluster = rig.relay.cluster().expect("a cluster");
    assert_eq!(cluster["primary"], M5);
    assert_eq!(cluster["term"], 3);
    let nodes = cluster["nodes"].as_array().unwrap();
    let names: Vec<&str> = nodes.iter().map(|n| n["name"].as_str().unwrap()).collect();
    assert_eq!(names, ["m5", "m1", "m2"], "workers are left out");
    let me = nodes.iter().find(|n| n["id"] == M1).unwrap();
    assert_eq!(me["fingerprint"], "5".repeat(64), "the served certificate");
    assert_eq!(me["addrs"], json!(["10.0.0.2:47821"]));
    assert_eq!(me["canBeMaster"], true);
    let other = nodes.iter().find(|n| n["id"] == M5).unwrap();
    assert_eq!(other["fingerprint"], format!("b{M5}b{M5}b{M5}b{M5}"));
    assert_eq!(other["addrs"], json!(["10.0.0.2:47900"]), "its own port");
    go(&rig, M1, M5, false);
    let cluster = rig.relay.cluster().unwrap();
    assert_eq!(cluster["nodes"][0]["online"], false);
}

#[test]
fn a_node_that_has_not_published_its_bridge_is_listed_without_a_fingerprint() {
    let rig = rig(M1, M5, true);
    let mut status = status(M1, M5, true);
    status["members"][0]["versions"] = json!({});
    status["members"][2]["versions"] = json!({"bridge": {"port": 47950}});
    *rig.net.status.lock().unwrap() = status;
    let cluster = rig.relay.cluster().unwrap();
    let nodes = cluster["nodes"].as_array().unwrap();
    assert_eq!(nodes[0]["fingerprint"], "", "no facts yet");
    assert_eq!(nodes[0]["addrs"], json!([]), "and no port to dial");
    assert_eq!(nodes[2]["fingerprint"], "", "a port but no certificate yet");
    assert_eq!(nodes[2]["addrs"], json!(["10.0.0.2:47950"]));
}

#[test]
fn addresses_with_ports_or_names_or_ipv6_are_made_into_host_and_bridge_port() {
    let rig = rig(M1, M5, true);
    let mut status = status(M1, M5, true);
    status["members"][0]["addrs"] = json!([
        "10.0.0.2",
        "fe80::1",
        "m5.local",
        "10.0.0.9:47820",
        "[fe80::2]:47820",
        "m5.local:47820"
    ]);
    *rig.net.status.lock().unwrap() = status;
    let cluster = rig.relay.cluster().unwrap();
    assert_eq!(
        cluster["nodes"][0]["addrs"],
        json!([
            "10.0.0.2:47900",
            "[fe80::1]:47900",
            "m5.local:47900",
            "10.0.0.9:47900",
            "[fe80::2]:47900",
            "m5.local:47900"
        ])
    );
}

#[test]
fn a_machine_in_no_cluster_says_nothing() {
    let rig = rig(M1, M5, true);
    *rig.net.status.lock().unwrap() = json!({"cluster": null, "members": [], "me": M1});
    assert_eq!(rig.relay.cluster(), None);
    assert_eq!(
        message(&rig, "1", &format!("{M5}/job-7"), "hi"),
        Routed::Local
    );
}

#[test]
fn standby_relays_to_reachable_primary() {
    let rig = rig(M1, M5, true);
    let routed = message(&rig, "1", &format!("{M5}/job-7"), "go on");
    assert_eq!(routed, Routed::Done(json!({"sent": true, "relayed": true})));
    assert_eq!(
        sent(&rig),
        [(
            M5.to_string(),
            "session.reply".to_string(),
            json!({"id": "job-7", "text": "go on"})
        )]
    );
}

#[test]
fn worker_session_goes_straight_to_worker() {
    let rig = rig(M1, M5, false);
    assert_eq!(
        message(&rig, "1", &format!("{W}/job-3"), "hi"),
        Routed::Local
    );
    assert!(sent(&rig).is_empty());
    assert!(rig.relay.list(DEV).entries.is_empty());
}

#[test]
fn the_primary_and_a_worker_never_hold() {
    let rig = rig(M5, M5, true);
    assert_eq!(message(&rig, "1", "job-7", "hi"), Routed::Local);
    let rig = self::rig(W, M5, false);
    assert_eq!(
        message(&rig, "1", &format!("{M5}/job-7"), "hi"),
        Routed::Local
    );
}

#[test]
fn unreachable_primary_holds_in_order() {
    let rig = rig(M1, M5, false);
    let a = held_id(message(&rig, "1", &format!("{M5}/job-7"), "one"));
    let b = held_id(message(&rig, "2", &format!("{M5}/job-7"), "two"));
    assert_ne!(a, b);
    let list = rig.relay.list(DEV);
    assert_eq!(summaries(&rig), ["one", "two"]);
    assert_eq!(list.entries[0].op, "session.message");
    assert_eq!(
        list.entries[0].session_id.as_deref(),
        Some(&*format!("{M5}/job-7"))
    );
    assert!(!list.entries[0].waiting);
    assert!(sent(&rig).is_empty());
    let again = held_id(message(&rig, "1", &format!("{M5}/job-7"), "one"));
    assert_eq!(again, a, "the same request is the same entry");
    assert_eq!(rig.relay.list(DEV).entries.len(), 2);
}

#[test]
fn a_refused_send_while_reachable_holds_when_the_primary_drops() {
    let rig = rig(M1, M5, true);
    *rig.net.reply.lock().unwrap() = Err(unreachable_error());
    assert!(matches!(
        message(&rig, "1", &format!("{M5}/job-7"), "one"),
        Routed::Held(_)
    ));
    assert_eq!(rig.relay.list(DEV).entries.len(), 1);
}

#[test]
fn a_refusal_is_an_error_and_a_timeout_is_not_resent() {
    let rig = rig(M1, M5, true);
    *rig.net.reply.lock().unwrap() = Err(WireError::new("bad_args", "no such session"));
    let Routed::Fail(failure) = message(&rig, "1", &format!("{M5}/job-7"), "x") else {
        panic!("a failure");
    };
    assert!(failure.message.contains("no such session"));
    *rig.net.reply.lock().unwrap() = Err(WireError::new("timeout", "m5 didn't answer"));
    let routed = message(&rig, "2", &format!("{M5}/job-7"), "y");
    assert_eq!(
        routed,
        Routed::Fail(BridgeFailure::primary_unreachable(
            "m5",
            "m5 didn't answer, so that may or may not have arrived."
        ))
    );
    assert!(rig.relay.list(DEV).entries.is_empty(), "never held twice");
}

#[test]
fn held_command_is_delivered_once_on_return() {
    let rig = rig(M1, M5, false);
    let one = held_id(message(&rig, "1", &format!("{M5}/job-7"), "one"));
    message(&rig, "2", &format!("{M5}/job-7"), "two");
    rig.relay.flush();
    assert!(sent(&rig).is_empty(), "still offline");
    go(&rig, M1, M5, true);
    rig.relay.flush();
    rig.relay.flush();
    assert_eq!(texts(&rig), [json!("one"), json!("two")], "once, in order");
    assert!(rig.relay.list(DEV).entries.is_empty());
    assert_eq!(rig.relay.cancel(DEV, &one), Cancelled::Delivered);
    assert_eq!(rig.relay.cancel(DEV, "nope"), Cancelled::Unknown);
}

#[test]
fn a_held_command_that_finds_the_primary_gone_again_stays_held() {
    let rig = rig(M1, M5, false);
    message(&rig, "1", &format!("{M5}/job-7"), "one");
    message(&rig, "2", &format!("{M5}/job-7"), "two");
    go(&rig, M1, M5, true);
    *rig.net.reply.lock().unwrap() = Err(unreachable_error());
    rig.relay.flush();
    assert_eq!(sent(&rig).len(), 1, "stops at the first that fails");
    assert_eq!(summaries(&rig), ["one", "two"]);
    *rig.net.reply.lock().unwrap() = Ok(json!({}));
    rig.relay.flush();
    assert!(rig.relay.list(DEV).entries.is_empty());
    assert_eq!(sent(&rig).len(), 3);
}

#[test]
fn held_goes_before_new_ops() {
    let rig = rig(M1, M5, false);
    message(&rig, "1", &format!("{M5}/job-7"), "held one");
    go(&rig, M1, M5, true);
    let routed = message(&rig, "2", &format!("{M5}/job-7"), "new one");
    assert!(
        matches!(routed, Routed::Held(_)),
        "behind the held one, never ahead of it"
    );
    assert!(sent(&rig).is_empty());
    rig.relay.flush();
    assert_eq!(texts(&rig), [json!("held one"), json!("new one")]);
}

#[test]
fn a_new_op_waits_behind_a_held_one_that_cannot_go() {
    let rig = rig(M1, M5, false);
    message(&rig, "1", &format!("{M5}/job-7"), "held one");
    go(&rig, M1, M5, true);
    *rig.net.reply.lock().unwrap() = Err(unreachable_error());
    let routed = message(&rig, "2", &format!("{M5}/job-7"), "new one");
    assert!(matches!(routed, Routed::Held(_)));
    assert_eq!(rig.relay.list(DEV).entries.len(), 2);
}

#[test]
fn held_runs_here_after_promote() {
    let rig = rig(M1, M5, false);
    message(&rig, "1", &format!("{M5}/job-7"), "keep going");
    go(&rig, M1, M1, true);
    *rig.rows.lock().unwrap() = vec![format!("{M1}/job-7")];
    rig.relay.flush();
    assert_eq!(
        sent(&rig),
        [(
            M1.to_string(),
            "session.reply".to_string(),
            json!({"id": "job-7", "text": "keep going"})
        )],
        "it runs on this machine now"
    );
    assert!(rig.relay.list(DEV).entries.is_empty());
}

#[test]
fn a_command_for_a_session_that_didnt_move_stays_held_after_a_flush() {
    let rig = rig(M1, M5, false);
    message(&rig, "1", &format!("{M5}/job-7"), "keep going");
    go(&rig, M1, M2, true);
    *rig.rows.lock().unwrap() = vec![format!("{M5}/job-7")];
    rig.relay.flush();
    assert!(sent(&rig).is_empty());
    assert_eq!(rig.relay.list(DEV).entries.len(), 1);
    *rig.rows.lock().unwrap() = vec![format!("{M2}/job-7")];
    rig.relay.flush();
    assert_eq!(sent(&rig)[0].0, M2);
    assert!(rig.relay.list(DEV).entries.is_empty());
}

#[test]
fn a_session_that_has_not_moved_does_not_block_other_commands() {
    let rig = rig(M1, M5, false);
    message(&rig, "1", &format!("{M5}/job-7"), "keep going");
    go(&rig, M1, M2, true);
    *rig.rows.lock().unwrap() = vec![format!("{M5}/job-7"), format!("{M2}/job-9")];
    rig.relay.flush();
    assert_eq!(rig.relay.list(DEV).entries.len(), 1, "kept back");
    let routed = message(&rig, "2", &format!("{M2}/job-9"), "other session");
    assert_eq!(
        routed,
        Routed::Done(json!({"sent": true, "relayed": true})),
        "sent at once, not held behind it"
    );
    assert_eq!(texts(&rig), [json!("other session")]);
}

#[test]
fn an_unknown_placement_stays_held() {
    let rig = rig(M1, M5, false);
    message(&rig, "1", &format!("{M5}/job-7"), "keep going");
    go(&rig, M1, M5, true);
    rig.rows.lock().unwrap().clear();
    rig.relay.flush();
    assert!(sent(&rig).is_empty(), "nothing is sent to a guess");
    assert_eq!(rig.relay.list(DEV).entries.len(), 1);
    assert!(
        rig.relay.list(DEV).notices.is_empty(),
        "and nothing is dropped"
    );
    *rig.rows.lock().unwrap() = vec![format!("{M5}/job-7")];
    rig.relay.flush();
    assert_eq!(sent(&rig).len(), 1);
    assert!(rig.relay.list(DEV).entries.is_empty());
}
