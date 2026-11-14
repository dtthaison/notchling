use crate::acting_ops_tests::FakeRoom;
use crate::cluster_node::{
    ClusterLog, ClusterNode, ClusterSetup, Hooks, LocalCalls, NetSide, NoHooks,
};
use crate::cluster_versions::own;
use crate::event_assert::text;
use crate::late_emit::Events;
use notchling_cluster_net::{
    fingerprint, Conn, Error, LinkEvents, Listener, MemNet, MemTransport, Pins, Transport,
};
use notchling_engine::cluster::wire::Origin;
use notchling_engine::cluster::NodeId;
use notchling_events::{MemorySink, Salt};
use notchling_local_api::ApiError;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

static NEXT: AtomicU64 = AtomicU64::new(0);

pub(crate) struct Dir(PathBuf);

impl Dir {
    pub(crate) fn new(tag: &str) -> Dir {
        let n = NEXT.fetch_add(1, Ordering::SeqCst);
        let path = PathBuf::from(format!("/tmp/nlc-{tag}-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).unwrap();
        Dir(path)
    }

    pub(crate) fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[derive(Default)]
pub(crate) struct FakeCalls {
    pub calls: Mutex<Vec<(String, Value, Origin)>>,
}

impl LocalCalls for FakeCalls {
    fn call(&self, op: &str, args: &Value, origin: &Origin) -> Result<Value, ApiError> {
        self.calls
            .lock()
            .unwrap()
            .push((op.to_string(), args.clone(), origin.clone()));
        Ok(json!({"session": format!("s-{op}")}))
    }
}

pub(crate) struct Counting {
    pub inner: MemTransport,
    pub listens: AtomicUsize,
    pub dials: AtomicUsize,
}

impl Transport for Counting {
    fn addrs(&self) -> Vec<String> {
        self.inner.addrs()
    }

    fn dial(
        &self,
        addrs: &[String],
        port: u16,
        expect: &str,
        events: Arc<dyn LinkEvents>,
    ) -> Result<Arc<dyn Conn>, Error> {
        self.dials.fetch_add(1, Ordering::SeqCst);
        self.inner.dial(addrs, port, expect, events)
    }

    fn listen(&self, port: u16, events: Arc<dyn LinkEvents>) -> Result<Listener, Error> {
        self.listens.fetch_add(1, Ordering::SeqCst);
        self.inner.listen(port, events)
    }
}

pub(crate) struct TestNode {
    pub node: Arc<ClusterNode>,
    pub fp: String,
    pub room: Arc<FakeRoom>,
    pub calls: Arc<FakeCalls>,
    pub transport: Arc<Counting>,
    pub sink: Arc<MemorySink>,
    pub log: ClusterLog,
    pub dir: Dir,
}

pub(crate) fn test_log() -> (Arc<MemorySink>, ClusterLog) {
    let sink = Arc::new(MemorySink::new());
    let log = ClusterLog::new(Events::fixed(sink.clone(), Salt([0; 32])));
    (sink, log)
}

pub(crate) fn events_text(sink: &MemorySink) -> String {
    let field = |event: &notchling_events::Event, name: &str| {
        text(event, name).unwrap_or_else(|| "-".to_string())
    };
    sink.events()
        .iter()
        .map(|e| {
            format!(
                "{} step={} switch={} on={} cause={} peer={} op={} {}\n",
                e.spec.name,
                field(e, "step"),
                field(e, "switch"),
                field(e, "on"),
                field(e, "cause"),
                field(e, "peer"),
                field(e, "op"),
                e.outcome.name()
            )
        })
        .collect()
}

impl TestNode {
    pub(crate) fn id(&self) -> NodeId {
        self.node.me().clone()
    }

    pub(crate) fn cluster_dir(&self) -> PathBuf {
        self.dir.path().join("cluster")
    }

    pub(crate) fn audit_text(&self) -> String {
        events_text(&self.sink)
    }
}

impl Drop for TestNode {
    fn drop(&mut self) {
        self.node.close();
    }
}

pub(crate) struct Opts {
    pub name: &'static str,
    pub os: &'static str,
    pub client: bool,
    pub fixture: bool,
    pub cluster_dir: Option<PathBuf>,
    pub hooks: Option<Arc<dyn Hooks>>,
    pub local: Option<Arc<dyn LocalCalls>>,
    pub fingerprint: Option<String>,
    pub headless: bool,
    pub room: Option<Arc<FakeRoom>>,
}

impl Opts {
    pub(crate) fn named(name: &'static str) -> Opts {
        Opts {
            name,
            os: "linux",
            client: false,
            fixture: false,
            cluster_dir: None,
            hooks: None,
            local: None,
            fingerprint: None,
            headless: false,
            room: None,
        }
    }
}

pub(crate) fn node(net: &Arc<MemNet>, opts: Opts) -> TestNode {
    let dir = Dir::new(opts.name);
    let fp = opts.fingerprint.clone().unwrap_or_else(|| {
        fingerprint(format!("{}-{}", opts.name, dir.path().display()).as_bytes())
    });
    let pins = Pins::default();
    let transport = Arc::new(Counting {
        inner: MemTransport::on(net, &fp, pins.clone()),
        listens: AtomicUsize::new(0),
        dials: AtomicUsize::new(0),
    });
    let room = opts.room.clone().unwrap_or_default();
    let calls = Arc::new(FakeCalls::default());
    let mut versions = own();
    versions.os = opts.os.to_string();
    let setup = ClusterSetup {
        cluster_dir: opts
            .cluster_dir
            .unwrap_or_else(|| dir.path().join("cluster")),
        fixture: opts.fixture,
        loopback_ok: false,
        port: 47823,
        versions,
        client: opts.client,
        name: opts.name.to_string(),
        tick: Duration::from_millis(50),
        probe: None,
    };
    let (sink, log) = test_log();
    let node = ClusterNode::start(
        setup,
        NetSide {
            transport: transport.clone(),
            pins,
            fingerprint: fp.clone(),
        },
        room.clone(),
        opts.local.unwrap_or_else(|| calls.clone()),
        opts.hooks.unwrap_or_else(|| Arc::new(NoHooks)),
        log.clone(),
    )
    .unwrap();
    TestNode {
        node,
        fp,
        room,
        calls,
        transport,
        sink,
        log,
        dir,
    }
}

pub(crate) fn eventually(what: &str, mut check: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        if check() {
            return;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    panic!("never happened: {what}");
}

pub(crate) fn member<'a>(status: &'a Value, id: &NodeId) -> Option<&'a Value> {
    status["members"]
        .as_array()?
        .iter()
        .find(|m| m["id"] == json!(id))
}

pub(crate) fn online(status: &Value, id: &NodeId) -> bool {
    member(status, id).is_some_and(|m| m["online"] == json!(true) && m["state"] == "active")
}

pub(crate) fn pair(net: &Arc<MemNet>, home: Opts, worker: Opts) -> (TestNode, TestNode) {
    let headless = worker.headless;
    let a = node(net, home);
    let b = node(net, worker);
    a.node.set_link(true);
    b.node.set_link(true);
    a.node.create("home").unwrap();
    let token = a.node.join_token().unwrap()["token"]
        .as_str()
        .unwrap()
        .to_string();
    let joined = b.node.join(&token, headless).unwrap();
    assert_eq!(joined["state"], "pending", "{joined}");
    a.node
        .local(notchling_engine::cluster::LocalCmd::Approve {
            node: b.id(),
            client: false,
            can_be_master: false,
        })
        .unwrap();
    eventually("both see both online", || {
        online(&a.node.status(), &b.id()) && online(&b.node.status(), &a.id())
    });
    (a, b)
}
