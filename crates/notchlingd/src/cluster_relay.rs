use crate::bridge_backend::answer_json;
use notchling_engine::bridge::model::{PendingPrompt, PromptKind, Session};
use notchling_engine::bridge::relay::{
    Call, Cancelled, HeldAct, HeldList, HeldNoticeView, HeldRef, HeldView, Relay, Routed,
};
use notchling_engine::bridge::BridgeFailure;
use notchling_engine::cluster::held::{Held, HeldQueue, HeldTarget};
use notchling_engine::cluster::persist;
use notchling_engine::cluster::wire::WireError;
use notchling_engine::cluster::{NodeId, Qualified};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub const SEND_WAIT: Duration = Duration::from_secs(10);
pub const NOTICES_KEPT: usize = 20;

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

pub struct Served {
    pub port: u16,
    pub fingerprint: Option<String>,
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
}

pub struct HeldRelay {
    parts: Parts,
    queue: Mutex<HeldQueue>,
    notices: Mutex<Vec<HeldNoticeView>>,
    turn: Mutex<()>,
    next: AtomicU64,
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

impl HeldRelay {
    pub fn new(parts: Parts) -> HeldRelay {
        let queue = persist::load_held(&parts.dir).unwrap_or_default();
        HeldRelay {
            parts,
            queue: Mutex::new(queue),
            notices: Mutex::new(Vec::new()),
            turn: Mutex::new(()),
            next: AtomicU64::new(0),
        }
    }

    fn now(&self) -> i64 {
        (self.parts.clock)()
    }

    fn save(&self, queue: &HeldQueue) {
        if let Err(why) = persist::save_held(&self.parts.dir, queue) {
            eprintln!("notchling: the held commands couldn't be saved: {why}");
        }
    }

    pub fn carry(&self) {
        let mut queue = lock(&self.queue);
        if let Ok(disk) = persist::load_held(&self.parts.dir) {
            queue.absorb(&disk);
        }
        self.save(&queue);
    }

    pub fn flush(&self) {
        if lock(&self.queue).list().is_empty() {
            return;
        }
        let status = self.parts.wire.status();
        let Some(ctx) = Ctx::of(&status) else {
            return;
        };
        let _turn = lock(&self.turn);
        self.flush_locked(&ctx);
    }

    fn flush_locked(&self, ctx: &Ctx) {
        if !ctx.reachable() {
            return;
        }
        let now = self.now();
        let due = {
            let mut queue = lock(&self.queue);
            let due = queue.take_due(now);
            if !due.is_empty() {
                self.save(&queue);
            }
            due
        };
        if due.is_empty() {
            return;
        }
        let rows = (self.parts.rows)();
        let mut back = Vec::new();
        let mut stopped = false;
        for held in due {
            if stopped {
                back.push(held);
                continue;
            }
            match self.deliver(ctx, &rows, &held) {
                Outcome::Sent => {}
                Outcome::Down => {
                    stopped = true;
                    back.push(held);
                }
                Outcome::Stay => back.push(held),
                Outcome::Drop(reason) => self.notice(&held.id, reason, now),
            }
        }
        let mut queue = lock(&self.queue);
        if !back.is_empty() {
            queue.keep_back(back, now);
        }
        self.save(&queue);
    }

    fn notice(&self, id: &str, reason: String, at_ms: i64) {
        let mut notices = lock(&self.notices);
        notices.push(HeldNoticeView {
            id: id.to_string(),
            reason,
            at_ms,
        });
        let over = notices.len().saturating_sub(NOTICES_KEPT);
        notices.drain(..over);
    }

    fn place(&self, ctx: &Ctx, rows: &[String], session: &str) -> Option<NodeId> {
        let Some(owner) = Qualified::parse(session) else {
            return Some(ctx.primary.clone());
        };
        let at = |node: &NodeId| rows.contains(&format!("{}/{}", node.as_str(), owner.id));
        if at(&ctx.primary) {
            Some(ctx.primary.clone())
        } else if owner.node != ctx.primary && at(&owner.node) {
            None
        } else {
            Some(ctx.primary.clone())
        }
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
            Err(e) if e.code == "timeout" => Outcome::Drop(format!(
                "{} didn't answer; check whether \"{}\" arrived",
                ctx.primary_name,
                act.summary()
            )),
            Err(_) if matches!(act, HeldAct::Answer { .. }) => closed(),
            Err(e) => Outcome::Drop(format!("{} refused it: {}", ctx.primary_name, e.message)),
        }
    }

    fn hold(&self, call: &Call<'_>) -> Routed {
        let now = self.now();
        let act = call.act;
        let target = match act.session_id() {
            Some(session) => HeldTarget::Session(session.to_string()),
            None => HeldTarget::Primary,
        };
        let held = Held {
            id: format!("h{now}-{}", self.next.fetch_add(1, Ordering::SeqCst)),
            op: act.op().to_string(),
            args: act.args(),
            target,
            device: call.device.to_string(),
            request: call.request.to_string(),
            at_ms: now,
        };
        let mut queue = lock(&self.queue);
        let id = queue.hold(held);
        let at_ms = queue
            .list()
            .iter()
            .find(|e| e.id == id)
            .map_or(now, |e| e.at_ms);
        self.save(&queue);
        Routed::Held(HeldRef { id, at_ms })
    }

    fn waiting_ahead(&self) -> bool {
        let now = self.now();
        let queue = lock(&self.queue);
        let waiting: Vec<String> = queue.waiting(now).into_iter().map(|e| e.id).collect();
        queue.list().iter().any(|e| !waiting.contains(&e.id))
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
            .map(|m| {
                let id = m["id"].as_str().unwrap_or_default();
                let fingerprint = match (&served.fingerprint, id == ctx.me.as_str()) {
                    (Some(own), true) => own.clone(),
                    _ => m["fingerprint"].as_str().unwrap_or_default().to_string(),
                };
                let addrs: Vec<String> = m["addrs"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_str)
                    .map(|a| {
                        if a.contains(':') && !a.starts_with('[') {
                            format!("[{a}]:{}", served.port)
                        } else {
                            format!("{a}:{}", served.port)
                        }
                    })
                    .collect();
                json!({
                    "id": id,
                    "name": m["name"],
                    "addrs": addrs,
                    "fingerprint": fingerprint,
                    "canBeMaster": true,
                    "online": m["online"].as_bool().unwrap_or(false),
                })
            })
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
        let _turn = lock(&self.turn);
        self.flush_locked(&ctx);
        if !ctx.standby || !Self::owned_by_primary(call, &ctx) {
            return Routed::Local;
        }
        if !ctx.primary_online || self.waiting_ahead() {
            return self.hold(call);
        }
        let Ok((op, args)) = self.request(call.act, &ctx.primary) else {
            return Routed::Fail(BridgeFailure::invalid("That can't be sent."));
        };
        match self.parts.wire.call(&ctx.primary, op, args, SEND_WAIT) {
            Ok(_) => Routed::Done(done(call.act)),
            Err(e) if UNREACHABLE.contains(&e.code.as_str()) => self.hold(call),
            Err(e) if e.code == "timeout" => Routed::Fail(BridgeFailure::primary_unreachable(
                &ctx.primary_name,
                format!(
                    "{} didn't answer, so that may or may not have arrived.",
                    ctx.primary_name
                ),
            )),
            Err(e) => Routed::Fail(BridgeFailure::invalid(e.message)),
        }
    }

    fn list(&self) -> HeldList {
        let now = self.now();
        let queue = lock(&self.queue);
        let waiting: Vec<String> = queue.waiting(now).into_iter().map(|e| e.id).collect();
        let entries = queue
            .list()
            .into_iter()
            .map(|held| {
                let act = HeldAct::from_parts(&held.op, &held.args);
                HeldView {
                    summary: act.as_ref().map_or_else(String::new, HeldAct::summary),
                    session_id: match &held.target {
                        HeldTarget::Session(id) => Some(id.clone()),
                        HeldTarget::Primary => None,
                    },
                    waiting: waiting.contains(&held.id),
                    id: held.id,
                    op: held.op,
                    at_ms: held.at_ms,
                }
            })
            .collect();
        HeldList {
            entries,
            notices: lock(&self.notices).clone(),
        }
    }

    fn cancel(&self, id: &str) -> Cancelled {
        let mut queue = lock(&self.queue);
        if queue.was_delivered(id) {
            return Cancelled::Delivered;
        }
        if queue.cancel(id) {
            self.save(&queue);
            return Cancelled::Dropped;
        }
        Cancelled::Unknown
    }

    fn confirm(&self, id: &str) -> bool {
        let confirmed = {
            let mut queue = lock(&self.queue);
            let confirmed = queue.confirm(id);
            self.save(&queue);
            confirmed
        };
        if confirmed {
            self.flush();
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
        self.flush();
    }
}

#[cfg(test)]
#[path = "cluster_relay_tests.rs"]
mod tests;
