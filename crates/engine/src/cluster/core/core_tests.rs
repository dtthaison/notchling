use std::collections::{BTreeMap, VecDeque};

use serde_json::{json, Value};

use super::{Effect, Input, LocalCmd, NodeCore, CALL_MS, DEDUPE_MS, OFFLINE_MS, PING_MS};
use crate::cluster::id::{NodeId, Tool};
use crate::cluster::jobs::{JobSpec, JobState};
use crate::cluster::join::JoinToken;
use crate::cluster::members::{MemberState, Term};
use crate::cluster::state::Command;
use crate::cluster::testkit::{fp, id, member, versions};
use crate::cluster::wire::{Msg, Origin, OriginTier, WireError};

struct Hub {
    nodes: BTreeMap<NodeId, NodeCore>,
    names: BTreeMap<NodeId, String>,
    linked: Vec<(NodeId, NodeId)>,
    seen: Vec<(String, Effect)>,
    now: u64,
}

impl Hub {
    fn new() -> Hub {
        Hub {
            nodes: BTreeMap::new(),
            names: BTreeMap::new(),
            linked: Vec::new(),
            seen: Vec::new(),
            now: 0,
        }
    }

    fn add(&mut self, name: &str, os: &str, client: bool) -> NodeId {
        let node = id(name);
        let core = NodeCore::new(node.clone(), versions(os), None, client)
            .with_boot_id(format!("boot-{name}"));
        self.nodes.insert(node.clone(), core);
        self.names.insert(node.clone(), name.to_string());
        node
    }

    fn run(&mut self, who: &NodeId, input: Input) -> Vec<Effect> {
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

    fn is_linked(&self, a: &NodeId, b: &NodeId) -> bool {
        self.linked
            .iter()
            .any(|(x, y)| (x == a && y == b) || (x == b && y == a))
    }

    fn link(&mut self, a: &NodeId, b: &NodeId) {
        self.linked.push((a.clone(), b.clone()));
        self.run(a, Input::Linked { peer: b.clone() });
        self.run(b, Input::Linked { peer: a.clone() });
    }

    fn tick(&mut self, who: &NodeId, ms: u64) -> Vec<Effect> {
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

    fn tick_all(&mut self, ms: u64) {
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

    fn local(&mut self, who: &NodeId, cmd: LocalCmd) -> Vec<Effect> {
        self.run(who, Input::Local { id: 1, cmd })
    }

    fn core(&self, who: &NodeId) -> &NodeCore {
        &self.nodes[who]
    }

    fn effects_of(&self, name: &str) -> Vec<&Effect> {
        self.seen
            .iter()
            .filter(|(n, _)| n == name)
            .map(|(_, e)| e)
            .collect()
    }

    fn create(&mut self, home: &str) {
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

    fn join(&mut self, home: &str, who: &str, os: &str, can_be_master: bool) {
        self.join_as(home, who, os, false, can_be_master);
    }

    fn join_as(&mut self, home: &str, who: &str, os: &str, client: bool, can_be_master: bool) {
        let (h, w) = (id(home), id(who));
        let secret = format!("secret-{who}");
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
        let me = member(who, os, false);
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

fn dispatch_cmd(key: &str, node: &str, tool: Tool) -> LocalCmd {
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

fn pair() -> (Hub, NodeId, NodeId) {
    let mut hub = Hub::new();
    let m5 = hub.add("m5", "macos", true);
    let w1 = hub.add("w1", "linux", false);
    hub.create("m5");
    hub.join("m5", "w1", "linux", false);
    (hub, m5, w1)
}

fn answers(effects: &[Effect]) -> Vec<&Result<Value, WireError>> {
    effects
        .iter()
        .filter_map(|e| match e {
            Effect::Answer { result, .. } => Some(result),
            _ => None,
        })
        .collect()
}

fn run_locals(effects: &[Effect]) -> Vec<(u64, String, Value)> {
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
fn call_reply_correlates_and_times_out() {
    let (mut hub, m5, w1) = pair();
    let origin = Origin {
        node: m5.clone(),
        tier: OriginTier::Node,
    };
    let effects = hub.local(
        &m5,
        LocalCmd::Forward {
            node: w1.clone(),
            op: "node.load".to_string(),
            args: json!({}),
            origin: origin.clone(),
        },
    );
    assert!(answers(&effects).is_empty());
    let runs = run_locals(
        &hub.seen
            .iter()
            .filter(|(n, _)| n == "w1")
            .map(|(_, e)| e.clone())
            .collect::<Vec<_>>(),
    );
    let (call, op, _) = runs.last().cloned().expect("run local");
    assert_eq!(op, "node.load");
    let effects = hub.run(
        &w1,
        Input::LocalDone {
            call,
            result: Ok(json!({"cpu": 3})),
        },
    );
    assert!(effects.iter().any(
        |e| matches!(e, Effect::Send { msg: Msg::Reply { ok: Some(v), .. }, .. } if v["cpu"] == 3)
    ));
    let answered: Vec<_> = hub
        .seen
        .iter()
        .filter(|(n, e)| {
            n == "m5" && matches!(e, Effect::Answer { result: Ok(v), .. } if v["cpu"] == 3)
        })
        .collect();
    assert_eq!(answered.len(), 1);

    let effects = hub.local(
        &m5,
        LocalCmd::Forward {
            node: w1.clone(),
            op: "node.load".to_string(),
            args: json!({}),
            origin,
        },
    );
    assert!(answers(&effects).is_empty());
    let start = hub.seen.len();
    for _ in 0..(CALL_MS / PING_MS) {
        hub.tick_all(PING_MS);
    }
    let timeouts: Vec<_> = hub.seen[start..]
        .iter()
        .filter(
            |(_, e)| matches!(e, Effect::Answer { result: Err(err), .. } if err.code == "timeout"),
        )
        .collect();
    assert_eq!(timeouts.len(), 1);
}

#[test]
fn incoming_call_from_worker_to_dispatch_is_refused() {
    let (mut hub, m5, w1) = pair();
    let effects = hub.run(
        &m5,
        Input::Msg {
            from: w1.clone(),
            msg: Msg::Call {
                id: 9,
                op: "session.new".to_string(),
                args: json!({}),
                origin: Origin {
                    node: w1.clone(),
                    tier: OriginTier::Node,
                },
                term: hub.core(&m5).highest_term().cloned().expect("term"),
            },
        },
    );
    assert!(effects.iter().any(|e| matches!(e, Effect::Send { msg: Msg::Reply { err: Some(e), .. }, .. } if e.code == "forbidden")));
    assert!(effects
        .iter()
        .any(|e| matches!(e, Effect::Publish { name, .. } if name == "cluster.refused")));
    assert!(run_locals(&effects).is_empty());
}

#[test]
fn duplicate_call_runs_once() {
    let (mut hub, m5, w1) = pair();
    let term = hub.core(&w1).highest_term().cloned().expect("term");
    let call = Msg::Call {
        id: 5,
        op: "session.new".to_string(),
        args: json!({"cwd": "/x"}),
        origin: Origin {
            node: m5.clone(),
            tier: OriginTier::Node,
        },
        term,
    };
    let first = hub.run(
        &w1,
        Input::Msg {
            from: m5.clone(),
            msg: call.clone(),
        },
    );
    let runs = run_locals(&first);
    assert_eq!(runs.len(), 1);
    let again = hub.run(
        &w1,
        Input::Msg {
            from: m5.clone(),
            msg: call.clone(),
        },
    );
    assert!(run_locals(&again).is_empty());
    hub.run(
        &w1,
        Input::LocalDone {
            call: runs[0].0,
            result: Ok(json!({"session": "s"})),
        },
    );
    let third = hub.run(
        &w1,
        Input::Msg {
            from: m5.clone(),
            msg: call,
        },
    );
    assert!(run_locals(&third).is_empty());
    assert!(third.iter().any(|e| matches!(
        e,
        Effect::Send {
            msg: Msg::Reply {
                ok: Some(_),
                id: 5,
                ..
            },
            ..
        }
    )));
}

#[test]
fn dedupe_forgets_after_its_window() {
    let (mut hub, m5, w1) = pair();
    let term = hub.core(&w1).highest_term().cloned().expect("term");
    let call = Msg::Call {
        id: 5,
        op: "session.new".to_string(),
        args: json!({}),
        origin: Origin {
            node: m5.clone(),
            tier: OriginTier::Node,
        },
        term,
    };
    let first = hub.run(
        &w1,
        Input::Msg {
            from: m5.clone(),
            msg: call.clone(),
        },
    );
    let runs = run_locals(&first);
    hub.run(
        &w1,
        Input::LocalDone {
            call: runs[0].0,
            result: Ok(json!({})),
        },
    );
    for _ in 0..70 {
        hub.tick_all(5_000);
    }
    assert!(hub.now > DEDUPE_MS);
    let later = hub.run(
        &w1,
        Input::Msg {
            from: m5.clone(),
            msg: call,
        },
    );
    assert_eq!(run_locals(&later).len(), 1);
}

#[test]
fn older_term_call_is_refused() {
    let (mut hub, m5, w1) = pair();
    let newer = Term {
        n: 3,
        node: id("w1"),
    };
    hub.run(
        &w1,
        Input::Msg {
            from: m5.clone(),
            msg: Msg::HelloOk {
                node: m5.clone(),
                cluster: "c1".to_string(),
                term: newer.clone(),
                home: id("w1"),
            },
        },
    );
    assert_eq!(hub.core(&w1).highest_term(), Some(&newer));
    let effects = hub.run(
        &w1,
        Input::Msg {
            from: m5.clone(),
            msg: Msg::Call {
                id: 11,
                op: "session.new".to_string(),
                args: json!({}),
                origin: Origin {
                    node: m5.clone(),
                    tier: OriginTier::Node,
                },
                term: Term {
                    n: 1,
                    node: id("m5"),
                },
            },
        },
    );
    assert!(run_locals(&effects).is_empty());
    let reply = effects.iter().find_map(|e| match e {
        Effect::Send {
            msg: Msg::Reply { err: Some(err), .. },
            ..
        } => Some(err.clone()),
        _ => None,
    });
    let err = reply.expect("a reply");
    assert_eq!(err.code, "stale_term");
    let body: Value = serde_json::from_str(&err.message).expect("json");
    assert_eq!(body["term"]["n"], 3);
    assert_eq!(body["home"], json!(id("w1")));
}

#[test]
fn newer_term_is_adopted() {
    let (mut hub, m5, w1) = pair();
    let newer = Term {
        n: 2,
        node: id("w1"),
    };
    let effects = hub.run(
        &w1,
        Input::Msg {
            from: m5.clone(),
            msg: Msg::Ping { boot_ms: 1 },
        },
    );
    assert!(!effects.is_empty());
    let effects = hub.run(
        &w1,
        Input::Msg {
            from: m5.clone(),
            msg: Msg::Call {
                id: 2,
                op: "node.load".to_string(),
                args: json!({}),
                origin: Origin {
                    node: m5.clone(),
                    tier: OriginTier::Node,
                },
                term: newer.clone(),
            },
        },
    );
    assert_eq!(hub.core(&w1).highest_term(), Some(&newer));
    assert_eq!(hub.core(&w1).state().expect("state").home, id("w1"));
    assert!(effects.iter().any(|e| matches!(e, Effect::Persist { .. })));
}

#[test]
fn home_seeing_newer_term_steps_down() {
    let (mut hub, m5, w1) = pair();
    let newer = Term {
        n: 2,
        node: id("w1"),
    };
    let effects = hub.run(
        &m5,
        Input::Msg {
            from: w1.clone(),
            msg: Msg::Refused {
                reason: "stale_term".to_string(),
                term: Some(newer.clone()),
                home: Some(id("w1")),
            },
        },
    );
    assert!(effects.iter().any(
        |e| matches!(e, Effect::SteppedDown { term, home } if *term == newer && *home == id("w1"))
    ));
    assert!(!hub.core(&m5).is_home());
    let out = hub.local(&m5, dispatch_cmd("k", "w1", Tool::Codex));
    assert!(run_locals(&out).is_empty());
    assert!(!out.iter().any(|e| matches!(
        e,
        Effect::Send {
            msg: Msg::Members { .. },
            ..
        }
    )));
}

#[test]
fn highest_term_survives_restart() {
    let (mut hub, m5, w1) = pair();
    let newer = Term {
        n: 4,
        node: id("m5"),
    };
    hub.run(
        &w1,
        Input::Msg {
            from: m5.clone(),
            msg: Msg::HelloOk {
                node: m5.clone(),
                cluster: "c1".to_string(),
                term: newer.clone(),
                home: id("m5"),
            },
        },
    );
    let saved = hub.core(&w1).state().cloned().expect("state");
    let mut again = NodeCore::new(w1.clone(), versions("linux"), Some(saved), false);
    assert_eq!(again.highest_term(), Some(&newer));
    let effects = again.step(Input::Msg {
        from: m5.clone(),
        msg: Msg::Call {
            id: 1,
            op: "node.load".to_string(),
            args: json!({}),
            origin: Origin {
                node: m5.clone(),
                tier: OriginTier::Node,
            },
            term: Term {
                n: 1,
                node: id("m5"),
            },
        },
    });
    assert!(run_locals(&effects).is_empty());
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

#[test]
fn dispatch_to_offline_node_is_refused() {
    let (mut hub, m5, w1) = pair();
    hub.tick(&m5, OFFLINE_MS);
    assert!(!hub.core(&m5).online().contains(&w1));
    let effects = hub.local(&m5, dispatch_cmd("k", "w1", Tool::Codex));
    let a = answers(&effects);
    assert!(matches!(a[0], Err(e) if e.message == "w1 is offline"));
    assert!(hub.core(&m5).state().expect("state").jobs.is_empty());
}

#[test]
fn dispatch_runs_once_and_moves_the_job_to_running() {
    let (mut hub, m5, w1) = pair();
    let effects = hub.local(&m5, dispatch_cmd("k", "w1", Tool::Codex));
    assert!(answers(&effects).is_empty());
    let w1_effects: Vec<Effect> = hub
        .seen
        .iter()
        .filter(|(n, _)| n == "w1")
        .map(|(_, e)| e.clone())
        .collect();
    let runs = run_locals(&w1_effects);
    let (call, op, args) = runs.last().cloned().expect("run");
    assert_eq!(op, "codex.send");
    assert_eq!(args["draft_folder"], "/srv/repo");
    let job = crate::cluster::jobs::job_id("k");
    assert_eq!(args["job"], job);
    let done = hub.run(
        &w1,
        Input::LocalDone {
            call,
            result: Ok(json!({"id": "codex:abc"})),
        },
    );
    assert!(!done.is_empty());
    let state = hub.core(&m5).state().expect("state");
    assert_eq!(state.jobs[&job].state, JobState::Running);
    assert_eq!(state.jobs[&job].session.as_deref(), Some("codex:abc"));
    let answered = hub
        .seen
        .iter()
        .filter(|(n, e)| {
            n == "m5" && matches!(e, Effect::Answer { result: Ok(v), .. } if v["job"] == json!(job))
        })
        .count();
    assert_eq!(answered, 1);
    let again = hub.local(&m5, dispatch_cmd("k", "w1", Tool::Codex));
    assert!(matches!(answers(&again)[0], Ok(v) if v["job"] == json!(job)));
    assert_eq!(
        hub.seen
            .iter()
            .filter(|(n, e)| n == "w1" && matches!(e, Effect::RunLocal { .. }))
            .count(),
        1
    );
}

#[test]
fn a_failed_start_fails_the_job() {
    let (mut hub, m5, w1) = pair();
    hub.local(&m5, dispatch_cmd("k", "w1", Tool::Shell));
    let w1_effects: Vec<Effect> = hub
        .seen
        .iter()
        .filter(|(n, _)| n == "w1")
        .map(|(_, e)| e.clone())
        .collect();
    let (call, op, _) = run_locals(&w1_effects).last().cloned().expect("run");
    assert_eq!(op, "shell.run");
    hub.run(
        &w1,
        Input::LocalDone {
            call,
            result: Err(WireError::new("failed", "no shell")),
        },
    );
    let job = crate::cluster::jobs::job_id("k");
    let state = hub.core(&m5).state().expect("state");
    assert_eq!(state.jobs[&job].state, JobState::Failed);
    assert_eq!(state.jobs[&job].detail.as_deref(), Some("no shell"));
}

#[test]
fn dispatch_from_client_goes_through_home() {
    let mut hub = Hub::new();
    let m5 = hub.add("m5", "macos", true);
    let m1 = hub.add("m1", "macos", true);
    let w1 = hub.add("w1", "linux", false);
    hub.create("m5");
    hub.join_as("m5", "m1", "macos", true, true);
    hub.join("m5", "w1", "linux", false);
    assert!(hub.core(&m5).state().expect("state").members[&m1].client);
    hub.link(&m1, &w1);
    hub.tick_all(0);
    let effects = hub.local(&m1, dispatch_cmd("k", "w1", Tool::Codex));
    assert!(answers(&effects).is_empty());
    let calls: Vec<_> = effects
        .iter()
        .filter(|e| matches!(e, Effect::Send { to, msg: Msg::Call { op, .. } } if *to == m5 && op == "cluster.dispatch"))
        .collect();
    assert_eq!(calls.len(), 1);
    let w1_effects: Vec<Effect> = hub
        .seen
        .iter()
        .filter(|(n, _)| n == "w1")
        .map(|(_, e)| e.clone())
        .collect();
    let (call, _, _) = run_locals(&w1_effects).last().cloned().expect("run");
    hub.run(
        &w1,
        Input::LocalDone {
            call,
            result: Ok(json!({"id": "s"})),
        },
    );
    let answered = hub
        .seen
        .iter()
        .filter(|(n, e)| {
            n == "m1"
                && matches!(e, Effect::Answer { result: Ok(v), .. } if v["state"] == "running")
        })
        .count();
    assert_eq!(answered, 1);
}

#[test]
fn tool_mismatch_dispatches_with_a_warning() {
    let mut hub = Hub::new();
    let m5 = hub.add("m5", "macos", true);
    let m1 = hub.add("m1", "macos", true);
    hub.create("m5");
    hub.join("m5", "m1", "macos", true);
    let mut older = versions("macos");
    older.tools.insert(
        crate::versions::Tool::Claude,
        crate::versions::Version::parse("2.1.323").expect("version"),
    );
    hub.run(
        &m1,
        Input::Local {
            id: 7,
            cmd: LocalCmd::SetVersions { versions: older },
        },
    );
    assert!(hub.core(&m5).state().expect("state").members[&m1]
        .versions
        .as_ref()
        .is_some_and(|v| v.tools[&crate::versions::Tool::Claude].as_str() == "2.1.323"));
    hub.local(&m5, dispatch_cmd("k", "m1", Tool::Claude));
    let m1_effects: Vec<Effect> = hub
        .seen
        .iter()
        .filter(|(n, _)| n == "m1")
        .map(|(_, e)| e.clone())
        .collect();
    let (call, op, _) = run_locals(&m1_effects).last().cloned().expect("run");
    assert_eq!(op, "session.new");
    hub.run(
        &m1,
        Input::LocalDone {
            call,
            result: Ok(json!({"session": "s1"})),
        },
    );
    let reply = hub
        .seen
        .iter()
        .rev()
        .find_map(|(n, e)| match e {
            Effect::Answer { result: Ok(v), .. } if n == "m5" => Some(v.clone()),
            _ => None,
        })
        .expect("answer");
    assert_eq!(
        reply["warning"],
        "m1 runs Claude Code 2.1.323, the Mac 2.1.324"
    );
}

#[test]
fn a_refusing_compat_stops_the_dispatch() {
    let (mut hub, m5, _w1) = pair();
    let effects = hub.local(&m5, dispatch_cmd("k", "w1", Tool::Claude));
    let a = answers(&effects);
    assert!(matches!(a[0], Err(e) if e.message.contains("isn't proven yet")));
    assert!(hub.core(&m5).state().expect("state").jobs.is_empty());
}

#[test]
fn dispatch_to_the_home_itself_runs_locally() {
    let (mut hub, m5, _) = pair();
    let effects = hub.local(&m5, dispatch_cmd("k", "m5", Tool::Codex));
    assert_eq!(run_locals(&effects).len(), 1);
}

#[test]
fn status_lists_roles_and_online() {
    let (hub, m5, w1) = pair();
    let status = hub.core(&m5).status();
    assert_eq!(status["cluster"]["home"], json!(m5));
    let members = status["members"].as_array().expect("members");
    let row = |id: &NodeId| {
        members
            .iter()
            .find(|m| m["id"] == json!(id))
            .expect("row")
            .clone()
    };
    assert_eq!(row(&m5)["role"], "home");
    assert_eq!(row(&w1)["role"], "worker");
    assert_eq!(row(&w1)["online"], true);
    let empty = NodeCore::new(id("x1"), versions("linux"), None, false).status();
    assert!(empty["cluster"].is_null());
}

#[test]
fn a_second_join_token_use_by_an_active_member_gets_its_answer_again() {
    let (mut hub, m5, w1) = pair();
    let effects = hub.run(
        &m5,
        Input::Msg {
            from: w1.clone(),
            msg: Msg::Join {
                secret: "spent".to_string(),
                name: "w1".to_string(),
                fingerprint: fp("w1"),
                addrs: vec![],
                port: 1,
                versions: versions("linux"),
            },
        },
    );
    assert!(effects.iter().any(|e| matches!(
        e,
        Effect::Send {
            msg: Msg::JoinOk { .. },
            ..
        }
    )));
}
