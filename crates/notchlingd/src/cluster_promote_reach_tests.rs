use crate::cluster_testkit::{pair, Opts};
use notchling_cluster_net::MemNet;
use notchling_engine::cluster::wire::{Origin, OriginTier};
use notchling_engine::cluster::{LocalCmd, NodeId};
use serde_json::{json, Value};
use std::time::Duration;

fn ask(from: &crate::cluster_testkit::TestNode, of: &NodeId, about: &NodeId) -> Value {
    let origin = Origin {
        node: from.id(),
        tier: OriginTier::Node,
    };
    from.node
        .local_wire(
            LocalCmd::Forward {
                node: of.clone(),
                op: "node.reach".to_string(),
                args: json!({"node": about}).into(),
                origin,
            },
            Duration::from_secs(5),
        )
        .expect("the answer")
}

#[test]
fn node_reach_answers_from_the_peers_view() {
    let net = MemNet::new();
    let (a, b) = pair(&net, Opts::named("home"), Opts::named("worker"));
    let stranger =
        NodeId::from_fingerprint(&notchling_cluster_net::fingerprint(b"stranger")).expect("an id");
    assert_eq!(ask(&a, &b.id(), &a.id()), json!({"online": true}));
    assert_eq!(ask(&a, &b.id(), &b.id()), json!({"online": true}));
    assert_eq!(ask(&a, &b.id(), &stranger), json!({"online": false}));
    assert_eq!(ask(&b, &a.id(), &b.id()), json!({"online": true}));
}

#[test]
fn node_reach_says_no_once_the_peer_is_gone() {
    let net = MemNet::new();
    let (a, b) = pair(&net, Opts::named("home"), Opts::named("worker"));
    let c_fp = notchling_cluster_net::fingerprint(b"third");
    let third = NodeId::from_fingerprint(&c_fp).expect("an id");
    assert_eq!(ask(&a, &b.id(), &third), json!({"online": false}));
    b.node.set_link(false);
    crate::cluster_testkit::eventually("a sees b offline", || {
        !crate::cluster_testkit::online(&a.node.status(), &b.id())
    });
    let origin = Origin {
        node: a.id(),
        tier: OriginTier::Node,
    };
    let lost = a.node.local_wire(
        LocalCmd::Forward {
            node: b.id(),
            op: "node.reach".to_string(),
            args: json!({"node": a.id()}).into(),
            origin,
        },
        Duration::from_secs(5),
    );
    assert!(lost.is_err(), "{lost:?}");
}
