use crate::acting::ACTING_OFF;
use crate::acting_ops::{bad, forbidden, now_ms, text_arg, ActingOps};
use crate::outbox_courier::live_files;
use notchling_engine::chrome::status;
use notchling_engine::control::{job_state_path, plan_respawn};
use notchling_engine::reload::flow::{self, Host, Options, Phase};
use notchling_engine::reload::mode_of;
use notchling_engine::sessions::{Agent, AgentGroup, AgentTarget};
use notchling_events::kinds::choices::DoneOrFailed;
use notchling_local_api::{ApiError, Caller, ClientKind};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, Weak};

const TIMED_OUT: &str = "didn't finish in";
const SLOW: &str =
    "Claude Code is slow to restart the session. It may finish by itself; try again in a minute.";

pub const AUDIT_WORD: &str = "reload";

pub trait Hosts: Send + Sync {
    fn host(&self, agent: &Agent) -> Box<dyn Host + Send>;
    fn current(&self, agent: &Agent) -> Option<String>;
}

#[derive(Clone, Debug)]
struct Task {
    phase: Phase,
    after_turn: bool,
    since_ms: i64,
    serial: u64,
    cancel: Arc<AtomicBool>,
    session: Option<String>,
}

pub type Audit = Arc<dyn Fn(&Agent, &str, DoneOrFailed) + Send + Sync>;

pub struct ReloadOps {
    audit: OnceLock<Audit>,
    hosts: Arc<dyn Hosts>,
    tasks: Mutex<HashMap<String, Task>>,
    serial: AtomicU64,
    inline: bool,
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

struct Cancelable {
    inner: Box<dyn Host + Send>,
    cancel: Arc<AtomicBool>,
}

impl Host for Cancelable {
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
        if self.cancelled() {
            return Err("Cancelled.".to_string());
        }
        self.inner.restart()
    }
    fn sleep(&mut self, ms: u64) {
        self.inner.sleep(ms)
    }
    fn now_ms(&mut self) -> u64 {
        self.inner.now_ms()
    }
    fn cancelled(&mut self) -> bool {
        self.cancel.load(Ordering::SeqCst) || self.inner.cancelled()
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
        Arc::new(ReloadOps {
            audit: OnceLock::new(),
            hosts,
            tasks: Mutex::new(HashMap::new()),
            serial: AtomicU64::new(1),
            inline,
        })
    }

    pub fn set_audit(&self, audit: Audit) {
        let _ = self.audit.set(audit);
    }

    pub fn start(self: &Arc<Self>, agent: &Agent, after_turn: bool) -> Value {
        let cancel = Arc::new(AtomicBool::new(false));
        let serial = {
            let mut tasks = lock(&self.tasks);
            if tasks.get(&agent.id).is_some_and(|t| !t.phase.is_over()) {
                drop(tasks);
                return self.state(agent, false);
            }
            let serial = self.serial.fetch_add(1, Ordering::SeqCst);
            tasks.insert(
                agent.id.clone(),
                Task {
                    phase: Phase::Waiting {
                        by: flow::Wait::Turn,
                    },
                    after_turn,
                    since_ms: now_ms(),
                    serial,
                    cancel: cancel.clone(),
                    session: None,
                },
            );
            serial
        };
        let (me, agent_now) = (self.clone(), agent.clone());
        let work = move || {
            let mut host = Cancelable {
                inner: me.hosts.host(&agent_now),
                cancel: cancel.clone(),
            };
            let id = agent_now.id.clone();
            let sink = me.clone();
            let end = flow::run(&mut host, Options { after_turn }, &mut |phase| {
                sink.set(&id, serial, phase)
            });
            if matches!(end, Phase::Cancelled) {
                return;
            }
            let done = matches!(end, Phase::Done { .. });
            if done {
                me.followed(&agent_now, serial);
            }
            if let Some(audit) = me.audit.get() {
                let result = if done {
                    DoneOrFailed::Done
                } else {
                    DoneOrFailed::Failed
                };
                audit(&agent_now, AUDIT_WORD, result);
            }
        };
        if self.inline {
            work();
        } else if std::thread::Builder::new()
            .name("reload".to_string())
            .spawn(work)
            .is_err()
        {
            self.set(
                &agent.id,
                serial,
                Phase::NeedsYou {
                    need: flow::Need::Failed {
                        text: "Couldn't start the reload.".to_string(),
                    },
                },
            );
        }
        self.state(agent, false)
    }

    fn followed(&self, agent: &Agent, serial: u64) {
        let now = self.hosts.current(agent).filter(|id| *id != agent.id);
        let mut tasks = lock(&self.tasks);
        if let Some(task) = tasks.get_mut(&agent.id).filter(|t| t.serial == serial) {
            task.session = now;
        }
    }

    pub fn cancel(&self, agent: &Agent) -> Value {
        if let Some(task) = lock(&self.tasks)
            .get(&agent.id)
            .filter(|t| !t.phase.is_over())
        {
            task.cancel.store(true, Ordering::SeqCst);
        }
        self.state(agent, false)
    }

    fn set(&self, id: &str, serial: u64, phase: Phase) {
        let mut tasks = lock(&self.tasks);
        if let Some(task) = tasks.get_mut(id).filter(|t| t.serial == serial) {
            if task.phase != phase {
                task.since_ms = now_ms();
            }
            task.phase = phase;
        }
    }

    pub fn state(&self, agent: &Agent, ack: bool) -> Value {
        let task = {
            let mut tasks = lock(&self.tasks);
            if ack && tasks.get(&agent.id).is_some_and(|t| t.phase.is_over()) {
                tasks.remove(&agent.id);
            }
            tasks.get(&agent.id).cloned()
        };
        let kind = kind_of(agent);
        json!({
            "offered": kind == Kind::Job,
            "kind": kind.word(),
            "task": task.map(|t| {
                let mut value = serde_json::to_value(&t.phase).unwrap_or(Value::Null);
                value["after_turn"] = json!(t.after_turn);
                value["since_ms"] = json!(t.since_ms);
                if let Some(session) = t.session {
                    value["session"] = json!(session);
                }
                value
            }),
        })
    }
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
            .filter(|mode| mode_of(&before).as_deref() != Some(mode.as_str()));
        let Some(mode) = mode else {
            return self.respawn(acting, None);
        };
        let wanted = status::with_mode(&before, &mode);
        status::write_pending(
            &file,
            &status::Pending {
                at_ms: now_ms(),
                on: status::flag_of(&wanted).unwrap_or(false),
                before: before.clone(),
                wanted: wanted.clone(),
            },
        )
        .map_err(|e| e.to_string())?;
        if let Err(e) = status::set_flags(&file, &wanted) {
            status::clear_pending(&file);
            return Err(e.to_string());
        }
        self.respawn(acting, Some((&file, before, wanted)))
    }

    fn respawn(
        &self,
        acting: &ActingOps,
        edited: Option<(&std::path::Path, Vec<String>, Vec<String>)>,
    ) -> Result<(), String> {
        match acting.run_raw(&plan_respawn(&self.job, &acting.roots)) {
            Ok(_) => {
                if let Some((file, _, _)) = edited {
                    status::clear_pending(file);
                }
                Ok(())
            }
            Err(e) => {
                let text = e.to_string();
                if text.contains(TIMED_OUT) {
                    return Err(SLOW.to_string());
                }
                if let Some((file, before, wanted)) = edited {
                    if status::read_flags(file).as_ref() == Some(&wanted) {
                        let _ = status::set_flags(file, &before);
                    }
                    status::clear_pending(file);
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
        std::thread::sleep(std::time::Duration::from_millis(ms));
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
        let inside = caller
            .pid
            .filter(|_| caller.client != ClientKind::App)
            .and_then(|pid| self.session_of(pid).map(|_| pid));
        let Some(pid) = inside else {
            let after = args
                .get("afterTurn")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            return Ok((self.agent(args)?, after));
        };
        let own = self
            .own_agent(pid)
            .ok_or_else(|| forbidden("This session is not one Notchling lists."))?;
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
            return Ok(reload.cancel(&agent));
        }
        match kind_of(&agent) {
            Kind::Job => Ok(reload.start(&agent, after_turn)),
            Kind::Terminal => Err(bad(format!(
                "This session runs in a terminal Notchling didn't start. Quit it there and run: claude --resume {}",
                agent.id
            ))),
            Kind::Remote => Err(bad("This session runs on another machine.")),
            Kind::Other => Err(bad("This session can't be reloaded.")),
        }
    }

    pub(crate) fn reload_state(&self, args: &Value) -> Result<Value, ApiError> {
        let agent = self.agent(args)?;
        let ack = args.get("ack").and_then(Value::as_bool).unwrap_or(false);
        let reload = self
            .reload()
            .ok_or_else(|| ApiError::internal("Reload is not wired".to_string()))?;
        Ok(reload.state(&agent, ack))
    }
}
