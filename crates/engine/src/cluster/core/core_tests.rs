use std::collections::{BTreeMap, VecDeque};

use serde_json::{json, Value};

use super::{Effect, Input, LocalCmd, NodeCore, OFFLINE_MS, PING_MS};
use crate::cluster::id::{NodeId, Tool};
use crate::cluster::jobs::JobSpec;
use crate::cluster::join::JoinToken;
use crate::cluster::members::MemberState;
use crate::cluster::state::Command;
use crate::cluster::testkit::{fp, id, member, versions};
use crate::cluster::wire::{Msg, OriginTier, WireError};

pub(super) struct Hub {
    pub(super) nodes: BTreeMap<NodeId, NodeCore>,
    pub(super) names: BTreeMap<NodeId, String>,
    pub(super) linked: Vec<(NodeId, NodeId)>,
    pub(super) seen: Vec<(String, Effect)>,
    pub(super) now: u64,
}

impl Hub {
    pub(super) fn new() -> Hub {
        Hub {
            nodes: BTreeMap::new(),
            names: BTreeMap::new(),
            linked: Vec::new(),
            seen: Vec::new(),
            now: 0,
        }
    }

    pub(super) fn add(&mut self, name: &str, os: &str, client: bool) -> NodeId {
        let node = id(name);
        let core = NodeCore::new(node.clone(), versions(os), None, client)
            .with_boot_id(format!("boot-{name}"));
        self.nodes.insert(node.clone(), core);
        self.names.insert(node.clone(), name.to_string());
        node
    }

    pub(super) fn run(&mut self, who: &NodeId, input: Input) -> Vec<Effect> {
        let mut queue: VecDeque<(NodeId, Input)> = VecDeque::new();
        queue.push_back((who.clone(), input));
        let mut first: Option<Vec<Effect>> = None;
        while let Some((node, input)) = queue.pop_front() {
            let effects = self.nodes.get_mut(&node).expect("node").step(input);
            if first.is_none() {
                first = Some(effects.clone());
            }
            for effect in effects {
                match &effect {
                    Effect::Send { to, msg } => {
                        if self.is_linked(&node, to) {
                            queue.push_back((
                                to.clone(),
                                Input::Msg {
                                    from: node.clone(),
                                    msg: msg.clone(),
                                },
                            ));
                        }
                    }
                    Effect::Close { peer } => {
                        self.linked.retain(|(a, b)| {
                            !((a == &node && b == peer) || (a == peer && b == &node))
                        });
                        queue.push_back((peer.clone(), Input::Unlinked { peer: node.clone() }));
                    }
                    _ => {}
                }
                self.seen.push((self.names[&node].clone(), effect));
            }
        }
        first.unwrap_or_default()
    }

    pub(super) fn is_linked(&self, a: &NodeId, b: &NodeId) -> bool {
        self.linked
            .iter()
            .any(|(x, y)| (x == a && y == b) || (x == b && y == a))
    }

    pub(super) fn link(&mut self, a: &NodeId, b: &NodeId) {
        self.linked.push((a.clone(), b.clone()));
        self.run(a, Input::Linked { peer: b.clone() });
        self.run(b, Input::Linked { peer: a.clone() });
    }

    pub(super) fn tick(&mut self, who: &NodeId, ms: u64) -> Vec<Effect> {
        self.now += ms;
        let now = self.now;
        self.run(
            who,
            Input::Tick {
                boot_ms: now,
                wall_ms: now as i64,
            },
        )
    }

    pub(super) fn tick_all(&mut self, ms: u64) {
        self.now += ms;
        let now = self.now;
        let ids: Vec<NodeId> = self.nodes.keys().cloned().collect();
        for n in ids {
            self.run(
                &n,
                Input::Tick {
                    boot_ms: now,
                    wall_ms: now as i64,
                },
            );
        }
    }

    pub(super) fn local(&mut self, who: &NodeId, cmd: LocalCmd) -> Vec<Effect> {
        self.run(who, Input::Local { id: 1, cmd })
    }

    pub(super) fn core(&self, who: &NodeId) -> &NodeCore {
        &self.nodes[who]
    }

    pub(super) fn effects_of(&self, name: &str) -> Vec<&Effect> {
        self.seen
            .iter()
            .filter(|(n, _)| n == name)
            .map(|(_, e)| e)
            .collect()
    }

    pub(super) fn create(&mut self, home: &str) {
        let node = id(home);
        let mut me = member(home, "macos", true);
        me.reference = true;
        self.local(
            &node,
            LocalCmd::Create {
                id: "c1".to_string(),
                name: "cluster".to_string(),
                me,
            },
        );
    }

    pub(super) fn join(&mut self, home: &str, who: &str, os: &str, can_be_master: bool) {
        self.join_as(home, who, os, false, can_be_master);
    }

    pub(super) fn join_as(
        &mut self,
        home: &str,
        who: &str,
        os: &str,
        client: bool,
        can_be_master: bool,
    ) {
        self.join_me(home, member(who, os, false), client, can_be_master);
    }

    pub(super) fn join_me(
        &mut self,
        home: &str,
        me: crate::cluster::members::Member,
        client: bool,
        can_be_master: bool,
    ) {
        let (h, w) = (id(home), me.id.clone());
        let secret = format!("secret-{}", me.name);
        self.local(
            &h,
            LocalCmd::IssueToken {
                secret: secret.clone(),
            },
        );
        let token = JoinToken {
            cluster: "c1".to_string(),
            home: h.clone(),
            fingerprint: fp(home),
            addrs: vec!["100.64.0.5".to_string()],
            port: 47823,
            secret,
        };
        self.local(&w, LocalCmd::JoinWith { token, me });
        self.linked.push((h.clone(), w.clone()));
        self.run(&h, Input::Linked { peer: w.clone() });
        self.run(&w, Input::Linked { peer: h.clone() });
        self.local(
            &h,
            LocalCmd::Approve {
                node: w.clone(),
                client,
                can_be_master,
            },
        );
        self.tick_all(0);
    }
}

pub(super) fn dispatch_cmd(key: &str, node: &str, tool: Tool) -> LocalCmd {
    LocalCmd::Dispatch {
        key: key.to_string(),
        spec: JobSpec {
            node: id(node),
            tool,
            cwd: "/srv/repo".to_string(),
            args: json!({"prompt": "go"}),
            secrets: Vec::new(),
        },
        tier: OriginTier::App,
    }
}

pub(super) fn pair() -> (Hub, NodeId, NodeId) {
    let mut hub = Hub::new();
    let m5 = hub.add("m5", "macos", true);
    let w1 = hub.add("w1", "linux", false);
    hub.create("m5");
    hub.join("m5", "w1", "linux", false);
    (hub, m5, w1)
}

pub(super) fn answers(effects: &[Effect]) -> Vec<&Result<Value, WireError>> {
    effects
        .iter()
        .filter_map(|e| match e {
            Effect::Answer { result, .. } => Some(result),
            _ => None,
        })
        .collect()
}

pub(super) fn run_locals(effects: &[Effect]) -> Vec<(u64, String, Value)> {
    effects
        .iter()
        .filter_map(|e| match e {
            Effect::RunLocal { call, op, args, .. } => Some((*call, op.clone(), args.clone())),
            _ => None,
        })
        .collect()
}

#[test]
fn join_with_a_token_makes_the_worker_a_member() {
    let (hub, m5, w1) = pair();
    let home = hub.core(&m5).state().expect("home state");
    assert_eq!(home.members[&w1].state, MemberState::Active);
    let worker = hub.core(&w1).state().expect("worker state");
    assert_eq!(worker.home, m5);
    assert!(hub.core(&m5).online().contains(&w1));
    assert!(hub.core(&w1).online().contains(&m5));
    assert!(hub
        .effects_of("m5")
        .iter()
        .any(|e| matches!(e, Effect::JoinRequest { node, .. } if *node == w1)));
}

#[test]
fn a_join_token_is_one_time() {
    let (mut hub, m5, _) = pair();
    let w2 = hub.add("w2", "linux", false);
    hub.local(
        &m5,
        LocalCmd::IssueToken {
            secret: "once".to_string(),
        },
    );
    let token = JoinToken {
        cluster: "c1".to_string(),
        home: m5.clone(),
        fingerprint: fp("m5"),
        addrs: vec![],
        port: 1,
        secret: "wrong".to_string(),
    };
    hub.local(
        &w2,
        LocalCmd::JoinWith {
            token,
            me: member("w2", "linux", false),
        },
    );
    hub.linked.push((m5.clone(), w2.clone()));
    hub.run(&m5, Input::Linked { peer: w2.clone() });
    hub.run(&w2, Input::Linked { peer: m5.clone() });
    assert!(hub.core(&w2).state().is_none());
    assert!(!hub
        .core(&m5)
        .state()
        .expect("state")
        .members
        .contains_key(&w2));
    assert!(hub.seen.iter().any(|(_, e)| matches!(e, Effect::Publish { name, data, .. } if name == "cluster.refused" && data["reason"] == "bad token")));
    assert!(
        hub.seen.iter().any(|(n, e)| n == "w2"
            && matches!(e, Effect::Answer { result: Err(err), .. } if err.code == "refused" && err.message == "bad token")),
        "a refused joiner's caller hears it at once"
    );
}

#[test]
fn a_join_nobody_answers_ends_with_an_error() {
    let (mut hub, m5, _) = pair();
    let w3 = hub.add("w3", "linux", false);
    let token = JoinToken {
        cluster: "c1".to_string(),
        home: m5.clone(),
        fingerprint: fp("m5"),
        addrs: vec![],
        port: 1,
        secret: "never".to_string(),
    };
    hub.local(
        &w3,
        LocalCmd::JoinWith {
            token,
            me: member("w3", "linux", false),
        },
    );
    let early = hub.tick(&w3, super::JOIN_ANSWER_MS - 1);
    assert!(!early.iter().any(|e| matches!(e, Effect::Answer { .. })));
    let late = hub.tick(&w3, 1);
    assert!(late.iter().any(
        |e| matches!(e, Effect::Answer { result: Err(err), .. } if err.code == "unreachable")
    ));
    let again = hub.local(
        &w3,
        LocalCmd::JoinWith {
            token: JoinToken {
                cluster: "c1".to_string(),
                home: m5.clone(),
                fingerprint: fp("m5"),
                addrs: vec![],
                port: 1,
                secret: "next".to_string(),
            },
            me: member("w3", "linux", false),
        },
    );
    assert!(!again
        .iter()
        .any(|e| matches!(e, Effect::Answer { result: Err(err), .. } if err.code == "exists")));
}

#[test]
fn hello_with_other_proto_is_refused() {
    let mut hub = Hub::new();
    let m5 = hub.add("m5", "macos", true);
    let w1 = hub.add("w1", "linux", false);
    hub.create("m5");
    hub.join("m5", "w1", "linux", false);
    let mut odd = versions("linux");
    odd.proto = 9;
    hub.run(&w1, Input::Unlinked { peer: m5.clone() });
    hub.run(&m5, Input::Unlinked { peer: w1.clone() });
    hub.linked.clear();
    hub.linked.push((m5.clone(), w1.clone()));
    let effects = hub.run(
        &m5,
        Input::Msg {
            from: w1.clone(),
            msg: Msg::Hello {
                node: w1.clone(),
                cluster: Some("c1".to_string()),
                versions: odd,
                boot_id: "b".to_string(),
                term: None,
            },
        },
    );
    assert!(effects.iter().any(|e| matches!(e, Effect::Send { msg: Msg::Refused { reason, .. }, .. } if reason.contains("node protocol 9"))));
    assert!(effects.iter().any(|e| matches!(e, Effect::Close { .. })));
    assert!(!hub.core(&m5).online().contains(&w1));
}

#[test]
fn removed_node_gets_removed_and_wipes() {
    let (mut hub, m5, w1) = pair();
    hub.local(&m5, LocalCmd::Remove { node: w1.clone() });
    assert!(hub.core(&w1).state().is_none());
    assert!(hub
        .effects_of("w1")
        .iter()
        .any(|e| matches!(e, Effect::Wipe)));
    assert_eq!(
        hub.core(&m5).state().expect("state").members[&w1].state,
        MemberState::Removed
    );
    let (pins, _) = hub.core(&m5).pin_set();
    assert!(!pins.contains(&fp("w1")));
}

#[test]
fn removed_node_is_told_when_it_says_hello() {
    let (mut hub, m5, w1) = pair();
    hub.local(&m5, LocalCmd::Remove { node: w1.clone() });
    let node = hub.nodes.remove(&w1).expect("node");
    hub.nodes.insert(w1.clone(), node);
    hub.linked.push((m5.clone(), w1.clone()));
    let effects = hub.run(
        &m5,
        Input::Msg {
            from: w1.clone(),
            msg: Msg::Hello {
                node: w1.clone(),
                cluster: Some("c1".to_string()),
                versions: versions("linux"),
                boot_id: "b".to_string(),
                term: None,
            },
        },
    );
    assert!(effects.iter().any(|e| matches!(e, Effect::Send { msg: Msg::Refused { reason, .. }, .. } if reason == "removed")));
}

#[test]
fn peer_goes_offline_after_20s_silence() {
    let (mut hub, m5, w1) = pair();
    hub.tick_all(1_000);
    assert!(hub.core(&m5).online().contains(&w1));
    hub.tick(&m5, OFFLINE_MS);
    assert!(!hub.core(&m5).online().contains(&w1));
    assert!(hub
        .seen
        .iter()
        .any(|(n, e)| n == "m5" && matches!(e, Effect::Close { peer } if *peer == w1)));
}

#[test]
fn pings_keep_a_quiet_peer_online() {
    let (mut hub, m5, w1) = pair();
    for _ in 0..8 {
        hub.tick_all(PING_MS);
    }
    assert!(hub.core(&m5).online().contains(&w1));
    assert!(hub.core(&w1).online().contains(&m5));
}

#[test]
fn node_dials_home_with_backoff() {
    let mut hub = Hub::new();
    let _m5 = hub.add("m5", "macos", true);
    let w1 = hub.add("w1", "linux", false);
    hub.create("m5");
    let home_state = hub.core(&id("m5")).state().cloned().expect("state");
    let mut worker_member = member("w1", "linux", false);
    worker_member.state = MemberState::Active;
    let sync = Command::Sync {
        id: "c1".to_string(),
        name: "cluster".to_string(),
        term: home_state.term.clone(),
        home: id("m5"),
        vault: id("m5"),
        members: vec![home_state.members[&id("m5")].clone(), worker_member],
    };
    let (state, _, _) = crate::cluster::state::start(sync, 0).expect("state");
    let mut worker = NodeCore::new(w1.clone(), versions("linux"), Some(state), false);
    let mut dial_times = Vec::new();
    for t in (0..=70_000u64).step_by(500) {
        let effects = worker.step(Input::Tick {
            boot_ms: t,
            wall_ms: t as i64,
        });
        if effects
            .iter()
            .any(|e| matches!(e, Effect::Dial { to, .. } if *to == id("m5")))
        {
            dial_times.push(t);
        }
    }
    assert_eq!(
        dial_times,
        vec![0, 1_000, 3_000, 7_000, 15_000, 31_000, 63_000]
    );
}

#[test]
fn can_be_master_node_dials_every_member_and_worker_only_home() {
    let mut hub = Hub::new();
    hub.add("m5", "macos", true);
    hub.add("m1", "macos", true);
    hub.add("w1", "linux", false);
    hub.add("w2", "linux", false);
    hub.create("m5");
    hub.join("m5", "m1", "macos", true);
    hub.join("m5", "w1", "linux", false);
    hub.join("m5", "w2", "linux", false);
    let dials = |hub: &mut Hub, who: &str| -> Vec<NodeId> {
        hub.linked.clear();
        let node = id(who);
        let core = hub.nodes.get_mut(&node).expect("node");
        for peer in core.online() {
            core.step(Input::Unlinked { peer });
        }
        hub.now += 100_000;
        let now = hub.now;
        let effects = hub.nodes.get_mut(&node).expect("node").step(Input::Tick {
            boot_ms: now,
            wall_ms: now as i64,
        });
        effects
            .into_iter()
            .filter_map(|e| match e {
                Effect::Dial { to, .. } => Some(to),
                _ => None,
            })
            .collect()
    };
    let m1 = dials(&mut hub, "m1");
    assert!(m1.contains(&id("w1")) && m1.contains(&id("w2")), "{m1:?}");
    let m5 = dials(&mut hub, "m5");
    assert_ne!(m1.contains(&id("m5")), m5.contains(&id("m1")));
    assert_eq!(m1.contains(&id("m5")), id("m1") < id("m5"));
    let w1 = dials(&mut hub, "w1");
    assert_eq!(w1, vec![id("m5")]);
}

#[test]
fn home_pushes_members_on_change() {
    let mut hub = Hub::new();
    hub.add("m5", "macos", true);
    hub.add("w1", "linux", false);
    hub.add("w2", "linux", false);
    hub.create("m5");
    hub.join("m5", "w1", "linux", false);
    hub.seen.clear();
    hub.join("m5", "w2", "linux", false);
    let pushed = hub
        .seen
        .iter()
        .filter(|(n, e)| n == "m5" && matches!(e, Effect::Send { to, msg: Msg::Members { members, .. } } if *to == id("w1") && members.len() == 3))
        .count();
    assert!(pushed >= 1);
    assert_eq!(hub.core(&id("w1")).state().expect("state").members.len(), 3);
}

#[test]
fn pins_follow_membership() {
    let (mut hub, m5, w1) = pair();
    let pins: Vec<_> = hub
        .seen
        .iter()
        .filter_map(|(n, e)| match e {
            Effect::Pins {
                fingerprints,
                join_open,
            } if n == "m5" => Some((fingerprints.clone(), *join_open)),
            _ => None,
        })
        .collect();
    assert!(pins.iter().any(|(f, open)| *open && f.is_empty()));
    assert!(pins.iter().any(|(f, _)| f.contains(&fp("w1"))));
    let (last, open) = pins.last().cloned().expect("pins");
    assert_eq!(last, vec![fp("w1")]);
    assert!(!open);
    hub.local(&m5, LocalCmd::Remove { node: w1 });
    let (now, _) = hub.core(&m5).pin_set();
    assert!(now.is_empty());
}
