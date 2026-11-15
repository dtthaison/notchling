use serde_json::json;

use super::core_tests::Hub;
use super::core_tests_adopt::{follows_x_with_its_members, newer, relink, stranded, unmoved};
use super::LocalCmd;
use crate::cluster::id::NodeId;
use crate::cluster::wire::{Origin, OriginTier};

fn the_drivers_load_call(hub: &mut Hub, from: &NodeId, to: &NodeId) {
    hub.local(
        from,
        LocalCmd::Forward {
            node: to.clone(),
            op: "node.load".to_string(),
            args: json!({}),
            origin: Origin {
                node: from.clone(),
                tier: OriginTier::Node,
            },
        },
    );
}

#[test]
fn a_greeted_link_alone_never_heals_a_stranded_node() {
    let mut c = stranded();
    for _ in 0..12 {
        c.hub.tick_all(5_000);
    }
    assert!(unmoved(&c));
    assert!(c.hub.core(&c.n).online().contains(&c.h));
}

#[test]
fn a_stranded_node_heals_on_its_next_load_call_to_the_old_home() {
    let mut c = stranded();
    for _ in 0..6 {
        c.hub.tick_all(5_000);
    }
    assert!(unmoved(&c));
    let (n, h) = (c.n.clone(), c.h.clone());
    the_drivers_load_call(&mut c.hub, &n, &h);
    let state = c.hub.core(&n).state().expect("state");
    assert_eq!(state.home, c.x);
    assert_eq!(state.term, newer());
    relink(&mut c.hub, &c.n, &c.x);
    assert!(follows_x_with_its_members(&c));
}
