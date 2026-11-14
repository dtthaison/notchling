use crate::acting_ops::Room;
use crate::cluster_node::{
    audit, wire_error, Answer, ClusterSetup, Hooks, In, JoinAsk, LocalCalls, NetSide, Parts,
    Shared, Submit,
};
use notchling_cluster_net::{Conn, Listener};
use notchling_engine::bridge::net::crypto::{hex, random_bytes, random_token};
use notchling_engine::cluster::core::LocalCmd;
use notchling_engine::cluster::join::{decode_token, encode_token, JoinToken};
use notchling_engine::cluster::persist;
use notchling_engine::cluster::wire::{decode, encode, Msg, Origin, OriginTier, WireError};
use notchling_engine::cluster::{Desired, Effect, Input, Member, MemberState, NodeCore, NodeId};
use notchling_local_api::AuditLog;
use notchling_platform::load::LoadSampler;
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender};
use std::sync::Arc;
use std::time::{Duration, Instant};

const LOAD_EVERY: Duration = Duration::from_secs(15);
const REMOTE_LOAD_EVERY: Duration = Duration::from_secs(30);
const VERSIONS_EVERY: Duration = Duration::from_secs(60);
const ADDRS_EVERY: Duration = Duration::from_secs(30);

enum Reply {
    Wait(Answer),
    Token(Answer, String),
    Load(NodeId),
}

pub(crate) struct Driver {
    setup: ClusterSetup,
    net: NetSide,
    me: NodeId,
    tx: Sender<In>,
    shared: Arc<Shared>,
    room: Arc<dyn Room>,
    local: Arc<dyn LocalCalls>,
    hooks: Arc<dyn Hooks>,
    audit: Arc<AuditLog>,
    core: NodeCore,
    boot_id: String,
    started: Instant,
    conns: BTreeMap<NodeId, Arc<dyn Conn>>,
    dialing: BTreeSet<NodeId>,
    listener: Option<Listener>,
    bound: Vec<String>,
    port: Option<u16>,
    link: bool,
    replies: BTreeMap<u64, Reply>,
    next_local: u64,
    loads: BTreeMap<NodeId, Value>,
    sampler: LoadSampler,
    last: BTreeMap<&'static str, Instant>,
    error: Option<String>,
    published: Value,
    tailscale: bool,
    later: Vec<(Reply, Result<Value, WireError>)>,
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

fn failed(why: impl Into<String>) -> WireError {
    WireError::new("failed", why)
}

impl Driver {
    pub(crate) fn new(
        setup: ClusterSetup,
        net: NetSide,
        me: NodeId,
        tx: Sender<In>,
        shared: Arc<Shared>,
        parts: Parts,
    ) -> Driver {
        let Parts {
            room,
            local,
            hooks,
            audit,
        } = parts;
        let boot_id = notchling_platform::clock::boot_id().unwrap_or_default();
        let mut error = None;
        let state = match persist::load(&setup.cluster_dir) {
            Ok(state) => state,
            Err(e) => {
                error = Some(format!("cluster.db: {e}"));
                None
            }
        };
        *crate::cluster_node::lock(&shared.rejoin) =
            state.as_ref().and_then(|s| s.rejoin_home(&me));
        let core = NodeCore::new(me.clone(), setup.versions.clone(), state, setup.client)
            .with_boot_id(boot_id.clone());
        Driver {
            setup,
            net,
            me,
            tx,
            shared,
            room,
            local,
            hooks,
            audit: Arc::new(audit),
            core,
            boot_id,
            started: Instant::now(),
            conns: BTreeMap::new(),
            dialing: BTreeSet::new(),
            listener: None,
            bound: Vec::new(),
            port: None,
            link: false,
            replies: BTreeMap::new(),
            next_local: 1,
            loads: BTreeMap::new(),
            sampler: LoadSampler::new(),
            last: BTreeMap::new(),
            error,
            published: Value::Null,
            tailscale: false,
            later: Vec::new(),
        }
    }

    pub(crate) fn run(&mut self, rx: Receiver<In>) {
        self.tick();
        let mut next = Instant::now() + self.setup.tick;
        loop {
            let wait = next.saturating_duration_since(Instant::now());
            match rx.recv_timeout(wait) {
                Ok(In::Stop) | Err(RecvTimeoutError::Disconnected) => break,
                Ok(input) => self.handle(input),
                Err(RecvTimeoutError::Timeout) => {}
            }
            if Instant::now() >= next {
                self.tick();
                next = Instant::now() + self.setup.tick;
            }
        }
        self.listener = None;
        for (_, conn) in std::mem::take(&mut self.conns) {
            conn.close();
        }
        for (_, reply) in std::mem::take(&mut self.replies) {
            answer(reply, Err(failed("the cluster node has stopped")));
        }
    }

    fn net_allowed(&self) -> bool {
        !self.setup.fixture || self.setup.loopback_ok
    }

    fn boot_ms(&self) -> u64 {
        notchling_platform::clock::boot_ms()
            .unwrap_or_else(|_| self.started.elapsed().as_millis() as u64)
    }

    fn due(&mut self, what: &'static str, every: Duration) -> bool {
        let now = Instant::now();
        match self.last.get(what) {
            Some(at) if now.duration_since(*at) < every => false,
            _ => {
                self.last.insert(what, now);
                true
            }
        }
    }

    fn tick(&mut self) {
        let input = Input::Tick {
            boot_ms: self.boot_ms(),
            wall_ms: now_ms(),
        };
        self.step(input);
        if self.due("load", LOAD_EVERY) {
            if let Ok(load) = self.sampler.sample() {
                let value = json!({
                    "cpu": load.cpu_pct,
                    "memUsedMb": load.mem_used_mb,
                    "memTotalMb": load.mem_total_mb,
                });
                self.loads.insert(self.me.clone(), value);
            }
        }
        if self.link && self.due("remote-load", REMOTE_LOAD_EVERY) {
            self.ask_loads();
        }
        if self.due("versions", VERSIONS_EVERY) {
            self.reprobe();
        }
        if self.due("addrs", ADDRS_EVERY) {
            self.tailscale = !notchling_platform::net_if::tailscale_addrs().is_empty();
            if self.link && self.bound != self.net.transport.addrs() {
                self.listen();
            }
        }
        self.after();
        self.flush_answers();
    }

    fn flush_answers(&mut self) {
        for (reply, result) in std::mem::take(&mut self.later) {
            answer(reply, result);
        }
    }

    fn reprobe(&mut self) {
        let Some(probe) = self.setup.probe.as_mut() else {
            return;
        };
        let versions = probe();
        if versions != self.setup.versions {
            self.setup.versions = versions.clone();
            self.local_cmd(LocalCmd::SetVersions { versions }, None);
        }
    }

    fn ask_loads(&mut self) {
        let online: Vec<NodeId> = self.core.online().into_iter().collect();
        for node in online {
            let origin = Origin {
                node: self.me.clone(),
                tier: OriginTier::Node,
            };
            let id = self.reply_id(Reply::Load(node.clone()));
            self.step(Input::Local {
                id,
                cmd: LocalCmd::Forward {
                    node,
                    op: "node.load".to_string(),
                    args: json!({}),
                    origin,
                },
            });
        }
    }

    fn reply_id(&mut self, reply: Reply) -> u64 {
        let id = self.next_local;
        self.next_local += 1;
        self.replies.insert(id, reply);
        id
    }

    fn local_cmd(&mut self, cmd: LocalCmd, answer: Option<Answer>) {
        let id = match answer {
            Some(answer) => self.reply_id(Reply::Wait(answer)),
            None => {
                let id = self.next_local;
                self.next_local += 1;
                id
            }
        };
        self.step(Input::Local { id, cmd });
    }

    fn handle(&mut self, input: In) {
        match input {
            In::Opened(conn) => self.opened(conn),
            In::Line(fp, line) => self.line(&fp, &line),
            In::Closed(fp, id) => self.closed(&fp, id),
            In::DialDone(node) => {
                self.dialing.remove(&node);
            }
            In::Local(cmd, answer) => self.local_cmd(cmd, answer),
            In::Create(name, answer) => self.create(name, answer),
            In::Token(answer) => self.token(answer),
            In::Join(token, headless, answer) => self.join(&token, headless, answer),
            In::Core(input) => self.step(input),
            In::Link(on) => self.set_link(on),
            In::Stop => {}
        }
        self.after();
        self.flush_answers();
    }

    fn peer_of(fp: &str) -> Option<NodeId> {
        NodeId::from_fingerprint(fp)
    }

    fn line(&mut self, fp: &str, line: &[u8]) {
        let Some(from) = Driver::peer_of(fp) else {
            return;
        };
        let Ok(msg) = decode(line) else {
            return;
        };
        let from_home = self.core.state().is_some_and(|s| s.home == from);
        let says_desired = from_home && matches!(msg, Msg::Members { .. } | Msg::HelloOk { .. });
        self.step(Input::Msg { from, msg });
        if says_desired {
            self.tell_desired();
        }
    }

    fn tell_desired(&self) {
        let desired = self
            .core
            .state()
            .and_then(|s| s.members.get(&self.me))
            .filter(|m| m.state == MemberState::Active)
            .map(|m| m.desired);
        if let Some(desired) = desired {
            let hooks = self.hooks.clone();
            let _ = std::thread::Builder::new()
                .name("cluster-desired".into())
                .spawn(move || hooks.desired(desired));
        }
    }

    fn me_member(&self, headless: bool) -> Member {
        Member {
            id: self.me.clone(),
            name: self.setup.name.clone(),
            fingerprint: self.net.fingerprint.clone(),
            addrs: self.net.transport.addrs(),
            port: self.port.unwrap_or(self.setup.port),
            client: self.setup.client,
            can_be_master: false,
            reference: false,
            accepts_delegation: false,
            headless,
            state: MemberState::Active,
            versions: Some(self.setup.versions.clone()),
            desired: Desired::default(),
            joined_ms: now_ms(),
        }
    }

    fn create(&mut self, name: String, answer: Answer) {
        let id = match random_bytes(16) {
            Ok(bytes) => hex(&bytes),
            Err(e) => {
                let _ = answer.send(Err(failed(e.to_string())));
                return;
            }
        };
        let mut me = self.me_member(false);
        me.reference = true;
        self.local_cmd(LocalCmd::Create { id, name, me }, Some(answer));
    }

    fn token(&mut self, answer: Answer) {
        let Some(state) = self.core.state() else {
            let _ = answer.send(Err(WireError::new(
                "no_cluster",
                "this node is in no cluster",
            )));
            return;
        };
        if !self.core.is_home() {
            let (home, origin) = (
                state.home.clone(),
                Origin {
                    node: self.me.clone(),
                    tier: OriginTier::Agent,
                },
            );
            let cmd = LocalCmd::Forward {
                node: home,
                op: "cluster.join.token".to_string(),
                args: json!({}),
                origin,
            };
            self.local_cmd(cmd, Some(answer));
            return;
        }
        let secret = match random_token(32) {
            Ok(secret) => secret,
            Err(e) => {
                let _ = answer.send(Err(failed(e.to_string())));
                return;
            }
        };
        let token = encode_token(&JoinToken {
            cluster: state.id.clone(),
            home: self.me.clone(),
            fingerprint: self.net.fingerprint.clone(),
            addrs: self.net.transport.addrs(),
            port: self.port.unwrap_or(self.setup.port),
            secret: secret.clone(),
        });
        let id = self.reply_id(Reply::Token(answer, token));
        self.step(Input::Local {
            id,
            cmd: LocalCmd::IssueToken { secret },
        });
    }

    fn join(&mut self, token: &str, headless: bool, answer: Answer) {
        let token = match decode_token(token) {
            Ok(token) => token,
            Err(e) => {
                let _ = answer.send(Err(WireError::new("bad_args", e)));
                return;
            }
        };
        if !self.link || !self.net_allowed() {
            let _ = answer.send(Err(WireError::new(
                "forbidden",
                "this node's cluster links are off",
            )));
            return;
        }
        let me = self.me_member(headless);
        self.local_cmd(LocalCmd::JoinWith { token, me }, Some(answer));
    }

    fn step(&mut self, input: Input) {
        let effects = self.core.step(input);
        let mut rest = effects.into_iter();
        while let Some(effect) = rest.next() {
            if let Effect::Persist { entry, changes } = &effect {
                if let Err(why) = persist::save(&self.setup.cluster_dir, entry, changes) {
                    self.save_failed(why, rest.collect());
                    return;
                }
                continue;
            }
            self.effect(effect);
        }
    }

    fn save_failed(&mut self, why: String, rest: Vec<Effect>) {
        audit(
            &self.audit,
            "cluster:save",
            None,
            "failed",
            Some(why.clone()),
        );
        self.error = Some(format!("saving the cluster failed: {why}"));
        let err = failed(format!("saving the cluster failed: {why}"));
        for effect in rest {
            if let Effect::Answer { local, .. } = effect {
                if let Some(reply) = self.replies.remove(&local) {
                    self.later.push((reply, Err(err.clone())));
                }
            }
        }
        for (_, reply) in std::mem::take(&mut self.replies) {
            self.later.push((reply, Err(err.clone())));
        }
        let state = persist::load(&self.setup.cluster_dir).ok().flatten();
        self.core = NodeCore::new(
            self.me.clone(),
            self.setup.versions.clone(),
            state,
            self.setup.client,
        )
        .with_boot_id(self.boot_id.clone());
        self.step(Input::Tick {
            boot_ms: self.boot_ms(),
            wall_ms: now_ms(),
        });
        let peers: Vec<NodeId> = self.conns.keys().cloned().collect();
        for peer in peers {
            self.step(Input::Linked { peer });
        }
    }

    fn effect(&mut self, effect: Effect) {
        match effect {
            Effect::Send { to, msg } => {
                if let (Some(conn), Ok(line)) = (self.conns.get(&to), encode(&msg)) {
                    let _ = conn.send(line);
                }
            }
            Effect::Dial {
                to,
                addrs,
                port,
                fingerprint,
            } => self.dial(to, addrs, port, fingerprint),
            Effect::Close { peer } => {
                if let Some(conn) = self.conns.remove(&peer) {
                    conn.close();
                }
            }
            Effect::Pins {
                fingerprints,
                join_open,
            } => {
                self.net.pins.set(fingerprints);
                self.net.pins.set_join_open(join_open);
            }
            Effect::Persist { .. } => {}
            Effect::Publish { topic, name, data } => self.publish(&topic, &name, &data),
            Effect::RunLocal {
                call,
                op,
                args,
                origin,
                ..
            } => self.run_local(call, op, args, origin),
            Effect::Answer { local, result } => {
                if let Some(reply) = self.replies.remove(&local) {
                    self.answered(reply, result);
                }
            }
            Effect::JoinRequest {
                item,
                node,
                name,
                os,
                arch,
                addr,
                can_be_master,
                headless,
            } => {
                let ask = JoinAsk {
                    item,
                    node,
                    name,
                    os,
                    arch,
                    addr,
                    can_be_master,
                    headless,
                };
                let submit = Submit::new(self.tx.clone());
                let hooks = self.hooks.clone();
                let _ = std::thread::Builder::new()
                    .name("cluster-join-ask".into())
                    .spawn(move || hooks.join_request(ask, submit));
            }
            Effect::SteppedDown { term, home } => audit(
                &self.audit,
                "cluster:stepped_down",
                Some(home.as_str()),
                "ok",
                Some(term.to_string()),
            ),
            Effect::Wipe => self.wipe(),
        }
    }

    fn answered(&mut self, reply: Reply, result: Result<Value, WireError>) {
        match reply {
            Reply::Load(node) => {
                if let Ok(value) = result {
                    self.loads.insert(node, value);
                }
            }
            other => self.later.push((other, result)),
        }
    }

    fn run_local(&mut self, call: u64, op: String, args: Value, origin: Origin) {
        if op == "node.load" {
            let result = Ok(self.loads.get(&self.me).cloned().unwrap_or(Value::Null));
            let _ = self.tx.send(In::Core(Input::LocalDone { call, result }));
            return;
        }
        let (local, tx, log) = (self.local.clone(), self.tx.clone(), self.audit.clone());
        let spawned = std::thread::Builder::new()
            .name("cluster-call".into())
            .spawn(move || {
                let result = local.call(&op, &args, &origin).map_err(wire_error);
                let outcome = if result.is_ok() { "ok" } else { "failed" };
                audit(
                    &log,
                    &format!("cluster:{op}"),
                    Some(origin.node.as_str()),
                    outcome,
                    None,
                );
                let _ = tx.send(In::Core(Input::LocalDone { call, result }));
            });
        if spawned.is_err() {
            let result = Err(failed("no thread for the call"));
            let _ = self.tx.send(In::Core(Input::LocalDone { call, result }));
        }
    }
}

fn answer(reply: Reply, result: Result<Value, WireError>) {
    match reply {
        Reply::Wait(tx) => {
            let _ = tx.send(result);
        }
        Reply::Token(tx, token) => {
            let _ = tx.send(result.map(|_| json!({"token": token})));
        }
        Reply::Load(_) => {}
    }
}

#[path = "cluster_driver_links.rs"]
mod links;
#[path = "cluster_driver_status.rs"]
mod status;
