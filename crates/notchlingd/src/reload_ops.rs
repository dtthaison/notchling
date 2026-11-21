use crate::acting::ACTING_OFF;
use crate::acting_ops::{bad, forbidden, now_ms, text_arg, ActingOps, Room};
use crate::outbox_courier::live_files;
use notchling_engine::chrome::status;
use notchling_engine::control::{job_state_path, plan_respawn};
use notchling_engine::reload::flow::{self, Host, Need, Options, Phase};
use notchling_engine::reload::{bypasses, mode_of};
use notchling_engine::sessions::{Agent, AgentGroup, AgentTarget};
use notchling_events::kinds::choices::DoneOrFailed;
use notchling_local_api::{topics, ApiError, Caller, ClientKind, Tier};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::{Arc, Condvar, Mutex, MutexGuard, OnceLock, Weak};
use std::time::{Duration, Instant};

const TIMED_OUT: &str = "didn't finish in";
const SLOW: &str =
    "Claude Code is slow to restart the session. It may finish by itself; try again in a minute.";
pub const RESTARTING: &str =
    "Already restarting. The reload can't be cancelled now; it finishes by itself.";
pub const STOPPED: &str = "The reload stopped before it finished. Try again.";

pub const AUDIT_WORD: &str = "reload";

pub trait Hosts: Send + Sync {
    fn host(&self, agent: &Agent) -> Box<dyn Host + Send>;
    fn current(&self, agent: &Agent) -> Option<String>;
}

pub type Audit = Arc<dyn Fn(&Agent, &str, DoneOrFailed) + Send + Sync>;
pub type Publish = Arc<dyn Fn(&Value) + Send + Sync>;

pub fn publisher(room: Arc<dyn Room>) -> Publish {
    Arc::new(move |data| room.publish(topics::RELOADS, topics::RELOAD_CHANGED, data))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ack {
    Any,
    Rev(u64),
}

#[derive(Clone, Debug)]
struct Entry {
    phase: Phase,
    cancel: bool,
    after_turn: bool,
    since_ms: i64,
    rev: u64,
    session: Option<String>,
}

impl Entry {
    fn json(&self) -> Value {
        let mut value = serde_json::to_value(&self.phase).unwrap_or(Value::Null);
        value["after_turn"] = json!(self.after_turn);
        value["since_ms"] = json!(self.since_ms);
        if self.cancel {
            value["cancelling"] = json!(true);
        }
        if let Some(session) = &self.session {
            value["session"] = json!(session);
        }
        value
    }
}

struct Book {
    entries: HashMap<String, Entry>,
    rev: u64,
}

impl Book {
    fn next(&mut self) -> u64 {
        self.rev += 1;
        self.rev
    }

    fn report(&self, id: &str) -> Value {
        let entry = self.entries.get(id);
        json!({
            "id": id,
            "rev": entry.map_or(self.rev, |e| e.rev),
            "task": entry.map(Entry::json),
        })
    }

    fn change(&mut self, id: &str, apply: impl FnOnce(&mut Entry)) -> Option<Value> {
        let rev = self.next();
        let entry = self.entries.get_mut(id)?;
        apply(entry);
        entry.rev = rev;
        entry.since_ms = now_ms();
        Some(self.report(id))
    }
}

pub struct ReloadOps {
    audit: OnceLock<Audit>,
    publish: OnceLock<Publish>,
    hosts: Arc<dyn Hosts>,
    book: Mutex<Book>,
    wake: Condvar,
    inline: bool,
    boot: String,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Job,
    Terminal,
    Remote,
    Other,
}

impl Kind {
    pub fn word(self) -> &'static str {
        match self {
            Kind::Job => "job",
            Kind::Terminal => "terminal",
            Kind::Remote => "remote",
            Kind::Other => "other",
        }
    }
}

pub fn kind_of(agent: &Agent) -> Kind {
    if agent.node.is_some() {
        return Kind::Remote;
    }
    match &agent.target {
        AgentTarget::Job(_) if agent.command.is_none() && !agent.headless => Kind::Job,
        AgentTarget::Process(_) => Kind::Terminal,
        _ => Kind::Other,
    }
}

struct Gated {
    inner: Box<dyn Host + Send>,
    ops: Arc<ReloadOps>,
    id: String,
}

impl Host for Gated {
    fn turn_running(&mut self) -> bool {
        self.inner.turn_running()
    }
    fn dialog_open(&mut self) -> bool {
        self.inner.dialog_open()
    }
    fn tasks_in_flight(&mut self) -> Option<u32> {
        self.inner.tasks_in_flight()
    }
    fn restart(&mut self) -> Result<bool, String> {
        self.inner.restart()
    }
    fn sleep(&mut self, ms: u64) {
        if self.inner.real_time() {
            self.ops.nap(&self.id, ms)
        } else {
            self.inner.sleep(ms)
        }
    }
    fn now_ms(&mut self) -> u64 {
        self.inner.now_ms()
    }
    fn cancelled(&mut self) -> bool {
        self.ops.cancel_asked(&self.id) || self.inner.cancelled()
    }
    fn real_time(&self) -> bool {
        self.inner.real_time()
    }
}

struct Worker {
    ops: Arc<ReloadOps>,
    agent: Agent,
    after_turn: bool,
    ended: bool,
}

impl Worker {
    fn run(mut self) {
        let ops = self.ops.clone();
        let id = self.agent.id.clone();
        let mut host = Gated {
            inner: ops.hosts.host(&self.agent),
            ops: ops.clone(),
            id: id.clone(),
        };
        let options = Options {
            after_turn: self.after_turn,
        };
        let end = flow::run(&mut host, options, &mut |phase| ops.step(&id, phase));
        drop(host);
        let done = matches!(end, Phase::Done { .. });
        let session = if done {
            ops.hosts.current(&self.agent).filter(|now| *now != id)
        } else {
            None
        };
        let cancelled = matches!(end, Phase::Cancelled);
        self.end(end, session);
        if cancelled {
            return;
        }
        if let Some(audit) = ops.audit.get() {
            let result = if done {
                DoneOrFailed::Done
            } else {
                DoneOrFailed::Failed
            };
            audit(&self.agent, AUDIT_WORD, result);
        }
    }

    fn end(&mut self, phase: Phase, session: Option<String>) {
        if self.ended {
            return;
        }
        self.ended = true;
        self.ops.finish(&self.agent.id, phase, session);
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        self.end(
            Phase::NeedsYou {
                need: Need::Failed {
                    text: STOPPED.to_string(),
                },
            },
            None,
        );
    }
}

impl ReloadOps {
    pub fn new(hosts: Arc<dyn Hosts>) -> Arc<ReloadOps> {
        ReloadOps::build(hosts, false)
    }

    #[cfg(test)]
    pub fn inline(hosts: Arc<dyn Hosts>) -> Arc<ReloadOps> {
        ReloadOps::build(hosts, true)
    }

    fn build(hosts: Arc<dyn Hosts>, inline: bool) -> Arc<ReloadOps> {
        let first = u64::try_from(now_ms()).unwrap_or(0);
        Arc::new(ReloadOps {
            audit: OnceLock::new(),
            publish: OnceLock::new(),
            hosts,
            book: Mutex::new(Book {
                entries: HashMap::new(),
                rev: first,
            }),
            wake: Condvar::new(),
            inline,
            boot: format!("{}-{first}", std::process::id()),
        })
    }

    pub fn set_audit(&self, audit: Audit) {
        let _ = self.audit.set(audit);
    }

    pub fn set_publish(&self, publish: Publish) {
        let _ = self.publish.set(publish);
    }

    fn tell(&self, report: &Value) {
        if let Some(publish) = self.publish.get() {
            publish(report);
        }
    }

    pub fn start(self: &Arc<Self>, agent: &Agent, after_turn: bool) -> Value {
        let report = {
            let mut book = lock(&self.book);
            if book
                .entries
                .get(&agent.id)
                .is_some_and(|e| !e.phase.is_over())
            {
                let report = book.report(&agent.id);
                drop(book);
                return reply(agent, report);
            }
            let rev = book.next();
            book.entries.insert(
                agent.id.clone(),
                Entry {
                    phase: Phase::Waiting {
                        by: flow::Wait::Turn,
                    },
                    cancel: false,
                    after_turn,
                    since_ms: now_ms(),
                    rev,
                    session: None,
                },
            );
            book.report(&agent.id)
        };
        self.tell(&report);
        let worker = Worker {
            ops: self.clone(),
            agent: agent.clone(),
            after_turn,
            ended: false,
        };
        if self.inline {
            worker.run();
        } else {
            let _ = std::thread::Builder::new()
                .name("reload".to_string())
                .spawn(move || worker.run());
        }
        self.state(agent, None)
    }

    fn step(&self, id: &str, phase: Phase) -> bool {
        let report = {
            let mut book = lock(&self.book);
            match book.entries.get(id) {
                Some(entry) if !entry.phase.is_over() && !entry.cancel => {}
                _ => return false,
            }
            if book.entries.get(id).is_some_and(|e| e.phase == phase) {
                return true;
            }
            book.change(id, |entry| entry.phase = phase)
        };
        if let Some(report) = report {
            self.tell(&report);
        }
        true
    }

    fn finish(&self, id: &str, phase: Phase, session: Option<String>) {
        let report = {
            let mut book = lock(&self.book);
            if book.entries.get(id).is_none_or(|e| e.phase.is_over()) {
                return;
            }
            book.change(id, |entry| {
                entry.phase = phase;
                entry.cancel = false;
                entry.session = session;
            })
        };
        self.wake.notify_all();
        if let Some(report) = report {
            self.tell(&report);
        }
    }

    fn cancel_asked(&self, id: &str) -> bool {
        lock(&self.book).entries.get(id).is_some_and(|e| e.cancel)
    }

    fn nap(&self, id: &str, ms: u64) {
        let end = Instant::now() + Duration::from_millis(ms);
        let mut book = lock(&self.book);
        while book.entries.get(id).is_some_and(|e| !e.cancel) {
            let Some(left) = end.checked_duration_since(Instant::now()) else {
                break;
            };
            book = self
                .wake
                .wait_timeout(book, left)
                .unwrap_or_else(|e| e.into_inner())
                .0;
        }
    }

    pub fn cancel(&self, agent: &Agent) -> Result<Value, ApiError> {
        let (asked, report) = {
            let mut book = lock(&self.book);
            let asked = match book.entries.get(&agent.id) {
                Some(Entry {
                    phase: Phase::Working { .. },
                    ..
                }) => return Err(bad(RESTARTING)),
                Some(Entry {
                    phase: Phase::Waiting { .. },
                    cancel: false,
                    ..
                }) => true,
                _ => false,
            };
            if asked {
                book.change(&agent.id, |entry| entry.cancel = true);
            }
            (asked, book.report(&agent.id))
        };
        if asked {
            self.wake.notify_all();
            self.tell(&report);
        }
        Ok(reply(agent, report))
    }

    pub fn all(&self) -> Value {
        let book = lock(&self.book);
        let mut ids: Vec<&String> = book.entries.keys().collect();
        ids.sort();
        let tasks: Vec<Value> = ids.into_iter().map(|id| book.report(id)).collect();
        json!({ "boot": self.boot, "rev": book.rev, "tasks": tasks })
    }

    pub fn state(&self, agent: &Agent, ack: Option<Ack>) -> Value {
        let (report, removed) = {
            let mut book = lock(&self.book);
            let over = book.entries.get(&agent.id).is_some_and(|e| {
                e.phase.is_over()
                    && match ack {
                        Some(Ack::Any) => true,
                        Some(Ack::Rev(rev)) => rev == e.rev,
                        None => false,
                    }
            });
            if over {
                book.entries.remove(&agent.id);
                book.next();
            }
            (book.report(&agent.id), over)
        };
        if removed {
            self.tell(&report);
        }
        reply(agent, report)
    }

    #[cfg(test)]
    pub fn wait_over(&self, id: &str, limit: Duration) -> bool {
        let end = Instant::now() + limit;
        let mut book = lock(&self.book);
        while book.entries.get(id).is_some_and(|e| !e.phase.is_over()) {
            let Some(left) = end.checked_duration_since(Instant::now()) else {
                return false;
            };
            book = self
                .wake
                .wait_timeout(book, left)
                .unwrap_or_else(|e| e.into_inner())
                .0;
        }
        true
    }
}

fn reply(agent: &Agent, report: Value) -> Value {
    let kind = kind_of(agent);
    json!({
        "offered": kind == Kind::Job,
        "kind": kind.word(),
        "rev": report["rev"],
        "task": report["task"],
    })
}

pub struct LiveHosts {
    acting: Weak<ActingOps>,
}

impl LiveHosts {
    pub fn new(acting: &Arc<ActingOps>) -> Arc<LiveHosts> {
        Arc::new(LiveHosts {
            acting: Arc::downgrade(acting),
        })
    }
}

impl Hosts for LiveHosts {
    fn host(&self, agent: &Agent) -> Box<dyn Host + Send> {
        let job = match &agent.target {
            AgentTarget::Job(job) => job.clone(),
            _ => String::new(),
        };
        Box::new(LiveHost {
            acting: self.acting.clone(),
            id: agent.id.clone(),
            job,
        })
    }

    fn current(&self, agent: &Agent) -> Option<String> {
        let acting = self.acting.upgrade()?;
        let AgentTarget::Job(job) = &agent.target else {
            return None;
        };
        live_files(acting.roots.clone()).row(job).map(|a| a.id)
    }
}

struct LiveHost {
    acting: Weak<ActingOps>,
    id: String,
    job: String,
}

impl LiveHost {
    fn acting(&self) -> Option<Arc<ActingOps>> {
        self.acting.upgrade()
    }

    fn row(&self, acting: &ActingOps) -> Option<Agent> {
        let files = live_files(acting.roots.clone());
        files.row(&self.job).or_else(|| {
            let now = acting.agents_now();
            let by_id = now.iter().find(|a| a.id == self.id).cloned();
            by_id.or_else(|| {
                now.into_iter()
                    .find(|a| matches!(&a.target, AgentTarget::Job(job) if *job == self.job))
            })
        })
    }

    fn busy(&self, acting: &ActingOps) -> bool {
        let files = live_files(acting.roots.clone());
        let Some(fresh) = self.row(acting) else {
            return false;
        };
        fresh.turn_running()
            || (fresh.group == AgentGroup::Working && files.restarted(&fresh, now_ms()))
    }

    fn asking(&self, acting: &ActingOps) -> bool {
        let Some(fresh) = self.row(acting) else {
            return false;
        };
        fresh.group == AgentGroup::NeedsInput
            || acting
                .prompts()
                .list()
                .iter()
                .any(|p| p.session_id == fresh.id)
    }

    fn tasks(&self, acting: &ActingOps) -> Option<u32> {
        status::tasks_in_flight(&job_state_path(&acting.roots, &self.job))
    }

    fn swap(&self, acting: &ActingOps, agent: &Agent) -> Result<(), String> {
        let file = job_state_path(&acting.roots, &self.job);
        let before = status::read_flags(&file).ok_or_else(|| {
            "Claude Code's file for this session lists no start flags.".to_string()
        })?;
        let mode = live_files(acting.roots.clone())
            .transcript(agent)
            .as_deref()
            .and_then(|path| status::permission_mode(std::path::Path::new(path)))
            .filter(|_| !bypasses(&before))
            .filter(|mode| mode_of(&before).as_deref() != Some(mode.as_str()));
        let Some(mode) = mode else {
            return self.respawn(acting, None);
        };
        let wanted = status::with_mode(&before, &mode);
        status::set_flags(&file, &wanted).map_err(|e| e.to_string())?;
        self.respawn(acting, Some((&file, before, wanted)))
    }

    fn respawn(
        &self,
        acting: &ActingOps,
        edited: Option<(&std::path::Path, Vec<String>, Vec<String>)>,
    ) -> Result<(), String> {
        match acting.run_raw(&plan_respawn(&self.job, &acting.roots)) {
            Ok(_) => Ok(()),
            Err(e) => {
                let text = e.to_string();
                if let Some((file, before, wanted)) = edited {
                    if status::read_flags(file).as_ref() == Some(&wanted) {
                        let _ = status::set_flags(file, &before);
                    }
                }
                if text.contains(TIMED_OUT) {
                    return Err(SLOW.to_string());
                }
                Err(text)
            }
        }
    }
}

impl Host for LiveHost {
    fn turn_running(&mut self) -> bool {
        self.acting().is_some_and(|acting| self.busy(&acting))
    }

    fn dialog_open(&mut self) -> bool {
        self.acting().is_some_and(|acting| self.asking(&acting))
    }

    fn tasks_in_flight(&mut self) -> Option<u32> {
        let acting = self.acting()?;
        self.row(&acting)?;
        self.tasks(&acting)
    }

    fn cancelled(&mut self) -> bool {
        self.acting().is_none_or(|acting| !acting.is_on())
    }

    fn restart(&mut self) -> Result<bool, String> {
        let acting = self
            .acting()
            .ok_or_else(|| "The daemon is stopping.".to_string())?;
        let agent = self
            .row(&acting)
            .ok_or_else(|| "The session is gone.".to_string())?;
        acting.locked(&agent, |agent| {
            if !acting.is_on() {
                return Err("Act on Sessions is off.".to_string());
            }
            if self.busy(&acting) || self.asking(&acting) || self.tasks(&acting) != Some(0) {
                return Ok(false);
            }
            acting.respawn_guarded(&self.job, || self.swap(&acting, agent))?;
            Ok(true)
        })
    }

    fn sleep(&mut self, ms: u64) {
        std::thread::sleep(Duration::from_millis(ms));
    }

    fn real_time(&self) -> bool {
        true
    }

    fn now_ms(&mut self) -> u64 {
        u64::try_from(now_ms()).unwrap_or(0)
    }
}

impl ActingOps {
    fn own_agent(&self, pid: u32) -> Option<Agent> {
        let own = self.session_of(pid)?;
        self.agents_now()
            .into_iter()
            .find(|a| a.id == own || matches!(&a.target, AgentTarget::Job(job) if *job == own))
    }

    fn reload_target(&self, caller: &Caller, args: &Value) -> Result<(Agent, bool), ApiError> {
        let asked = text_arg(args, "id")?;
        if caller.tier == Tier::App && caller.client == ClientKind::App {
            let after = args
                .get("afterTurn")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            return Ok((self.agent(args)?, after));
        }
        let own = caller
            .pid
            .and_then(|pid| self.own_agent(pid))
            .ok_or_else(|| forbidden("Notchling can't tell which session this is.").with_reason("no_session"))?;
        if asked != "self" && asked != own.id {
            return Err(forbidden("A session may reload only itself."));
        }
        Ok((own, true))
    }

    pub(crate) fn reload_call(&self, caller: &Caller, args: &Value) -> Result<Value, ApiError> {
        if !self.is_on() {
            return Err(forbidden(ACTING_OFF));
        }
        let reload = self
            .reload()
            .ok_or_else(|| ApiError::internal("Reload is not wired".to_string()))?;
        let (agent, after_turn) = self.reload_target(caller, args)?;
        if args.get("cancel").and_then(Value::as_bool) == Some(true) {
            return reload.cancel(&agent);
        }
        match kind_of(&agent) {
            Kind::Job => Ok(reload.start(&agent, after_turn)),
            Kind::Terminal => Err(bad(format!(
                "This session runs in a terminal Notchling didn't start. Quit it there and run: claude --resume {}",
                notchling_engine::lanes::quote(agent.resume_session_id.as_deref().unwrap_or(&agent.id))
            ))),
            Kind::Remote => Err(bad("This session runs on another machine.")),
            Kind::Other => Err(bad("This session can't be reloaded.")),
        }
    }

    pub(crate) fn reload_state(&self, args: &Value) -> Result<Value, ApiError> {
        let reload = self
            .reload()
            .ok_or_else(|| ApiError::internal("Reload is not wired".to_string()))?;
        if args.get("all").and_then(Value::as_bool) == Some(true) {
            return Ok(reload.all());
        }
        let agent = self.agent(args)?;
        let ack = (args.get("ack").and_then(Value::as_bool) == Some(true)).then(|| {
            args.get("rev")
                .and_then(Value::as_u64)
                .map_or(Ack::Any, Ack::Rev)
        });
        Ok(reload.state(&agent, ack))
    }
}
