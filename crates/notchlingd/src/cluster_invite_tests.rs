use super::{invite_item, now_ms, parse_invite, raise_joined};
use crate::acting_ops_tests::FakeRoom;
use crate::cluster_node::{Hooks, InviteJoin, JoinAsk, Submit};
use crate::cluster_ops::{follow_switch, ClusterOps};
use crate::cluster_testkit::{eventually, node, online, pair, Dir, Opts, TestNode};
use crate::inbox_ops::InboxOps;
use crate::op_group::OpGroup;
use crate::switches::{SwitchFlag, CLUSTER, CLUSTER_OFF};
use notchling_cluster_net::MemNet;
use notchling_engine::cluster::join::decode_token;
use notchling_engine::cluster::wire::OriginTier;
use notchling_engine::cluster::{LocalCmd, NodeId};
use notchling_engine::inbox::{Item, Kind};
use notchling_local_api::{ops, Caller, ClientKind, ErrorCode, OpSpec, Tier};
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};

fn caller(tier: Tier, client: ClientKind) -> Caller {
    Caller {
        tier,
        client,
        peer: None,
        pid: None,
    }
}

fn app() -> Caller {
    caller(Tier::App, ClientKind::App)
}

fn ops_of(n: &TestNode) -> ClusterOps {
    let flag = SwitchFlag::off(CLUSTER, CLUSTER_OFF);
    follow_switch(&flag, &n.node);
    flag.set(true);
    ClusterOps::new(Ok(n.node.clone()), flag, n.room.clone(), n.log.clone())
}

fn args_for(node: &NodeId, name: &str) -> Value {
    json!({"node": node, "name": name, "canBeMaster": false, "acting": true})
}

fn open_invites(n: &TestNode) -> Vec<Value> {
    n.node.status()["invites"]
        .as_array()
        .cloned()
        .unwrap_or_default()
}

#[derive(Default)]
struct Recorder {
    joined: Mutex<Vec<InviteJoin>>,
    asks: Mutex<Vec<NodeId>>,
    inbox: Mutex<Option<Arc<InboxOps>>>,
}

impl Hooks for Recorder {
    fn join_request(&self, ask: JoinAsk, _node: Submit) {
        self.asks.lock().unwrap().push(ask.node);
    }

    fn invite_joined(&self, joined: InviteJoin) {
        if let Some(inbox) = self.inbox.lock().unwrap().as_ref() {
            raise_joined(inbox, &joined);
        }
        self.joined.lock().unwrap().push(joined);
    }
}

struct World {
    home: TestNode,
    joiner: TestNode,
    recorder: Arc<Recorder>,
    inbox: Arc<InboxOps>,
    token: String,
    _dir: Dir,
}

fn invited_world(tag: &'static str, joiner_tag: &'static str) -> World {
    let net = MemNet::new();
    let dir = Dir::new(&format!("{tag}-inbox"));
    let inbox = InboxOps::new(dir.path().to_path_buf(), Arc::new(FakeRoom::default()));
    let recorder = Arc::new(Recorder::default());
    *recorder.inbox.lock().unwrap() = Some(inbox.clone());
    let mut opts = Opts::named(tag);
    opts.hooks = Some(recorder.clone());
    let home = node(&net, opts);
    home.node.set_link(true);
    home.node.create("home").unwrap();
    let joiner = node(&net, Opts::named(joiner_tag));
    joiner.node.set_link(true);
    let made = ops_of(&home)
        .call(&app(), ops::CLUSTER_INVITE, &args_for(&joiner.id(), "box9"))
        .unwrap();
    let token = made["token"].as_str().unwrap().to_string();
    World {
        home,
        joiner,
        recorder,
        inbox,
        token,
        _dir: dir,
    }
}

fn join(w: &World) {
    let joined = w.joiner.node.join(&w.token, true).unwrap();
    assert_ne!(joined["state"], "pending", "{joined}");
    eventually("the invited node is online", || {
        online(&w.home.node.status(), &w.joiner.id())
    });
}

#[test]
fn agent_cannot_invite() {
    let net = MemNet::new();
    let (home, worker) = pair(&net, Opts::named("invhome1"), Opts::named("invbox1"));
    let ops = ops_of(&home);
    assert_eq!(ops.spec(ops::CLUSTER_INVITE), Some(OpSpec::APP));
    let args = args_for(&worker.id(), "box9");
    let mut with_minter = args.clone();
    with_minter["minter"] = json!(worker.id());
    for (who, args) in [
        (caller(Tier::Agent, ClientKind::Cli), &args),
        (caller(Tier::Agent, ClientKind::Hook), &args),
        (caller(Tier::Agent, ClientKind::Node), &with_minter),
    ] {
        let err = ops.call(&who, ops::CLUSTER_INVITE, args).unwrap_err();
        assert_eq!(err.code, ErrorCode::Forbidden, "{who:?}");
    }
    assert!(open_invites(&home).is_empty());
}

#[test]
fn app_invite_returns_token_and_expiry() {
    let net = MemNet::new();
    let (home, worker) = pair(&net, Opts::named("invhome2"), Opts::named("invbox2"));
    let before = now_ms();
    let made = ops_of(&home)
        .call(&app(), ops::CLUSTER_INVITE, &args_for(&worker.id(), "box9"))
        .unwrap();
    let token = decode_token(made["token"].as_str().unwrap()).unwrap();
    assert_eq!(token.home, home.id());
    let expires = made["expiresMs"].as_i64().unwrap();
    assert!(
        expires >= before + 590_000 && expires <= now_ms() + 610_000,
        "{expires}"
    );
    let invites = open_invites(&home);
    assert_eq!(invites.len(), 1);
    assert_eq!(invites[0]["node"], json!(worker.id()));
    assert_eq!(invites[0]["name"], "box9");
    assert_eq!(invites[0]["expiresMs"], json!(expires));
}

#[test]
fn a_node_in_no_cluster_cannot_invite() {
    let net = MemNet::new();
    let alone = node(&net, Opts::named("invalone"));
    let err = ops_of(&alone)
        .call(&app(), ops::CLUSTER_INVITE, &args_for(&alone.id(), "box9"))
        .unwrap_err();
    assert!(err.message.contains("no cluster"), "{err:?}");
}

#[test]
fn bad_node_id_is_bad_request() {
    let net = MemNet::new();
    let (home, worker) = pair(&net, Opts::named("invhome3"), Opts::named("invbox3"));
    let ops = ops_of(&home);
    let id = worker.id().to_string();
    for args in [
        json!({"name": "box9"}),
        json!({"node": "", "name": "box9"}),
        json!({"node": "xyz", "name": "box9"}),
        json!({"node": id.to_uppercase(), "name": "box9"}),
        json!({"node": format!("{id}0"), "name": "box9"}),
        json!({"node": 7, "name": "box9"}),
        json!({"node": id}),
        json!({"node": id, "name": ""}),
        json!({"node": id, "name": "Box 9"}),
        json!({"node": id, "name": "-box"}),
        json!({"node": id, "name": "box-20260101"}),
        json!({"node": id, "name": "box9", "canBeMaster": "yes"}),
        json!({"node": id, "name": "box9", "acting": 1}),
    ] {
        let err = ops.call(&app(), ops::CLUSTER_INVITE, &args).unwrap_err();
        assert_eq!(err.code, ErrorCode::BadRequest, "{args}");
    }
    assert!(open_invites(&home).is_empty());
}

#[test]
fn choices_default_to_off() {
    let invite = parse_invite(&json!({"node": "0123456789abcdef", "name": "box9"})).unwrap();
    assert!(!invite.can_be_master && !invite.acting);
    let invite = parse_invite(
        &json!({"node": "0123456789abcdef", "name": "box9", "canBeMaster": true, "acting": true}),
    )
    .unwrap();
    assert!(invite.can_be_master && invite.acting);
}

fn client_of(net: &Arc<MemNet>, home: &TestNode) -> TestNode {
    let mut opts = Opts::named("invclient");
    opts.client = true;
    let c = node(net, opts);
    c.node.set_link(true);
    let token = home.node.join_token().unwrap()["token"]
        .as_str()
        .unwrap()
        .to_string();
    assert_eq!(c.node.join(&token, false).unwrap()["state"], "pending");
    home.node
        .local(LocalCmd::Approve {
            node: c.id(),
            client: true,
            can_be_master: false,
        })
        .unwrap();
    eventually("the client is online", || {
        online(&home.node.status(), &c.id()) && online(&c.node.status(), &home.id())
    });
    c
}

#[test]
fn client_invite_is_forwarded_with_minter() {
    let net = MemNet::new();
    let (home, worker) = pair(&net, Opts::named("invhome4"), Opts::named("invbox4"));
    let c = client_of(&net, &home);
    let mut args = args_for(&worker.id(), "box9");
    args["minter"] = json!(worker.id());
    ops_of(&c).call(&app(), ops::CLUSTER_INVITE, &args).unwrap();
    let calls = home.calls.calls.lock().unwrap();
    assert_eq!(calls.len(), 1);
    let (op, sent, origin) = &calls[0];
    assert_eq!(op, "cluster.invite");
    assert_eq!(sent["minter"], json!(c.id()), "{sent}");
    assert_eq!(sent["node"], json!(worker.id()));
    assert_eq!(sent["name"], "box9");
    assert_eq!(sent["acting"], true);
    assert_eq!(origin.tier, OriginTier::App);
    assert_eq!(origin.node, c.id());
}

#[test]
fn standby_without_app_cannot_forward_invite() {
    let net = MemNet::new();
    let (home, worker) = pair(&net, Opts::named("invhome5"), Opts::named("invbox5"));
    let reply = ops_of(&worker).call(&app(), ops::CLUSTER_INVITE, &args_for(&worker.id(), "box9"));
    assert!(reply.is_err(), "{reply:?}");
    assert!(home.calls.calls.lock().unwrap().is_empty());
    assert!(open_invites(&home).is_empty());
}

#[test]
fn only_the_primary_takes_a_forwarded_invite() {
    let net = MemNet::new();
    let (home, worker) = pair(&net, Opts::named("invhome6"), Opts::named("invbox6"));
    let mut args = args_for(&worker.id(), "box9");
    args["minter"] = json!(home.id());
    let err = ops_of(&worker)
        .call(
            &caller(Tier::App, ClientKind::Node),
            ops::CLUSTER_INVITE,
            &args,
        )
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::Forbidden, "{err:?}");
    assert!(open_invites(&home).is_empty());
}

#[test]
fn status_invites_hold_no_token() {
    let w = invited_world("invhome7", "invbox7");
    let secret = decode_token(&w.token).unwrap().secret;
    let status = w.home.node.status().to_string();
    for planted in [&w.token, &secret] {
        assert!(!status.contains(planted.as_str()), "{status}");
    }
    assert_eq!(open_invites(&w.home).len(), 1);
    for data in w.home.room.named("cluster.changed") {
        let text = data.to_string();
        for planted in [&w.token, &secret] {
            assert!(!text.contains(planted.as_str()), "{text}");
        }
    }
    join(&w);
    eventually("the invite is spent", || open_invites(&w.home).is_empty());
}

#[test]
fn events_carry_no_token() {
    let w = invited_world("invhome8", "invbox8");
    join(&w);
    eventually("the invite_joined event", || {
        w.home.audit_text().contains("step=invite_joined")
    });
    let text = w.home.audit_text();
    assert!(text.contains("step=invited"), "{text}");
    let secret = decode_token(&w.token).unwrap().secret;
    for n in [&w.home, &w.joiner] {
        let all: String = n.sink.events().iter().map(|e| format!("{e:?}")).collect();
        for planted in [&w.token, &secret] {
            assert!(!all.contains(planted.as_str()), "{all}");
        }
    }
}

fn invited_peers(n: &TestNode) -> Vec<String> {
    n.audit_text()
        .lines()
        .filter(|l| l.contains("step=invited "))
        .map(|l| {
            l.split(' ')
                .find_map(|f| f.strip_prefix("peer="))
                .unwrap_or("")
                .to_string()
        })
        .collect()
}

#[test]
fn invited_event_names_the_primary_for_its_own_app() {
    let w = invited_world("invhome12", "invbox12");
    eventually("the invited event", || !invited_peers(&w.home).is_empty());
    assert_eq!(invited_peers(&w.home), vec![w.home.id().to_string()]);
    join(&w);
    eventually("the invite_joined event", || {
        w.home.audit_text().contains("step=invite_joined")
    });
    let line = w
        .home
        .audit_text()
        .lines()
        .find(|l| l.contains("step=invite_joined"))
        .unwrap()
        .to_string();
    assert!(line.contains(&format!("peer={}", w.joiner.id())), "{line}");
}

#[test]
fn invited_event_names_the_client_that_forwarded() {
    let net = MemNet::new();
    let (home, worker) = pair(&net, Opts::named("invhome13"), Opts::named("invbox13"));
    let c = client_of(&net, &home);
    let mut args = args_for(&worker.id(), "box9");
    args["minter"] = json!(c.id());
    ops_of(&home)
        .call(
            &caller(Tier::App, ClientKind::Node),
            ops::CLUSTER_INVITE,
            &args,
        )
        .unwrap();
    eventually("the invited event", || !invited_peers(&home).is_empty());
    assert_eq!(invited_peers(&home), vec![c.id().to_string()]);
    assert_ne!(invited_peers(&home), vec![worker.id().to_string()]);
}

fn notices(inbox: &InboxOps) -> Vec<Item> {
    inbox
        .items()
        .into_iter()
        .filter(|i| i.kind == Kind::Notice)
        .collect()
}

#[test]
fn invite_joined_raises_notice_without_actions() {
    let w = invited_world("invhome9", "invbox9");
    join(&w);
    eventually("a notice", || !notices(&w.inbox).is_empty());
    let item = notices(&w.inbox).remove(0);
    assert_eq!(item.title, "box9 joined the cluster");
    assert!(item.actions.is_empty(), "{item:?}");
    assert!(
        item.detail.starts_with("Added over SSH from "),
        "{}",
        item.detail
    );
    assert!(
        item.detail.contains(&w.joiner.id().to_string()),
        "{}",
        item.detail
    );
    let secret = decode_token(&w.token).unwrap().secret;
    assert!(!format!("{item:?}").contains(&secret));
    assert!(w.recorder.asks.lock().unwrap().is_empty());
    assert_eq!(w.recorder.joined.lock().unwrap().len(), 1);
    assert!(w.inbox.items().iter().all(|i| i.kind != Kind::NodeJoin));
}

#[test]
fn invite_item_cleans_what_a_machine_reported() {
    let joined = InviteJoin {
        node: NodeId::parse("0123456789abcdef").unwrap(),
        name: "box9".into(),
        os: "macos".into(),
        arch: "arm64\n\u{1b}[31mred".into(),
        minter: NodeId::parse("fedcba9876543210").unwrap(),
        minter_name: "m5\tx".into(),
    };
    let item = invite_item(&joined, 7);
    assert!(
        !item.detail.chars().any(char::is_control),
        "{}",
        item.detail
    );
    assert!(item.detail.contains("macOS"), "{}", item.detail);
    assert_eq!(item.created_ms, 7);
    assert!(item.actions.is_empty());
}

#[test]
fn a_token_without_an_invite_still_asks() {
    let w = invited_world("invhome10", "invbox10");
    let plain = w.home.node.join_token().unwrap()["token"]
        .as_str()
        .unwrap()
        .to_string();
    let stranger = node(&MemNet::new(), Opts::named("invother"));
    drop(stranger);
    let net = MemNet::new();
    let dir = Dir::new("invhome11-inbox");
    let inbox = InboxOps::new(dir.path().to_path_buf(), Arc::new(FakeRoom::default()));
    let recorder = Arc::new(Recorder::default());
    *recorder.inbox.lock().unwrap() = Some(inbox.clone());
    let mut opts = Opts::named("invhome11");
    opts.hooks = Some(recorder.clone());
    let home = node(&net, opts);
    home.node.set_link(true);
    home.node.create("home").unwrap();
    let joiner = node(&net, Opts::named("invbox11"));
    joiner.node.set_link(true);
    let token = home.node.join_token().unwrap()["token"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(plain.starts_with("nlj1."));
    assert_eq!(joiner.node.join(&token, false).unwrap()["state"], "pending");
    eventually("the join is asked", || {
        recorder.asks.lock().unwrap().contains(&joiner.id())
    });
    assert!(recorder.joined.lock().unwrap().is_empty());
    assert!(notices(&inbox).is_empty());
    assert!(!online(&home.node.status(), &joiner.id()));
}
