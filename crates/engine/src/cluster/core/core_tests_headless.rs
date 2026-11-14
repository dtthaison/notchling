use serde_json::json;

use super::core_tests::{pair, Hub};
use super::{Effect, LocalCmd};
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
