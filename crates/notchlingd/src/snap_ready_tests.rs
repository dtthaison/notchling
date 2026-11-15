use super::*;
use crate::cluster_node::Hooks;
use crate::cluster_testkit::{eventually, member, pair, Opts, TestNode};
use crate::limits_ops_tests::TempDir;
use crate::snap_send::{SenderParts, SnapPeers};
use notchling_cluster_net::MemNet;
use notchling_engine::cluster::ack_book::AckBook;
use notchling_engine::cluster::core::SnapAsk;
use notchling_engine::cluster::ready::HostFacts;
use notchling_engine::cluster::snapshot::{Manifest, Part, PartKind};
use notchling_engine::cluster::Term;
use notchling_engine::cluster::Versions;
use notchling_engine::versions::{Tool, Version};
use serde_json::json;
use std::sync::Mutex;
use std::time::Duration;

fn facts(encrypted: Option<bool>) -> HostFacts {
    HostFacts {
        encrypted,
        home: "/Users/son".to_string(),
        user: "son".to_string(),
        free_mb: 100_000,
    }
}

fn versions(host: Option<HostFacts>) -> Versions {
    let mut v = crate::cluster_versions::own();
    v.tools
        .insert(Tool::Claude, Version::parse("2.1.1").unwrap());
    v.tools
        .insert(Tool::Codex, Version::parse("0.1.0").unwrap());
    v.host = host;
    v
}

fn id(c: char) -> String {
    c.to_string().repeat(16)
}

fn status(me: &str, home: &str, members: &[(&str, bool, Option<HostFacts>)]) -> Value {
    json!({
        "me": me,
        "cluster": {"home": home},
        "members": members.iter().map(|(m, can, host)| json!({
            "id": m,
            "name": m,
            "canBeMaster": can,
            "state": "active",
            "versions": versions(host.clone()),
        })).collect::<Vec<_>>(),
    })
}

fn node_id(text: &str) -> NodeId {
    NodeId::parse(text).expect("a node id")
}

#[test]
fn ready_set_lists_only_ready_other_members() {
    let (p, a, b, c) = (id('a'), id('b'), id('c'), id('d'));
    let s = status(
        &p,
        &p,
        &[
            (&p, true, Some(facts(Some(true)))),
            (&a, true, Some(facts(Some(true)))),
            (&b, true, Some(facts(Some(false)))),
            (&c, false, Some(facts(Some(true)))),
        ],
    );
    let got = ready_set(&s, 0);
    assert_eq!(got, BTreeSet::from([node_id(&a)]));
}

#[test]
fn unknown_facts_are_not_ready() {
    let (p, a) = (id('a'), id('b'));
    let s = status(
        &p,
        &p,
        &[(&p, true, Some(facts(Some(true)))), (&a, true, None)],
    );
    assert!(ready_set(&s, 0).is_empty());
}

#[test]
fn a_member_turning_ready_is_reported_once() {
    let (p, a) = (id('a'), id('b'));
    let off = status(
        &p,
        &p,
        &[
            (&p, true, Some(facts(Some(true)))),
            (&a, true, Some(facts(Some(false)))),
        ],
    );
    let on = status(
        &p,
        &p,
        &[
            (&p, true, Some(facts(Some(true)))),
            (&a, true, Some(facts(Some(true)))),
        ],
    );
    let mut watch = ReadyWatch::default();
    assert!(watch.newly_ready(&off, 0).is_empty());
    assert_eq!(watch.newly_ready(&on, 0), vec![node_id(&a)]);
    assert!(watch.newly_ready(&on, 0).is_empty());
    assert!(watch.newly_ready(&on, 0).is_empty());
    assert!(watch.newly_ready(&off, 0).is_empty());
    assert_eq!(watch.newly_ready(&on, 0), vec![node_id(&a)]);
}

#[test]
fn space_taken_by_a_larger_set_turns_a_member_not_ready() {
    let (p, a) = (id('a'), id('b'));
    let s = status(
        &p,
        &p,
        &[
            (&p, true, Some(facts(Some(true)))),
            (&a, true, Some(facts(Some(true)))),
        ],
    );
    let mut watch = ReadyWatch::default();
    assert_eq!(watch.newly_ready(&s, 10_000).len(), 1);
    assert!(watch.newly_ready(&s, 60_000).is_empty());
    assert_eq!(watch.newly_ready(&s, 10_000).len(), 1);
}

#[test]
fn a_machine_that_is_not_the_primary_reports_nothing_and_starts_fresh() {
    let (p, a) = (id('a'), id('b'));
    let members = [
        (p.as_str(), true, Some(facts(Some(true)))),
        (a.as_str(), true, Some(facts(Some(true)))),
    ];
    let as_standby = status(&a, &p, &members);
    let as_home = status(&p, &p, &members);
    let mut watch = ReadyWatch::default();
    assert_eq!(watch.newly_ready(&as_home, 0).len(), 1);
    assert!(watch.newly_ready(&as_standby, 0).is_empty());
    assert_eq!(watch.newly_ready(&as_home, 0).len(), 1);
}

#[test]
fn a_refused_ask_is_tried_again() {
    let (p, a) = (id('a'), id('b'));
    let s = status(
        &p,
        &p,
        &[
            (&p, true, Some(facts(Some(true)))),
            (&a, true, Some(facts(Some(true)))),
        ],
    );
    let mut watch = ReadyWatch::default();
    let fresh = watch.newly_ready(&s, 0);
    assert_eq!(fresh.len(), 1);
    watch.forget(&fresh[0]);
    assert_eq!(watch.newly_ready(&s, 0), fresh);
}

struct NoPeers;

impl SnapPeers for NoPeers {
    fn call(
        &self,
        _node: &NodeId,
        _op: &str,
        _args: Value,
    ) -> Result<Value, notchling_local_api::ApiError> {
        Err(notchling_local_api::ApiError::internal("no peers here"))
    }
}

fn lone_sender(dir: &TempDir) -> SnapSender {
    SnapSender::new(SenderParts {
        peers: Arc::new(NoPeers),
        facts: Arc::new(|| None),
        ready: Arc::new(|_| false),
        book: Arc::new(AckBook::new(dir.root())),
        retry: Duration::ZERO,
    })
}

#[derive(Default)]
struct Asks(Mutex<Vec<SnapAsk>>);

impl Hooks for Asks {
    fn snapshot(&self, ask: SnapAsk) {
        self.0.lock().unwrap().push(ask);
    }
}

fn set_host(node: &TestNode, host: Option<HostFacts>) {
    let mut v = versions(host);
    v.os = "linux".to_string();
    node.node
        .local(LocalCmd::SetVersions { versions: v })
        .unwrap();
}

#[test]
fn a_member_turning_ready_gets_a_ready_snapshot_ask() {
    let net = MemNet::new();
    let asks = Arc::new(Asks::default());
    let mut home = Opts::named("home");
    home.hooks = Some(asks.clone());
    let (a, b) = pair(&net, home, Opts::named("standby"));
    a.node
        .local(LocalCmd::SetMember {
            node: b.id(),
            can_be_master: Some(true),
            client: None,
            reference: None,
            accepts_delegation: None,
            headless: None,
            desired: None,
        })
        .unwrap();
    set_host(&a, Some(facts(Some(true))));
    set_host(&b, Some(facts(Some(false))));
    eventually("both report their facts", || {
        let s = a.node.status();
        member(&s, &b.id()).is_some_and(|m| m["versions"]["host"]["encrypted"] == json!(false))
            && member(&s, &a.id()).is_some_and(|m| m["versions"]["host"].is_object())
    });
    let dir = TempDir::new("ready");
    let sender = lone_sender(&dir);
    let mut watch = ReadyWatch::default();
    assert_eq!(watch_step(&a.node, &sender, &mut watch), 0);
    set_host(&b, Some(facts(Some(true))));
    eventually("b reports encryption on", || {
        member(&a.node.status(), &b.id())
            .is_some_and(|m| m["versions"]["host"]["encrypted"] == json!(true))
    });
    assert_eq!(watch_step(&a.node, &sender, &mut watch), 1);
    eventually("the hook was asked", || !asks.0.lock().unwrap().is_empty());
    assert_eq!(asks.0.lock().unwrap()[0].trigger, Trigger::Ready(b.id()));
    assert_eq!(watch_step(&a.node, &sender, &mut watch), 0);
    std::thread::sleep(Duration::from_millis(200));
    assert_eq!(asks.0.lock().unwrap().len(), 1, "an edge, not every tick");
}

fn make_can_be_master(a: &TestNode, b: &TestNode) {
    a.node
        .local(LocalCmd::SetMember {
            node: b.id(),
            can_be_master: Some(true),
            client: None,
            reference: None,
            accepts_delegation: None,
            headless: None,
            desired: None,
        })
        .unwrap();
}

fn last_set_of(node: &TestNode, slot: &SenderSlot) -> Arc<LastSet> {
    let holder = Arc::new(OnceLock::new());
    let _ = holder.set(node.node.clone());
    Arc::new(LastSet::new(holder, slot.clone(), node.cluster_dir()))
}

#[test]
fn the_predicate_follows_the_members_reported_facts() {
    let net = MemNet::new();
    let (a, b) = pair(&net, Opts::named("home"), Opts::named("standby"));
    make_can_be_master(&a, &b);
    let slot: SenderSlot = Arc::new(OnceLock::new());
    let ready = predicate(last_set_of(&a, &slot));
    assert!(!ready(&b.id()), "nothing reported yet");
    set_host(&a, Some(facts(Some(true))));
    set_host(&b, Some(facts(Some(true))));
    eventually("a member is ready", || ready(&b.id()));
    set_host(&b, Some(facts(Some(false))));
    eventually("a member is not ready", || !ready(&b.id()));
}

#[derive(Default)]
struct Counting(std::sync::atomic::AtomicUsize);

impl SnapPeers for Counting {
    fn call(
        &self,
        _node: &NodeId,
        _op: &str,
        _args: Value,
    ) -> Result<Value, notchling_local_api::ApiError> {
        self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Ok(json!({}))
    }
}

fn manifest(home: &NodeId, len: u64) -> Manifest {
    Manifest {
        set: "s-1".to_string(),
        seq: 1,
        term: Term {
            n: 1,
            node: home.clone(),
        },
        home: home.clone(),
        created_ms: 1,
        sessions: Vec::new(),
        parts: vec![Part {
            kind: PartKind::File,
            path: "/data/devices.json".into(),
            sha256: "0".repeat(64),
            len,
        }],
    }
}

#[test]
fn can_be_master_is_inert_until_ready() {
    let net = MemNet::new();
    let (a, b) = pair(&net, Opts::named("home"), Opts::named("standby"));
    make_can_be_master(&a, &b);
    set_host(&a, Some(facts(Some(true))));
    set_host(&b, Some(facts(Some(false))));
    eventually("b reports", || {
        member(&a.node.status(), &b.id())
            .is_some_and(|m| m["versions"]["host"]["encrypted"] == json!(false))
    });
    let dir = TempDir::new("inert");
    let slot: SenderSlot = Arc::new(OnceLock::new());
    let last = last_set_of(&a, &slot);
    let peers = Arc::new(Counting::default());
    let status_of = a.node.clone();
    let sender = SnapSender::new(SenderParts {
        peers: peers.clone(),
        facts: Arc::new(move || crate::snap_send::facts_from_status(&status_of.status())),
        ready: predicate(last),
        book: Arc::new(AckBook::new(dir.root())),
        retry: Duration::ZERO,
    });
    let set_dir = dir.root().join("s-1");
    assert!(sender
        .offer(manifest(&a.id(), 10), set_dir.clone())
        .is_empty());
    assert!(sender.ready_targets().is_empty());
    assert!(sender.targets().is_empty());
    assert!(sender.wait_idle(Duration::from_secs(5)));
    assert_eq!(peers.0.load(std::sync::atomic::Ordering::SeqCst), 0);
    set_host(&b, Some(facts(Some(true))));
    eventually("b is a target", || {
        sender.offer(manifest(&a.id(), 10), set_dir.clone()) == vec![b.id()]
    });
    assert!(sender.wait_idle(Duration::from_secs(5)));
}

#[test]
fn the_primarys_last_set_is_the_newest_offer_then_the_acked_lengths_then_zero() {
    let dir = TempDir::new("lastset");
    let book = Arc::new(AckBook::new(dir.root()));
    let sender = SnapSender::new(SenderParts {
        peers: Arc::new(NoPeers),
        facts: Arc::new(|| None),
        ready: Arc::new(|_| false),
        book: book.clone(),
        retry: Duration::ZERO,
    });
    assert_eq!(sender.last_set_bytes(), 0);
    let two = std::collections::BTreeMap::from([
        (std::path::PathBuf::from("/t/a.jsonl"), 3 * 1024 * 1024),
        (std::path::PathBuf::from("/t/b.jsonl"), 1024 * 1024),
    ]);
    book.record(&node_id(&id('b')), "s-1", two).unwrap();
    let one = std::collections::BTreeMap::from([(std::path::PathBuf::from("/t/a.jsonl"), 10)]);
    book.record(&node_id(&id('c')), "s-1", one).unwrap();
    assert_eq!(sender.last_set_bytes(), 4 * 1024 * 1024);
    assert_eq!(sender.last_set_mb(), 4);
    sender.offer(
        manifest(&node_id(&id('a')), 1_500_000),
        dir.root().join("s-1"),
    );
    assert_eq!(sender.last_set_bytes(), 1_500_000);
    assert_eq!(sender.last_set_mb(), 2);
}

fn write_current(cluster: &std::path::Path, home: &NodeId, m: &Manifest) {
    let root = cluster.join("standby").join(home.as_str());
    std::fs::create_dir_all(root.join("sets").join(&m.set)).unwrap();
    std::fs::write(root.join("current.json"), serde_json::to_vec(m).unwrap()).unwrap();
}

#[test]
fn a_standby_checks_space_against_its_own_current_set_not_zero() {
    let net = MemNet::new();
    let (a, b) = pair(&net, Opts::named("home"), Opts::named("standby"));
    make_can_be_master(&a, &b);
    let slot: SenderSlot = Arc::new(OnceLock::new());
    let on_standby = last_set_of(&b, &slot);
    eventually("b sees the primary", || {
        b.node.status()["cluster"]["home"] == json!(a.id())
    });
    assert_eq!(on_standby.mb(), 0);
    write_current(
        &b.cluster_dir(),
        &a.id(),
        &manifest(&a.id(), 7 * 1024 * 1024 + 1),
    );
    assert_eq!(on_standby.mb(), 8);
    assert_eq!(
        standby_set_bytes(&b.cluster_dir(), &a.id()),
        7 * 1024 * 1024 + 1
    );
    assert_eq!(standby_set_bytes(&b.cluster_dir(), &b.id()), 0);
    let on_primary = last_set_of(&a, &slot);
    assert_eq!(
        on_primary.mb(),
        0,
        "the primary reads its sender, not a store"
    );
    let dir = TempDir::new("primary-sender");
    let sender = Arc::new(lone_sender(&dir));
    sender.offer(manifest(&a.id(), 3 * 1024 * 1024), dir.root().join("s-1"));
    let _ = slot.set(sender);
    assert_eq!(on_primary.mb(), 3);
    assert_eq!(on_standby.mb(), 8, "a standby ignores any sender");
}

#[test]
fn a_standbys_status_fails_space_for_a_set_the_primary_would_refuse() {
    let net = MemNet::new();
    let (a, b) = pair(&net, Opts::named("home"), Opts::named("standby"));
    make_can_be_master(&a, &b);
    let mut tight = facts(Some(true));
    tight.free_mb = 10_000;
    set_host(&a, Some(facts(Some(true))));
    set_host(&b, Some(tight));
    eventually("b reports its space", || {
        member(&b.node.status(), &b.id())
            .is_some_and(|m| m["versions"]["host"]["free_mb"] == json!(10_000))
            && member(&b.node.status(), &a.id()).is_some_and(|m| m["versions"]["host"].is_object())
    });
    let slot: SenderSlot = Arc::new(OnceLock::new());
    let last = last_set_of(&b, &slot);
    let ready = predicate(last.clone());
    assert!(ready(&b.id()));
    write_current(
        &b.cluster_dir(),
        &a.id(),
        &manifest(&a.id(), 6_000 * 1024 * 1024),
    );
    assert!(!ready(&b.id()), "10 GB free is under twice a 6 GB set");
}
