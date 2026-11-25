use crate::embed_fetch::ModelNet;
use crate::embed_ops_tests::{rig_net, Rig};
use crate::limits_ops_tests::TempDir;
use crate::op_group::OpGroup;
use notchling_local_api::{bind_identity, ops, AgentIdentity, Caller, ClientKind, Tier};
use serde_json::{json, Value};

const COUNTS: [&str; 3] = ["corpora", "backlog", "stored"];

fn caller(tier: Tier) -> Caller {
    Caller {
        tier,
        client: ClientKind::Cli,
        peer: None,
        pid: Some(1),
    }
}

fn who(verified: bool, session: &str, org: Option<&str>) -> AgentIdentity {
    let mut who = AgentIdentity::shell();
    who.verified = verified;
    who.session = verified.then(|| session.to_string());
    who.organization = org.map(str::to_string);
    who
}

fn indexed(docs: usize) -> (Rig, TempDir) {
    let (rig, dir) = rig_net(docs, true, true, ModelNet::offline());
    rig.ops.tick();
    (rig, dir)
}

fn status(rig: &Rig, tier: Tier, identity: Option<AgentIdentity>) -> Value {
    let _bound = bind_identity(identity);
    rig.ops
        .call(&caller(tier), ops::EMBED_STATUS, &json!({}))
        .unwrap()
}

#[test]
fn a_shell_or_unverified_agent_is_refused_like_the_other_search_ops() {
    let (rig, _dir) = indexed(4);
    for identity in [
        None,
        Some(who(false, "s1", None)),
        Some(who(false, "s1", Some("work"))),
        Some(who(true, "s1", None)),
    ] {
        let reply = status(&rig, Tier::Agent, identity);
        assert_eq!(reply, json!({"reason": "no_session"}));
    }
}

#[test]
fn a_verified_agent_gets_the_state_and_models_but_no_counts() {
    let (rig, _dir) = indexed(4);
    let reply = status(&rig, Tier::Agent, Some(who(true, "s1", Some("work"))));
    assert!(reply.get("reason").is_none(), "{reply}");
    assert_eq!(reply["organization"], json!("work"));
    assert!(reply["state"].is_string(), "{reply}");
    assert!(reply["models"].as_array().is_some_and(|m| !m.is_empty()));
    assert!(reply["fetch"].is_array());
    assert_eq!(reply["paused"], json!(false));
    assert!(reply["budget"]["paused"].is_string());
    assert!(reply.get("error").is_some());
    for key in COUNTS {
        assert!(reply.get(key).is_none(), "{key} reached an agent: {reply}");
    }
}

#[test]
fn an_agent_in_another_organization_sees_no_counts_either() {
    let (rig, _dir) = indexed(4);
    let work = status(&rig, Tier::Agent, Some(who(true, "s1", Some("work"))));
    let other = status(&rig, Tier::Agent, Some(who(true, "s2", Some("personal"))));
    assert_eq!(other["organization"], json!("personal"));
    for key in COUNTS {
        assert!(other.get(key).is_none(), "{key} reached an agent: {other}");
    }
    let (mut work, mut other) = (work, other);
    work.as_object_mut().unwrap().remove("organization");
    other.as_object_mut().unwrap().remove("organization");
    assert_eq!(work, other);
}

#[test]
fn the_app_sees_every_count_whoever_the_thread_is_bound_to() {
    let (rig, _dir) = indexed(4);
    for identity in [None, Some(who(true, "s1", Some("work")))] {
        let reply = status(&rig, Tier::App, identity);
        assert!(reply.get("reason").is_none(), "{reply}");
        assert!(reply.get("organization").is_none(), "{reply}");
        assert_eq!(reply["corpora"][0]["corpus"], json!("sessions"));
        assert_eq!(reply["corpora"][0]["docs"], json!(4));
        assert_eq!(reply["corpora"][0]["chunks"], json!(4));
        for key in ["backlog", "stored", "models", "fetch", "budget", "state"] {
            assert!(reply.get(key).is_some(), "{key} missing: {reply}");
        }
    }
}
