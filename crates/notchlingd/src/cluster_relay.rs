use crate::bridge_backend::answer_json;
use notchling_engine::bridge::model::{PendingPrompt, PromptKind, Session};
use notchling_engine::bridge::relay::{
    Call, Cancelled, HeldAct, HeldList, HeldNoticeView, HeldRef, HeldView, Relay, Routed,
};
use notchling_engine::bridge::BridgeFailure;
use notchling_engine::cluster::held::{Found, Held, HeldQueue, HeldTarget};
use notchling_engine::cluster::persist;
use notchling_engine::cluster::wire::WireError;
use notchling_engine::cluster::{NodeId, Qualified};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, Weak};
use std::time::Duration;

pub const SEND_WAIT: Duration = Duration::from_secs(10);
pub const NOTICES_KEPT: usize = 20;
pub const NOTICES_PER_DEVICE: usize = 10;
pub const STAY_FIRST_MS: i64 = 15_000;
pub const STAY_CAP_MS: i64 = 300_000;

const UNREACHABLE: [&str; 3] = ["offline", "no_cluster", "not_sent"];

pub trait Wire: Send + Sync {
    fn status(&self) -> Value;
    fn call(
        &self,
        to: &NodeId,
        op: &str,
        args: Value,
        within: Duration,
    ) -> Result<Value, WireError>;
}

pub trait SetView: Send + Sync {
    fn newest_ms(&self) -> Option<i64>;
    fn transcript(&self, original: &Path) -> Option<PathBuf>;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Served {
    pub port: u16,
    pub fingerprint: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    pub total: usize,
    pub device: usize,
}

impl Limits {
    pub const DEFAULT: Limits = Limits {
        total: 200,
        device: 50,
    };
}

pub type Clock = Arc<dyn Fn() -> i64 + Send + Sync>;
pub type PromptFinder = Arc<dyn Fn(&str) -> Option<PendingPrompt> + Send + Sync>;
pub type ModelFinder = Arc<dyn Fn(&str) -> Option<String> + Send + Sync>;

pub struct Parts {
    pub wire: Arc<dyn Wire>,
    pub dir: PathBuf,
    pub clock: Clock,
    pub prompts: PromptFinder,
    pub model: ModelFinder,
    pub rows: Arc<dyn Fn() -> Vec<String> + Send + Sync>,
    pub set: Arc<dyn SetView>,
    pub bridge: Arc<dyn Fn() -> Served + Send + Sync>,
    pub limits: Limits,
}

#[derive(Default)]
struct Work {
    wanted: bool,
    busy: bool,
    stop: bool,
}

#[derive(Default)]
struct Signal {
    state: Mutex<Work>,
    changed: Condvar,
}

impl Signal {
    fn wake(&self) {
        lock(&self.state).wanted = true;
        self.changed.notify_all();
    }
}

pub struct HeldRelay {
    parts: Parts,
    queue: Mutex<HeldQueue>,
    flight: Mutex<Vec<Held>>,
    notices: Mutex<Vec<HeldNoticeView>>,
    stays: Mutex<BTreeMap<String, Stay>>,
    routing: Mutex<()>,
    signal: Arc<Signal>,
    flushing: AtomicBool,
    next: AtomicU64,
}

struct Stay {
    tries: u32,
    retry_ms: i64,
}

struct Ctx {
    me: NodeId,
    primary: NodeId,
    primary_name: String,
    primary_online: bool,
    standby: bool,
    term: u64,
    members: Vec<Value>,
}

impl Ctx {
    fn of(status: &Value) -> Option<Ctx> {
        let cluster = status.get("cluster").filter(|c| !c.is_null())?;
        let me = NodeId::parse(status.get("me")?.as_str()?)?;
        let primary = NodeId::parse(cluster.get("home")?.as_str()?)?;
        let members: Vec<Value> = status.get("members")?.as_array()?.clone();
        let find = |id: &NodeId| {
            members
                .iter()
                .find(|m| m.get("id").and_then(Value::as_str) == Some(id.as_str()))
        };
        let primary_member = find(&primary);
        Some(Ctx {
            primary_name: primary_member
                .and_then(|m| m.get("name")?.as_str())
                .unwrap_or("the primary")
                .to_string(),
            primary_online: primary == me
                || primary_member
                    .and_then(|m| m.get("online")?.as_bool())
                    .unwrap_or(false),
            standby: me != primary
                && find(&me).and_then(|m| m.get("role")?.as_str()) == Some("standby"),
            term: cluster["term"]["n"].as_u64().unwrap_or(0),
            me,
            primary,
            members,
        })
    }

    fn reachable(&self) -> bool {
        self.me == self.primary || self.primary_online
    }
}

enum Outcome {
    Sent,
    Down,
    Stay,
    Drop(String),
    Unsure(String),
}

fn bare(id: &str) -> String {
    Qualified::parse(id).map_or_else(|| id.to_string(), |q| q.id)
}

fn done(act: &HeldAct) -> Value {
    match act {
        HeldAct::Message { .. } | HeldAct::SendNow { .. } => json!({"sent": true, "relayed": true}),
        HeldAct::Answer { .. } => json!({"answered": true, "relayed": true}),
        HeldAct::NewSession { .. } => json!({"started": true, "relayed": true}),
    }
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

fn fnv(text: &str) -> u64 {
    text.bytes().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3)
    })
}

fn target_of(act: &HeldAct) -> HeldTarget {
    match act.session_id() {
        Some(session) => HeldTarget::Session(session.to_string()),
        None => HeldTarget::Primary,
    }
}

fn host_of(addr: &str) -> String {
    if let Some(rest) = addr.strip_prefix('[') {
        let host = rest.split(']').next().unwrap_or(rest);
        return format!("[{host}]");
    }
    match addr.matches(':').count() {
        0 => addr.to_string(),
        1 => addr.split(':').next().unwrap_or(addr).to_string(),
        _ => format!("[{addr}]"),
    }
}

impl HeldRelay {
    pub fn new(parts: Parts) -> HeldRelay {
        let queue = persist::load_held(&parts.dir).unwrap_or_default();
        HeldRelay {
            parts,
            queue: Mutex::new(queue),
            flight: Mutex::new(Vec::new()),
            notices: Mutex::new(Vec::new()),
            stays: Mutex::new(BTreeMap::new()),
            routing: Mutex::new(()),
            signal: Arc::new(Signal::default()),
            flushing: AtomicBool::new(false),
            next: AtomicU64::new(0),
        }
    }

    pub fn start(self: &Arc<Self>) {
        let signal = self.signal.clone();
        let relay: Weak<HeldRelay> = Arc::downgrade(self);
        let _ = std::thread::Builder::new()
            .name("held-flush".to_string())
            .spawn(move || loop {
                {
                    let mut state = lock(&signal.state);
                    while !state.wanted && !state.stop {
                        state = signal
                            .changed
                            .wait(state)
                            .unwrap_or_else(|e| e.into_inner());
                    }
                    if state.stop {
                        return;
                    }
                    state.wanted = false;
                    state.busy = true;
                }
                if let Some(relay) = relay.upgrade() {
                    relay.flush();
                }
                lock(&signal.state).busy = false;
                signal.changed.notify_all();
            });
    }

    #[cfg(test)]
    fn settle(&self) {
        let until = std::time::Instant::now() + Duration::from_secs(10);
        let mut state = lock(&self.signal.state);
        while (state.wanted || state.busy) && std::time::Instant::now() < until {
            state = self
                .signal
                .changed
                .wait_timeout(state, Duration::from_millis(50))
                .unwrap_or_else(|e| e.into_inner())
                .0;
        }
    }

    fn now(&self) -> i64 {
        (self.parts.clock)()
    }

    fn new_id(&self) -> String {
        format!(
            "h{}-{}",
            self.now(),
            self.next.fetch_add(1, Ordering::SeqCst)
        )
    }

    fn save(&self, queue: &HeldQueue) -> bool {
        persist::save_held(&self.parts.dir, queue).is_ok()
    }

    pub fn carry(&self) {
        let mut queue = lock(&self.queue);
        if let Ok(disk) = persist::load_held(&self.parts.dir) {
            queue.absorb(&disk);
        }
        self.save(&queue);
    }

    pub fn flush(&self) {
        if self.flushing.swap(true, Ordering::SeqCst) {
            return;
        }
        self.flush_once();
        self.flushing.store(false, Ordering::SeqCst);
    }

    fn flush_once(&self) {
        if lock(&self.queue).count(None) == 0 {
            return;
        }
        let status = self.parts.wire.status();
        let Some(ctx) = Ctx::of(&status) else {
            return;
        };
        if !ctx.reachable() {
            return;
        }
        let rows = (self.parts.rows)();
        let resting = self.resting(&ctx, &rows);
        let due = {
            let mut queue = lock(&self.queue);
            let due = queue.take_due_except(self.now(), &resting);
            if due.is_empty() {
                return;
            }
            *lock(&self.flight) = due.clone();
            self.save(&queue);
            due
        };
        let mut back = Vec::new();
        let mut stopped = false;
        for held in due {
            if stopped {
                back.push(held);
                continue;
            }
            match self.deliver(&ctx, &rows, &held) {
                Outcome::Sent => self.landed(&held),
                Outcome::Down => {
                    lock(&self.stays).remove(&held.id);
                    stopped = true;
                    back.push(held);
                }
                Outcome::Stay => {
                    self.rest(&held.id);
                    back.push(held);
                }
                Outcome::Drop(reason) => {
                    lock(&self.queue).forget_sent(&held.device, &held.request);
                    self.notice(&held, reason);
                    self.landed(&held);
                }
                Outcome::Unsure(reason) => {
                    self.notice(&held, reason);
                    self.landed(&held);
                }
            }
        }
        let mut queue = lock(&self.queue);
        if !back.is_empty() {
            queue.keep_back(back, self.now());
        }
        lock(&self.flight).clear();
        self.save(&queue);
    }

    fn landed(&self, held: &Held) {
        lock(&self.stays).remove(&held.id);
        lock(&self.flight).retain(|h| h.id != held.id);
    }

    fn rest(&self, id: &str) {
        let now = self.now();
        let mut stays = lock(&self.stays);
        let tries = stays.get(id).map_or(0, |stay| stay.tries).saturating_add(1);
        let wait = (STAY_FIRST_MS << (tries - 1).min(5)).min(STAY_CAP_MS);
        stays.insert(
            id.to_string(),
            Stay {
                tries,
                retry_ms: now + wait,
            },
        );
    }

    fn resting(&self, ctx: &Ctx, rows: &[String]) -> BTreeSet<String> {
        if lock(&self.stays).is_empty() {
            return BTreeSet::new();
        }
        let entries = lock(&self.queue).list();
        let now = self.now();
        let mut stays = lock(&self.stays);
        stays.retain(|id, _| entries.iter().any(|e| e.id == *id));
        entries
            .iter()
            .filter(|e| stays.get(&e.id).is_some_and(|stay| stay.retry_ms > now))
            .filter(|e| match &e.target {
                HeldTarget::Session(session) => self.place(ctx, rows, session).is_none(),
                HeldTarget::Primary => false,
            })
            .map(|e| e.id.clone())
            .collect()
    }

    fn notice(&self, held: &Held, reason: String) {
        let mut notices = lock(&self.notices);
        notices.push(HeldNoticeView {
            id: held.id.clone(),
            device: held.device.clone(),
            reason,
            at_ms: self.now(),
        });
        let mut mine = notices.iter().filter(|n| n.device == held.device).count();
        notices.retain(|n| {
            if n.device == held.device && mine > NOTICES_PER_DEVICE {
                mine -= 1;
                false
            } else {
                true
            }
        });
        let over = notices.len().saturating_sub(NOTICES_KEPT);
        notices.drain(..over);
    }

    fn place(&self, ctx: &Ctx, rows: &[String], session: &str) -> Option<NodeId> {
        let Some(owner) = Qualified::parse(session) else {
            return Some(ctx.primary.clone());
        };
        rows.contains(&format!("{}/{}", ctx.primary.as_str(), owner.id))
            .then(|| ctx.primary.clone())
    }

    fn request(&self, act: &HeldAct, target: &NodeId) -> Result<(&'static str, Value), ()> {
        Ok(match act {
            HeldAct::Message { session_id, text } => (
                "session.reply",
                json!({"id": bare(session_id), "text": text}),
            ),
            HeldAct::SendNow { session_id } => {
                ("session.interrupt", json!({"id": bare(session_id)}))
            }
            HeldAct::Answer {
                request_id,
                allow,
                answers,
                ..
            } => {
                if Qualified::parse(request_id).is_some_and(|q| q.node != *target) {
                    return Err(());
                }
                let id = bare(request_id);
                match (allow, answers) {
                    (Some(allow), _) => (
                        "session.permission",
                        json!({"id": id, "decision": if *allow { "allow" } else { "deny" }}),
                    ),
                    (None, Some(list)) => {
                        let items = match (self.parts.prompts)(request_id).map(|p| p.kind) {
                            Some(PromptKind::Question(items)) => Some(items),
                            _ => None,
                        };
                        let answers = match items {
                            Some(items) => answer_json(list, &items),
                            None => json!(list),
                        };
                        ("session.answer", json!({"id": id, "answers": answers}))
                    }
                    (None, None) => return Err(()),
                }
            }
            HeldAct::NewSession {
                cwd,
                model,
                model_id,
            } => {
                let mut args = json!({"folder": cwd});
                let full = model_id
                    .clone()
                    .or_else(|| model.as_deref().and_then(|short| (self.parts.model)(short)));
                if let Some(full) = full {
                    args["flags"] = json!(["--model", full]);
                }
                ("session.new", args)
            }
        })
    }

    fn deliver(&self, ctx: &Ctx, rows: &[String], held: &Held) -> Outcome {
        let Some(act) = HeldAct::from_parts(&held.op, &held.args) else {
            return Outcome::Drop("A held command couldn't be read, so it was dropped".to_string());
        };
        let target = match &held.target {
            HeldTarget::Primary => ctx.primary.clone(),
            HeldTarget::Session(session) => match self.place(ctx, rows, session) {
                Some(node) => node,
                None => return Outcome::Stay,
            },
        };
        let closed = || {
            Outcome::Drop(format!(
                "{} closed that question before your answer arrived",
                ctx.primary_name
            ))
        };
        let Ok((op, args)) = self.request(&act, &target) else {
            return closed();
        };
        match self.parts.wire.call(&target, op, args, SEND_WAIT) {
            Ok(_) => Outcome::Sent,
            Err(e) if UNREACHABLE.contains(&e.code.as_str()) => Outcome::Down,
            Err(e) if e.code == "timeout" => Outcome::Unsure(format!(
                "{} didn't answer; check whether \"{}\" arrived",
                ctx.primary_name,
                act.summary()
            )),
            Err(_) if matches!(act, HeldAct::Answer { .. }) => closed(),
            Err(e) => Outcome::Drop(format!("{} refused it: {}", ctx.primary_name, e.message)),
        }
    }

    fn ident(&self, call: &Call<'_>) -> String {
        if let Some(key) = call.key {
            return format!("k:{key}");
        }
        if call.request.is_empty() {
            return format!("u:{}", self.new_id());
        }
        let body = format!("{}|{}", call.act.op(), call.act.args());
        format!("r:{}:{:016x}", call.request, fnv(&body))
    }

    fn hold(&self, call: &Call<'_>, ident: &str, ctx: &Ctx) -> Routed {
        let now = self.now();
        let mut queue = lock(&self.queue);
        let (all, mine) = {
            let flight = lock(&self.flight);
            (
                queue.count(None) + flight.len(),
                queue.count(Some(call.device))
                    + flight.iter().filter(|h| h.device == call.device).count(),
            )
        };
        let limits = self.parts.limits;
        if all >= limits.total || mine >= limits.device {
            return Routed::Fail(BridgeFailure::primary_unreachable(
                &ctx.primary_name,
                format!(
                    "{} isn't reachable and too many commands are already waiting for it. Try again when it is back.",
                    ctx.primary_name
                ),
            ));
        }
        let held = Held {
            id: self.new_id(),
            op: call.act.op().to_string(),
            args: call.act.args(),
            target: target_of(call.act),
            device: call.device.to_string(),
            request: ident.to_string(),
            at_ms: now,
        };
        let id = queue.hold(held);
        let at_ms = queue
            .list()
            .iter()
            .find(|e| e.id == id)
            .map_or(now, |e| e.at_ms);
        if !self.save(&queue) {
            queue.cancel(&id);
            return Routed::Fail(BridgeFailure::primary_unreachable(
                &ctx.primary_name,
                format!(
                    "{} isn't reachable and this machine couldn't keep your command to send later.",
                    ctx.primary_name
                ),
            ));
        }
        drop(queue);
        self.signal.wake();
        Routed::Held(HeldRef { id, at_ms })
    }

    fn blocked(&self, act: &HeldAct) -> bool {
        let now = self.now();
        let target = target_of(act);
        let queue = lock(&self.queue);
        let flight = lock(&self.flight);
        let waiting: Vec<String> = queue.waiting(now).into_iter().map(|e| e.id).collect();
        queue
            .list()
            .iter()
            .chain(flight.iter())
            .any(|e| e.target == target && !waiting.contains(&e.id))
    }

    fn owned_by_primary(call: &Call<'_>, ctx: &Ctx) -> bool {
        match call.act.session_id() {
            None => true,
            Some(session) => Qualified::parse(session).is_some_and(|q| q.node == ctx.primary),
        }
    }

    fn stale(&self, ctx: &Ctx) -> bool {
        ctx.standby && !ctx.primary_online
    }

    fn node_json(&self, ctx: &Ctx, served: &Served, member: &Value) -> Value {
        let id = member["id"].as_str().unwrap_or_default();
        let mine = id == ctx.me.as_str();
        let facts = &member["versions"]["bridge"];
        let port = if mine {
            Some(served.port)
        } else {
            facts["port"].as_u64().and_then(|p| u16::try_from(p).ok())
        };
        let fingerprint = if mine {
            served.fingerprint.clone()
        } else {
            facts["fingerprint"].as_str().map(str::to_string)
        };
        let addrs: Vec<String> = match port {
            Some(port) => member["addrs"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .map(|a| format!("{}:{port}", host_of(a)))
                .collect(),
            None => Vec::new(),
        };
        json!({
            "id": id,
            "name": member["name"],
            "addrs": addrs,
            "fingerprint": fingerprint.unwrap_or_default(),
            "canBeMaster": true,
            "online": member["online"].as_bool().unwrap_or(false),
        })
    }
}

impl Drop for HeldRelay {
    fn drop(&mut self) {
        lock(&self.signal.state).stop = true;
        self.signal.changed.notify_all();
    }
}

impl Relay for HeldRelay {
    fn cluster(&self) -> Option<Value> {
        let status = self.parts.wire.status();
        let ctx = Ctx::of(&status)?;
        let served = (self.parts.bridge)();
        let nodes: Vec<Value> = ctx
            .members
            .iter()
            .filter(|m| {
                m["canBeMaster"].as_bool() == Some(true)
                    && m.get("state").and_then(Value::as_str).unwrap_or("active") == "active"
            })
            .map(|m| self.node_json(&ctx, &served, m))
            .collect();
        Some(json!({
            "primary": ctx.primary.as_str(),
            "term": ctx.term,
            "nodes": nodes,
        }))
    }

    fn route(&self, call: &Call<'_>) -> Routed {
        let status = self.parts.wire.status();
        let Some(ctx) = Ctx::of(&status) else {
            return Routed::Local;
        };
        if !ctx.standby || !Self::owned_by_primary(call, &ctx) {
            return Routed::Local;
        }
        let ident = self.ident(call);
        let (op, args) = {
            let _gate = lock(&self.routing);
            let found = lock(&self.queue).find(call.device, &ident);
            match found {
                Some(Found::Held(held)) => {
                    return Routed::Held(HeldRef {
                        id: held.id,
                        at_ms: held.at_ms,
                    })
                }
                Some(Found::Delivered(_)) => return Routed::Done(done(call.act)),
                None => {}
            }
            if !ctx.primary_online || self.blocked(call.act) {
                return self.hold(call, &ident, &ctx);
            }
            let Ok(request) = self.request(call.act, &ctx.primary) else {
                return Routed::Fail(BridgeFailure::invalid("That can't be sent."));
            };
            {
                let mut queue = lock(&self.queue);
                queue.note_sent(&self.new_id(), call.device, &ident);
                self.save(&queue);
            }
            request
        };
        let forget = || {
            let mut queue = lock(&self.queue);
            queue.forget_sent(call.device, &ident);
            self.save(&queue);
        };
        match self.parts.wire.call(&ctx.primary, op, args, SEND_WAIT) {
            Ok(_) => Routed::Done(done(call.act)),
            Err(e) if UNREACHABLE.contains(&e.code.as_str()) => {
                forget();
                self.hold(call, &ident, &ctx)
            }
            Err(e) if e.code == "timeout" => Routed::Fail(BridgeFailure::primary_unreachable(
                &ctx.primary_name,
                format!(
                    "{} didn't answer, so that may or may not have arrived.",
                    ctx.primary_name
                ),
            )),
            Err(e) => {
                forget();
                Routed::Fail(BridgeFailure::invalid(e.message))
            }
        }
    }

    fn list(&self, device: &str) -> HeldList {
        let now = self.now();
        let queue = lock(&self.queue);
        let flight = lock(&self.flight);
        let waiting: Vec<String> = queue.waiting(now).into_iter().map(|e| e.id).collect();
        let entries = flight
            .iter()
            .cloned()
            .chain(queue.list())
            .filter(|held| held.device == device)
            .map(|held| {
                let act = HeldAct::from_parts(&held.op, &held.args);
                HeldView {
                    summary: act.as_ref().map_or_else(String::new, HeldAct::summary),
                    session_id: match &held.target {
                        HeldTarget::Session(id) => Some(id.clone()),
                        HeldTarget::Primary => None,
                    },
                    waiting: waiting.contains(&held.id),
                    key: held.request.strip_prefix("k:").map(str::to_string),
                    id: held.id,
                    device: held.device,
                    op: held.op,
                    at_ms: held.at_ms,
                }
            })
            .collect();
        HeldList {
            entries,
            notices: lock(&self.notices)
                .iter()
                .filter(|n| n.device == device)
                .cloned()
                .collect(),
        }
    }

    fn cancel(&self, device: &str, id: &str) -> Cancelled {
        let mut queue = lock(&self.queue);
        if queue.owns(id, device) {
            queue.cancel(id);
            self.save(&queue);
            return Cancelled::Dropped;
        }
        let in_flight = lock(&self.flight)
            .iter()
            .any(|h| h.id == id && h.device == device);
        if in_flight {
            return Cancelled::InFlight;
        }
        if queue.delivered_to(id, device) {
            return Cancelled::Delivered;
        }
        Cancelled::Unknown
    }

    fn cancel_key(&self, device: &str, key: &str) -> Cancelled {
        let ident = format!("k:{key}");
        let mut queue = lock(&self.queue);
        match queue.find(device, &ident) {
            Some(Found::Held(held)) => {
                queue.cancel(&held.id);
                self.save(&queue);
                Cancelled::Dropped
            }
            Some(Found::Delivered(id)) => {
                let in_flight = lock(&self.flight).iter().any(|h| h.id == id);
                if in_flight {
                    Cancelled::InFlight
                } else {
                    Cancelled::Delivered
                }
            }
            None => Cancelled::Unknown,
        }
    }

    fn confirm(&self, device: &str, id: &str) -> bool {
        let confirmed = {
            let mut queue = lock(&self.queue);
            let waiting = queue.owns(id, device) && queue.is_waiting(id, self.now());
            if waiting {
                queue.confirm(id);
                self.save(&queue);
            }
            waiting
        };
        if confirmed {
            self.signal.wake();
        }
        confirmed
    }

    fn stale_ms(&self) -> Option<i64> {
        let ctx = Ctx::of(&self.parts.wire.status())?;
        self.stale(&ctx)
            .then(|| self.parts.set.newest_ms())
            .flatten()
    }

    fn set_transcript(&self, session: &Session) -> Option<PathBuf> {
        let ctx = Ctx::of(&self.parts.wire.status())?;
        if !self.stale(&ctx) {
            return None;
        }
        let owner = Qualified::parse(&session.id)?;
        if owner.node != ctx.primary {
            return None;
        }
        let original = session.transcript_path.as_deref()?;
        self.parts.set.transcript(Path::new(original))
    }

    fn tick(&self) {
        if lock(&self.queue).count(None) > 0 {
            self.signal.wake();
        }
    }
}

#[cfg(test)]
#[path = "cluster_relay_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "cluster_relay_prompts_tests.rs"]
mod prompts_tests;

#[cfg(test)]
#[path = "cluster_relay_phone_tests.rs"]
mod phone_tests;

#[cfg(test)]
#[path = "cluster_relay_retry_tests.rs"]
mod retry_tests;
