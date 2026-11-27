use crate::cluster_blobs::{Blobs, NODE_PUT, NODE_PUT_END};
use crate::limits_ops_tests::TempDir;
use crate::snap_files::{stage_set, StageMeta};
use crate::snap_files_tests::{meta, node, plan, put};
use crate::snap_recv::SnapNode;
use crate::snap_send::{Candidate, Facts, SenderParts, SnapPeers, SnapSender};
use notchling_engine::cluster::ack_book::AckBook;
use notchling_engine::cluster::snapshot::Manifest;
use notchling_engine::cluster::standby_store::StandbyStore;
use notchling_engine::cluster::wire::{Origin, OriginTier};
use notchling_engine::cluster::NodeId;
use notchling_local_api::{ApiError, ErrorCode};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::Duration;

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

pub(crate) fn id_of(n: u8) -> NodeId {
    NodeId::from_fingerprint(&format!("{n:02x}").repeat(32)).unwrap()
}

#[derive(Clone)]
pub(crate) struct Entry {
    pub(crate) id: NodeId,
    pub(crate) can_be_master: bool,
    pub(crate) online: bool,
}

#[derive(Default)]
struct Gate {
    held: bool,
    entered: usize,
}

pub(crate) struct Net {
    pub(crate) home: Mutex<NodeId>,
    pub(crate) members: Mutex<Vec<Entry>>,
    pub(crate) ready: Mutex<BTreeSet<NodeId>>,
    nodes: Mutex<BTreeMap<NodeId, Arc<Member>>>,
    pub(crate) log: Mutex<Vec<(NodeId, String)>>,
    gate: (Mutex<Gate>, Condvar),
    cut_on: Mutex<Option<(NodeId, String, usize)>>,
}

pub(crate) struct Member {
    pub(crate) cluster: PathBuf,
    pub(crate) blobs: Arc<Blobs>,
    pub(crate) node: Arc<SnapNode>,
    pub(crate) book: Arc<AckBook>,
}

pub(crate) struct NetPeers {
    from: NodeId,
    net: Arc<Net>,
}

impl Net {
    pub(crate) fn facts_of(&self, me: &NodeId) -> Option<Facts> {
        Some(Facts {
            me: me.clone(),
            home: Some(lock(&self.home).clone()),
            members: lock(&self.members)
                .iter()
                .map(|e| Candidate {
                    id: e.id.clone(),
                    can_be_master: e.can_be_master,
                    online: e.online,
                    active: true,
                })
                .collect(),
        })
    }

    pub(crate) fn member(&self, id: &NodeId) -> Arc<Member> {
        lock(&self.nodes).get(id).cloned().unwrap()
    }

    pub(crate) fn set_online(&self, id: &NodeId, online: bool) {
        for e in lock(&self.members).iter_mut() {
            if &e.id == id {
                e.online = online;
            }
        }
    }

    pub(crate) fn hold(&self) {
        lock(&self.gate.0).held = true;
    }

    pub(crate) fn release(&self) {
        lock(&self.gate.0).held = false;
        self.gate.1.notify_all();
    }

    pub(crate) fn wait_entered(&self, n: usize) {
        let mut gate = lock(&self.gate.0);
        while gate.entered < n {
            gate = self
                .gate
                .1
                .wait_timeout(gate, Duration::from_secs(10))
                .unwrap()
                .0;
        }
    }

    pub(crate) fn cut(&self, node: &NodeId, op: &str, after: usize) {
        *lock(&self.cut_on) = Some((node.clone(), op.to_string(), after));
    }

    pub(crate) fn mend(&self) {
        *lock(&self.cut_on) = None;
    }

    pub(crate) fn calls(&self, node: &NodeId, op: &str) -> usize {
        lock(&self.log)
            .iter()
            .filter(|(n, o)| n == node && o == op)
            .count()
    }

    fn cut_now(&self, node: &NodeId, op: &str) -> bool {
        let mut cut = lock(&self.cut_on);
        let Some((target, name, after)) = cut.as_mut() else {
            return false;
        };
        if target != node || name != op {
            return false;
        }
        if *after == 0 {
            return true;
        }
        *after -= 1;
        false
    }
}

impl SnapPeers for NetPeers {
    fn call(&self, node: &NodeId, op: &str, args: Value) -> Result<Value, ApiError> {
        let net = &self.net;
        let online = lock(&net.members)
            .iter()
            .find(|e| &e.id == node)
            .is_none_or(|e| e.online);
        if !online {
            return Err(ApiError::new(ErrorCode::Failed, "that machine is offline"));
        }
        lock(&net.log).push((node.clone(), op.to_string()));
        if op == "snap.offer" {
            let mut gate = lock(&net.gate.0);
            gate.entered += 1;
            net.gate.1.notify_all();
            while gate.held {
                gate = net
                    .gate
                    .1
                    .wait_timeout(gate, Duration::from_millis(50))
                    .unwrap()
                    .0;
            }
        }
        if net.cut_now(node, op) {
            return Err(ApiError::new(ErrorCode::Failed, "the link was cut"));
        }
        let member = net.member(node);
        let origin = Origin {
            node: self.from.clone(),
            tier: OriginTier::Node,
        };
        match op {
            NODE_PUT => member.blobs.put(&args),
            NODE_PUT_END => member.blobs.end(&args),
            _ => member
                .node
                .call(op, &args, &origin)
                .unwrap_or_else(|| Err(ApiError::new(ErrorCode::UnknownOp, "no such op"))),
        }
    }
}

pub(crate) struct Fixture {
    pub(crate) t: TempDir,
    pub(crate) net: Arc<Net>,
    pub(crate) primary: NodeId,
    pub(crate) sender: SnapSender,
    pub(crate) book: Arc<AckBook>,
}

impl Fixture {
    pub(crate) fn new(tag: &str, others: &[(u8, bool)]) -> Fixture {
        let t = TempDir::new(tag);
        let primary = node();
        let mut entries = vec![Entry {
            id: primary.clone(),
            can_be_master: true,
            online: true,
        }];
        for (n, can) in others {
            entries.push(Entry {
                id: id_of(*n),
                can_be_master: *can,
                online: true,
            });
        }
        let net = Arc::new(Net {
            home: Mutex::new(primary.clone()),
            members: Mutex::new(entries),
            ready: Mutex::new(BTreeSet::new()),
            nodes: Mutex::new(BTreeMap::new()),
            log: Mutex::new(Vec::new()),
            gate: (Mutex::new(Gate::default()), Condvar::new()),
            cut_on: Mutex::new(None),
        });
        let all: Vec<NodeId> = lock(&net.members).iter().map(|e| e.id.clone()).collect();
        for id in all {
            let cluster = t.path(&format!("nodes/{}/cluster", id.as_str()));
            std::fs::create_dir_all(&cluster).unwrap();
            let blobs = Blobs::new(cluster.clone());
            let book = Arc::new(AckBook::new(&cluster));
            let facts_net = net.clone();
            let me = id.clone();
            let node = SnapNode::new(
                cluster.clone(),
                blobs.clone(),
                Arc::new(NetPeers {
                    from: id.clone(),
                    net: net.clone(),
                }),
                Arc::new(move || facts_net.facts_of(&me)),
                book.clone(),
            );
            lock(&net.nodes).insert(
                id.clone(),
                Arc::new(Member {
                    cluster,
                    blobs,
                    node,
                    book,
                }),
            );
        }
        let book = net.member(&primary).book.clone();
        let facts_net = net.clone();
        let ready_net = net.clone();
        let me = primary.clone();
        let sender = SnapSender::new(SenderParts {
            peers: Arc::new(NetPeers {
                from: primary.clone(),
                net: net.clone(),
            }),
            facts: Arc::new(move || facts_net.facts_of(&me)),
            ready: Arc::new(move |id| lock(&ready_net.ready).contains(id)),
            book: book.clone(),
            retry: Duration::ZERO,
        });
        Fixture {
            t,
            net,
            primary,
            sender,
            book,
        }
    }

    pub(crate) fn rebuild_sender(&self) -> SnapSender {
        self.sender_with_retry(Duration::ZERO)
    }

    pub(crate) fn sender_with_retry(&self, retry: Duration) -> SnapSender {
        let facts_net = self.net.clone();
        let ready_net = self.net.clone();
        let me = self.primary.clone();
        SnapSender::new(SenderParts {
            peers: Arc::new(NetPeers {
                from: self.primary.clone(),
                net: self.net.clone(),
            }),
            facts: Arc::new(move || facts_net.facts_of(&me)),
            ready: Arc::new(move |id| lock(&ready_net.ready).contains(id)),
            book: self.book.clone(),
            retry,
        })
    }

    pub(crate) fn make_ready(&self, ids: &[NodeId]) {
        lock(&self.net.ready).extend(ids.iter().cloned());
    }

    pub(crate) fn transcript(&self) -> PathBuf {
        self.t.path("home/.claude/projects/p/s1.jsonl")
    }

    pub(crate) fn stage(&self, seq: u64, tail_from: u64, body: &[u8]) -> (Manifest, PathBuf) {
        put(&self.transcript(), body);
        put(
            &self.t.path("home/data/devices.json"),
            format!("{{\"seq\":{seq}}}").as_bytes(),
        );
        put(&self.t.path("home/data/pages/a.html"), b"<p>a</p>");
        put(&self.t.path("home/data/pages/img/b.bin"), &[7; 5000]);
        let mut p = plan(&format!("s-{seq}"));
        p.tails = vec![(self.transcript(), tail_from)];
        p.files = vec![
            self.t.path("home/data/devices.json"),
            self.t.path("home/data/pages"),
        ];
        let mut m: StageMeta = meta();
        m.seq = seq;
        m.home = self.primary.clone();
        let stage = self.t.path("home/data/cluster/outgoing");
        let manifest = stage_set(&p, &stage, &m).unwrap();
        let dir = stage.join(&manifest.set);
        (manifest, dir)
    }

    pub(crate) fn store_of(&self, id: &NodeId) -> StandbyStore {
        StandbyStore::new(&self.net.member(id).cluster, &self.primary)
    }
}
