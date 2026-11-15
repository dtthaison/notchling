use super::{parse, FailoverSink, KeyMirror, NodeSink, SinkNode, KEY, STARTING};
use crate::acting_ops_tests::{app, FakeRoom};
use crate::cluster_node::Hooks;
use crate::cluster_ops::{follow_switch, ClusterOps};
use crate::cluster_testkit::{eventually, node, online, Opts, TestNode};
use crate::inbox_ops::InboxOps;
use crate::late_emit::Events;
use crate::limits_ops_tests::TempDir;
use crate::op_group::OpGroup;
use crate::settings_asks::item_id;
use crate::settings_ops::{SettingsOps, SwitchStates};
use crate::switches::{SwitchFlag, CLUSTER, CLUSTER_OFF};
use notchling_cluster_net::MemNet;
use notchling_engine::cluster::failover::Failover;
use notchling_engine::cluster::LocalCmd;
use notchling_engine::inbox::ActionId;
use notchling_engine::settings::Settings;
use notchling_events::{MemorySink, Salt};
use notchling_local_api::{ops, topics, Caller, ClientKind, ErrorCode, OpSpec, Tier};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::Arc;

fn caller(tier: Tier) -> Caller {
    Caller {
        tier,
        client: match tier {
            Tier::App => ClientKind::App,
            Tier::Agent => ClientKind::Cli,
        },
        peer: Some(PathBuf::from("/agents/notchling")),
        pid: None,
    }
}

struct Mirrored {
    mirror: Arc<KeyMirror>,
}

impl Hooks for Mirrored {
    fn failover(&self, value: Failover, order: u64) {
        self.mirror.follow(value, order);
    }
}

struct Side {
    node: TestNode,
    ops: ClusterOps,
    settings: TempDir,
    room: Arc<FakeRoom>,
}

impl Side {
    fn key(&self) -> Option<Value> {
        Settings::open(&self.settings.path("settings.json"))
            .ok()?
            .raw(KEY)
            .cloned()
    }

    fn value(&self) -> String {
        self.node.node.status()["failover"]["value"]
            .as_str()
            .unwrap_or("none")
            .to_string()
    }

    fn set(&self, tier: Tier, value: &str) -> Result<Value, notchling_local_api::ApiError> {
        self.ops.call(
            &caller(tier),
            ops::CLUSTER_FAILOVER_SET,
            &json!({"value": value}),
        )
    }
}

fn side(net: &Arc<MemNet>, name: &'static str, client: bool, preset: Option<&str>) -> Side {
    let settings = TempDir::new(&format!("fo-{name}"));
    if let Some(value) = preset {
        let mut store = Settings::open(&settings.path("settings.json")).unwrap();
        store.set(KEY, json!(value)).unwrap();
    }
    let room = Arc::new(FakeRoom::default());
    let mirror = Arc::new(KeyMirror::new(settings.path("settings.json"), room.clone()));
    let hooks: Arc<dyn Hooks> = Arc::new(Mirrored { mirror });
    let mut opts = Opts::named(name);
    opts.client = client;
    opts.hooks = Some(hooks);
    opts.room = Some(room.clone());
    let n = node(net, opts);
    eventually("the node has reported", || n.node.started());
    let flag = SwitchFlag::off(CLUSTER, CLUSTER_OFF);
    follow_switch(&flag, &n.node);
    flag.set(true);
    let ops = ClusterOps::new(Ok(n.node.clone()), flag, n.room.clone(), n.log.clone());
    Side {
        node: n,
        ops,
        settings,
        room,
    }
}

fn start(net: &Arc<MemNet>, name: &'static str) -> Side {
    let home = side(net, name, true, None);
    home.node.node.create("home").unwrap();
    home
}

fn join(
    net: &Arc<MemNet>,
    home: &Side,
    name: &'static str,
    client: bool,
    preset: Option<&str>,
) -> Side {
    let joiner = side(net, name, client, preset);
    admit(home, &joiner, client);
    joiner
}

fn admit(home: &Side, joiner: &Side, client: bool) {
    let token = home.node.node.join_token().unwrap()["token"]
        .as_str()
        .unwrap()
        .to_string();
    joiner.node.node.join(&token, false).unwrap();
    home.node
        .node
        .local(LocalCmd::Approve {
            node: joiner.node.id(),
            client,
            can_be_master: true,
        })
        .unwrap();
    eventually("both see both online", || {
        online(&home.node.node.status(), &joiner.node.id())
            && online(&joiner.node.node.status(), &home.node.id())
    });
}

#[test]
fn app_change_is_journaled_on_primary() {
    let net = MemNet::new();
    let home = start(&net, "fo-home1");
    assert_eq!(home.ops.spec(ops::CLUSTER_FAILOVER_SET), Some(OpSpec::APP));
    assert_eq!(home.value(), "off");
    let reply = home.set(Tier::App, "30m").unwrap();
    assert_eq!(reply["value"], "30m");
    assert_eq!(home.value(), "30m");
    eventually("the key follows the journal", || {
        home.key() == Some(json!("30m"))
    });
    eventually("the change is announced", || {
        home.room.named(topics::SETTINGS_CHANGED)
            == vec![json!({"key": KEY, "value": "30m", "by": "cluster"})]
    });
    home.set(Tier::App, "off").unwrap();
    eventually("off is journaled and mirrored", || {
        home.value() == "off" && home.key() == Some(json!("off"))
    });
}

#[test]
fn client_change_is_forwarded_to_primary() {
    let net = MemNet::new();
    let home = start(&net, "fo-home2");
    let client = join(&net, &home, "fo-client2", true, None);
    client.set(Tier::App, "1h").unwrap();
    assert_eq!(home.value(), "1h");
    eventually("the client hears it and mirrors it", || {
        client.value() == "1h" && client.key() == Some(json!("1h"))
    });
    eventually("the primary mirrors it", || home.key() == Some(json!("1h")));
}

#[test]
fn joining_node_takes_the_cluster_value() {
    let net = MemNet::new();
    let home = start(&net, "fo-home3");
    home.set(Tier::App, "1h").unwrap();
    let joiner = join(&net, &home, "fo-joiner3", false, Some("30m"));
    eventually("the joiner journals and writes the cluster's value", || {
        joiner.value() == "1h" && joiner.key() == Some(json!("1h"))
    });
    home.set(Tier::App, "off").unwrap();
    eventually("a later change reaches it too", || {
        joiner.value() == "off" && joiner.key() == Some(json!("off"))
    });
}

#[test]
fn the_journal_wins_over_a_stale_key() {
    let net = MemNet::new();
    let home = start(&net, "fo-home4");
    let joiner = join(&net, &home, "fo-joiner4", false, None);
    let mut store = Settings::open(&joiner.settings.path("settings.json")).unwrap();
    store.set(KEY, json!("1h")).unwrap();
    assert_eq!(joiner.value(), "off");
    home.set(Tier::App, "30m").unwrap();
    eventually("the journal wins over the stale key", || {
        joiner.key() == Some(json!("30m"))
    });
}

#[test]
fn a_node_in_no_cluster_keeps_its_key_to_itself() {
    let net = MemNet::new();
    let alone = side(&net, "fo-alone5", true, None);
    let sink = NodeSink::new(alone.node.node.clone());
    assert!(sink.set(Failover::After1h).is_ok());
    assert_eq!(alone.value(), "none");
    let err = alone.set(Tier::App, "30m").unwrap_err();
    assert_eq!(err.code, ErrorCode::Failed);
    assert!(err.message.contains("no cluster"), "{err:?}");
}

#[test]
fn a_bad_value_is_refused_before_anything_is_sent() {
    let net = MemNet::new();
    let home = start(&net, "fo-home6");
    for args in [
        json!({}),
        json!({"value": "2h"}),
        json!({"value": 30}),
        json!({"value": ""}),
    ] {
        let err = home
            .ops
            .call(&caller(Tier::App), ops::CLUSTER_FAILOVER_SET, &args)
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::BadRequest, "{args}");
    }
    assert_eq!(home.value(), "off");
}

struct AskRig {
    home: Side,
    worker: Side,
    tmp: TempDir,
    inbox: Arc<InboxOps>,
    settings: SettingsOps,
}

fn ask_rig(net: &Arc<MemNet>, name: &'static str) -> AskRig {
    let home = start(net, name);
    let worker = join(net, &home, "fo-worker-ask", true, None);
    let tmp = TempDir::new("fo-ask");
    let inbox = InboxOps::new(tmp.path("inbox"), home.room.clone());
    let states: SwitchStates = Arc::new(|_: &str| None);
    let settings = SettingsOps::new(tmp.path("data"), home.room.clone(), states)
        .with_events(Events::fixed(Arc::new(MemorySink::new()), Salt([2; 32])))
        .with_inbox(inbox.clone());
    settings.bind_failover(Arc::new(NodeSink::new(home.node.node.clone())));
    home.room.app.store(true, Ordering::SeqCst);
    home.room
        .following
        .lock()
        .unwrap()
        .push(topics::INBOX.into());
    AskRig {
        home,
        worker,
        tmp,
        inbox,
        settings,
    }
}

impl AskRig {
    fn ask(&self, value: &str) -> String {
        let reply = self
            .settings
            .call(
                &caller(Tier::Agent),
                ops::SETTINGS_SET,
                &json!({"key": KEY, "value": value}),
            )
            .unwrap();
        assert_eq!(reply["state"], "waiting", "{reply}");
        reply["id"].as_str().unwrap().to_string()
    }

    fn finished(&self, id: &str) -> Value {
        self.settings
            .call(
                &caller(Tier::Agent),
                ops::SETTINGS_ASKED,
                &json!({"id": id, "wait": 5}),
            )
            .unwrap()
    }

    fn item(&self, id: &str) -> Value {
        self.inbox.flush();
        let list = self
            .inbox
            .call(&app(), ops::INBOX_LIST, &json!({}))
            .unwrap();
        list["items"]
            .as_array()
            .unwrap()
            .iter()
            .find(|i| i["id"] == item_id(id))
            .cloned()
            .unwrap_or(Value::Null)
    }

    fn stored(&self) -> Option<Value> {
        Settings::open(&self.tmp.path("data/settings.json"))
            .ok()?
            .raw(KEY)
            .cloned()
    }
}

#[test]
fn agent_set_raises_an_item_and_deny_keeps_off() {
    let net = MemNet::new();
    let r = ask_rig(&net, "fo-home7");
    let id = r.ask("30m");
    let item = r.item(&id);
    assert_eq!(item["kind"], "setting_change");
    assert_eq!(item["title"], "Set Automatic Failover to 30m");
    assert!(
        item["detail"].as_str().unwrap().ends_with("Now off."),
        "{item}"
    );
    assert_eq!(
        r.home.value(),
        "off",
        "nothing changes before the user allows"
    );
    r.inbox
        .decide(&item_id(&id), ActionId::Deny, "mac")
        .unwrap();
    let done = r.finished(&id);
    assert_eq!(done["state"], "denied", "{done}");
    assert_eq!(r.home.value(), "off");
    assert_eq!(r.stored(), None);
    assert_eq!(r.worker.value(), "off");
}

#[test]
fn an_allowed_agent_set_is_journaled_on_the_primary_and_reaches_the_cluster() {
    let net = MemNet::new();
    let r = ask_rig(&net, "fo-home8");
    let id = r.ask("1h");
    r.inbox
        .decide(&item_id(&id), ActionId::Allow, "phone")
        .unwrap();
    let done = r.finished(&id);
    assert_eq!(done["state"], "applied", "{done}");
    assert_eq!(r.home.value(), "1h");
    assert_eq!(r.stored(), Some(json!("1h")));
    eventually("the other member follows", || r.worker.value() == "1h");
}

#[test]
fn a_set_while_the_primary_is_unreachable_fails_and_the_key_keeps_the_clusters_value() {
    let net = MemNet::new();
    let home = start(&net, "fo-home9");
    let worker = join(&net, &home, "fo-worker9", true, None);
    home.set(Tier::App, "30m").unwrap();
    eventually("the worker mirrors 30m", || {
        worker.key() == Some(json!("30m"))
    });
    let sink: Arc<dyn FailoverSink> = Arc::new(NodeSink::new(worker.node.node.clone()));
    home.node.node.set_link(false);
    eventually("the worker sees the primary gone", || {
        !online(&worker.node.node.status(), &home.node.id())
    });
    let err = sink.set(Failover::Off).unwrap_err();
    assert!(err.contains("offline"), "{err}");
    assert_eq!(worker.value(), "30m");
    assert_eq!(worker.key(), Some(json!("30m")));
}

#[test]
fn the_value_parses_only_the_three_choices() {
    assert_eq!(parse(&json!("off")), Some(Failover::Off));
    assert_eq!(parse(&json!("30m")), Some(Failover::After30m));
    assert_eq!(parse(&json!("1h")), Some(Failover::After1h));
    assert_eq!(parse(&json!("2h")), None);
    assert_eq!(parse(&json!(true)), None);
}

#[test]
fn an_older_mirror_event_never_overwrites_a_newer_one() {
    let tmp = TempDir::new("fo-order");
    let room = Arc::new(FakeRoom::default());
    let mirror = KeyMirror::new(tmp.path("settings.json"), room.clone());
    mirror.follow(Failover::After1h, 2);
    mirror.follow(Failover::After30m, 1);
    let store = Settings::open(&tmp.path("settings.json")).unwrap();
    assert_eq!(store.raw(KEY), Some(&json!("1h")));
    assert_eq!(room.named(topics::SETTINGS_CHANGED).len(), 1);
}

#[test]
fn a_rejoin_after_a_wipe_still_overwrites_the_key() {
    let net = MemNet::new();
    let a = start(&net, "fo-wipe-a");
    let x = join(&net, &a, "fo-wipe-x", true, None);
    a.set(Tier::App, "30m").unwrap();
    eventually("x mirrors 30m", || x.key() == Some(json!("30m")));
    a.node
        .node
        .local(LocalCmd::Remove { node: x.node.id() })
        .unwrap();
    eventually("x is wiped", || x.node.node.status()["cluster"].is_null());
    let b = start(&net, "fo-wipe-b");
    admit(&b, &x, true);
    assert_eq!(b.value(), "off");
    eventually("the key follows the new cluster's off", || {
        x.value() == "off" && x.key() == Some(json!("off"))
    });
}

#[test]
fn a_started_node_reports_started() {
    let net = MemNet::new();
    let home = start(&net, "fo-started");
    eventually("the first tick has reported", || home.node.node.started());
}

struct Fake {
    started: bool,
    cluster: bool,
    journaled: std::sync::Mutex<Vec<Failover>>,
}

impl Fake {
    fn sink(started: bool, cluster: bool) -> (Arc<Fake>, NodeSink) {
        let fake = Arc::new(Fake {
            started,
            cluster,
            journaled: std::sync::Mutex::new(Vec::new()),
        });
        (fake.clone(), NodeSink::over(fake))
    }

    fn calls(&self) -> usize {
        self.journaled.lock().unwrap().len()
    }
}

impl SinkNode for Fake {
    fn started(&self) -> bool {
        self.started
    }

    fn in_cluster(&self) -> bool {
        self.cluster
    }

    fn journal(&self, value: Failover) -> Result<(), String> {
        self.journaled.lock().unwrap().push(value);
        Ok(())
    }
}

#[test]
fn a_set_before_the_node_has_reported_is_refused_and_never_unjournaled() {
    let (fake, sink) = Fake::sink(false, false);
    assert_eq!(sink.set(Failover::After30m), Err(STARTING.to_string()));
    assert_eq!(fake.calls(), 0);
    let (fake, sink) = Fake::sink(false, true);
    assert_eq!(sink.set(Failover::After30m), Err(STARTING.to_string()));
    assert_eq!(fake.calls(), 0);
}

#[test]
fn a_started_node_in_no_cluster_lets_the_key_stand_and_one_in_a_cluster_journals() {
    let (fake, sink) = Fake::sink(true, false);
    assert_eq!(sink.set(Failover::After30m), Ok(()));
    assert_eq!(fake.calls(), 0);
    let (fake, sink) = Fake::sink(true, true);
    assert_eq!(sink.set(Failover::After1h), Ok(()));
    assert_eq!(*fake.journaled.lock().unwrap(), vec![Failover::After1h]);
}
