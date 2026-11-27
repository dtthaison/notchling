use serde_json::json;

use super::core_tests::{pair, Hub};
use super::{Effect, Input, LocalCmd};
use crate::cluster::members::MemberState;
use crate::cluster::testkit::{id, member};
use crate::cluster::wire::{decode, Msg};

fn headless_pair() -> Hub {
    let mut hub = Hub::new();
    hub.add("m5", "macos", true);
    hub.add("w1", "linux", false);
    hub.create("m5");
    let mut me = member("w1", "linux", false);
    me.headless = true;
    hub.join_me("m5", me, false, false);
    hub
}

#[test]
fn the_home_records_a_headless_join() {
    let hub = headless_pair();
    let (m5, w1) = (id("m5"), id("w1"));
    let home = hub.core(&m5).state().expect("home state");
    assert!(home.members[&w1].headless);
    assert!(!home.members[&w1].client);
    let worker = hub.core(&w1).state().expect("worker state");
    assert!(worker.members[&w1].headless);
    assert!(hub
        .effects_of("m5")
        .iter()
        .any(|e| matches!(e, Effect::JoinRequest { headless: true, .. })));
}

#[test]
fn a_headless_member_rejoins_its_home_on_its_own() {
    let hub = headless_pair();
    let (m5, w1) = (id("m5"), id("w1"));
    let worker = hub.core(&w1).state().expect("worker state");
    assert_eq!(worker.rejoin_home(&w1), Some(m5.clone()));
    let home = hub.core(&m5).state().expect("home state");
    assert_eq!(home.rejoin_home(&m5), None);
}

#[test]
fn a_member_joined_from_the_app_never_rejoins_on_its_own() {
    let (hub, _, w1) = pair();
    let worker = hub.core(&w1).state().expect("worker state");
    assert!(!worker.members[&w1].headless);
    assert_eq!(worker.rejoin_home(&w1), None);
}

#[test]
fn a_client_pending_removed_or_unknown_member_never_rejoins() {
    let hub = headless_pair();
    let w1 = id("w1");
    let base = hub.core(&w1).state().expect("worker state").clone();
    let mut client = base.clone();
    client.members.get_mut(&w1).expect("me").client = true;
    assert_eq!(client.rejoin_home(&w1), None);
    for state in [MemberState::Pending, MemberState::Removed] {
        let mut other = base.clone();
        other.members.get_mut(&w1).expect("me").state = state;
        assert_eq!(other.rejoin_home(&w1), None, "{state:?}");
    }
    let mut gone = base.clone();
    gone.members.remove(&w1);
    assert_eq!(gone.rejoin_home(&w1), None);
    let mut homeless = base;
    let home = homeless.home.clone();
    homeless.members.remove(&home);
    assert_eq!(homeless.rejoin_home(&w1), None);
}

#[test]
fn the_home_can_clear_the_headless_mark() {
    let mut hub = headless_pair();
    let (m5, w1) = (id("m5"), id("w1"));
    hub.local(
        &m5,
        LocalCmd::SetMember {
            node: w1.clone(),
            can_be_master: None,
            client: None,
            reference: None,
            accepts_delegation: None,
            headless: Some(false),
            desired: None,
        },
    );
    hub.tick_all(0);
    let worker = hub.core(&w1).state().expect("worker state");
    assert!(!worker.members[&w1].headless);
    assert_eq!(worker.rejoin_home(&w1), None);
}

#[test]
fn an_older_join_message_reads_as_not_headless() {
    let line = json!({
        "type": "join",
        "secret": "s",
        "name": "w1",
        "fingerprint": "f",
        "addrs": [],
        "port": 1,
        "versions": crate::cluster::testkit::versions("linux"),
    });
    let msg = decode(line.to_string().as_bytes()).expect("a join");
    assert!(matches!(
        msg,
        Msg::Join {
            headless: false,
            ..
        }
    ));
}

#[test]
fn an_event_from_a_peer_names_its_sender_whatever_it_claims() {
    let (mut hub, m5, w1) = pair();
    let effects = hub.run(
        &m5,
        Input::Msg {
            from: w1.clone(),
            msg: Msg::Event {
                topic: "sessions".to_string(),
                name: "sessions.changed".to_string(),
                data: json!({"from": "someone else"}),
            },
        },
    );
    let data = effects
        .iter()
        .find_map(|e| match e {
            Effect::Publish { name, data, .. } if name == "sessions.changed" => Some(data.clone()),
            _ => None,
        })
        .expect("published");
    assert_eq!(data["from"], json!(w1));
}

#[test]
fn an_outbox_event_from_a_peer_keeps_only_ids_and_names_its_sender() {
    let (mut hub, m5, w1) = pair();
    let effects = hub.run(
        &m5,
        Input::Msg {
            from: w1.clone(),
            msg: Msg::Event {
                topic: "outbox".to_string(),
                name: "outbox.removed".to_string(),
                data: json!({
                    "id": "o1", "session": "ab12", "why": "sent", "text": "secret",
                    "row": {"text": "secret"}, "from": "someone else", "n": 3,
                }),
            },
        },
    );
    let data = effects
        .iter()
        .find_map(|e| match e {
            Effect::Publish { topic, data, .. } if topic == "outbox" => Some(data.clone()),
            _ => None,
        })
        .expect("published");
    assert_eq!(
        data,
        json!({"id": "o1", "session": "ab12", "why": "sent", "from": w1})
    );
}

#[test]
fn a_peer_event_on_a_topic_this_node_does_not_consume_is_dropped() {
    let (mut hub, m5, w1) = pair();
    for topic in ["cluster", "prompts", "settings"] {
        let effects = hub.run(
            &m5,
            Input::Msg {
                from: w1.clone(),
                msg: Msg::Event {
                    topic: topic.to_string(),
                    name: "cluster.refused".to_string(),
                    data: json!({"from": "forged"}),
                },
            },
        );
        assert!(
            !effects
                .iter()
                .any(|e| matches!(e, Effect::Publish { name, .. } if name == "cluster.refused")),
            "{topic}"
        );
    }
}

fn prompt_event(name: &str) -> Msg {
    Msg::Event {
        topic: "prompts".to_string(),
        name: name.to_string(),
        data: json!({"id": "p1", "from": "someone else"}),
    }
}

fn published(effects: &[Effect], name: &str) -> Option<serde_json::Value> {
    effects.iter().find_map(|e| match e {
        Effect::Publish { name: n, data, .. } if n == name => Some(data.clone()),
        _ => None,
    })
}

#[test]
fn a_prompt_event_is_published_only_on_a_client_with_its_sender_named() {
    let (mut hub, m5, w1) = pair();
    for name in ["prompt.pending", "prompt.resolved"] {
        let effects = hub.run(
            &m5,
            Input::Msg {
                from: w1.clone(),
                msg: prompt_event(name),
            },
        );
        let data = published(&effects, name).expect("a client publishes it");
        assert_eq!(data["from"], json!(w1));
        assert_eq!(data["id"], json!("p1"));
    }
    let effects = hub.run(
        &w1,
        Input::Msg {
            from: m5.clone(),
            msg: prompt_event("prompt.pending"),
        },
    );
    assert!(published(&effects, "prompt.pending").is_none());
}

#[test]
fn a_prompt_event_with_another_name_or_from_a_stranger_is_dropped() {
    let (mut hub, m5, w1) = pair();
    let effects = hub.run(
        &m5,
        Input::Msg {
            from: w1.clone(),
            msg: prompt_event("prompt.other"),
        },
    );
    assert!(published(&effects, "prompt.other").is_none());
    let effects = hub.run(
        &m5,
        Input::Msg {
            from: id("zz"),
            msg: prompt_event("prompt.pending"),
        },
    );
    assert!(published(&effects, "prompt.pending").is_none());
}

#[test]
fn a_prompt_event_from_a_greeted_member_that_is_not_active_is_dropped() {
    for state in [MemberState::Pending, MemberState::Removed] {
        let (mut hub, m5, w1) = pair();
        let effects = hub.run(
            &m5,
            Input::Msg {
                from: w1.clone(),
                msg: prompt_event("prompt.pending"),
            },
        );
        assert!(
            published(&effects, "prompt.pending").is_some(),
            "{state:?}: active first"
        );
        let core = hub.nodes.get_mut(&m5).expect("home");
        core.state
            .as_mut()
            .expect("state")
            .members
            .get_mut(&w1)
            .expect("member")
            .state = state;
        assert!(core.peers.get(&w1).is_some_and(|p| p.greeted));
        let effects = hub.run(
            &m5,
            Input::Msg {
                from: w1.clone(),
                msg: prompt_event("prompt.pending"),
            },
        );
        assert!(published(&effects, "prompt.pending").is_none(), "{state:?}");
    }
}
