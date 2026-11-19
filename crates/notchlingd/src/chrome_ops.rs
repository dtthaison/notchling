use crate::acting_ops::{now_ms, ActingOps};
use crate::outbox_courier::{live_files, JobFiles};
use notchling_engine::chrome::drive::{self, Goal, Refusal, Report};
use notchling_engine::chrome::flow::{self, Host, Options, Phase, Step};
use notchling_engine::chrome::status::{self, Detected};
use notchling_engine::control::{claude_json_for, job_state_path, plan_respawn};
use notchling_engine::reply::keys::timing::{DETACH_QUIET_MS, DETACH_TIMEOUT_MS};
use notchling_engine::sessions::{Agent, AgentGroup, AgentTarget};
use notchling_events::kinds::choices::DoneOrFailed;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, Weak};
use std::time::Duration;

const TIMED_OUT: &str = "didn't finish in";

pub trait Hosts: Send + Sync {
    fn host(&self, agent: &Agent) -> Box<dyn Host + Send>;
    fn detect(&self, agent: &Agent) -> Detected;
}

#[derive(Clone, Debug)]
struct Task {
    on: bool,
    phase: Phase,
    since_ms: i64,
    serial: u64,
    cancel: Arc<AtomicBool>,
}

pub type Audit = Arc<dyn Fn(&Agent, &str, DoneOrFailed) + Send + Sync>;

pub fn audit_word(on: bool, force: bool) -> String {
    format!(
        "chrome_{}{}",
        if on { "on" } else { "off" },
        if force { "_forced" } else { "" }
    )
}

pub struct ChromeOps {
    audit: OnceLock<Audit>,
    hosts: Arc<dyn Hosts>,
    tasks: Mutex<HashMap<String, Task>>,
    serial: AtomicU64,
    inline: bool,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

pub(crate) struct Cancelable {
    pub(crate) inner: Box<dyn Host + Send>,
    pub(crate) cancel: Arc<AtomicBool>,
}

impl Host for Cancelable {
    fn set_cancel(&mut self, cancel: Arc<AtomicBool>) {
        self.cancel = cancel;
    }
    fn turn_running(&mut self) -> bool {
        self.inner.turn_running()
    }
    fn tasks_in_flight(&mut self) -> Option<u32> {
        self.inner.tasks_in_flight()
    }
    fn look(&mut self, goal: Goal) -> Result<Report, Refusal> {
        self.inner.look(goal)
    }
    fn restart(&mut self, on: bool) -> Result<(), String> {
        if self.cancelled() {
            return Err("Cancelled.".to_string());
        }
        self.inner.restart(on)
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

pub fn offers(agent: &Agent) -> bool {
    matches!(agent.target, AgentTarget::Job(_))
        && agent.command.is_none()
        && agent.node.is_none()
        && !agent.headless
}

impl ChromeOps {
    pub fn new(hosts: Arc<dyn Hosts>) -> Arc<ChromeOps> {
        Arc::new(ChromeOps {
            audit: OnceLock::new(),
            hosts,
            tasks: Mutex::new(HashMap::new()),
            serial: AtomicU64::new(1),
            inline: false,
        })
    }

    #[cfg(test)]
    pub fn inline(hosts: Arc<dyn Hosts>) -> Arc<ChromeOps> {
        Arc::new(ChromeOps {
            audit: OnceLock::new(),
            hosts,
            tasks: Mutex::new(HashMap::new()),
            serial: AtomicU64::new(1),
            inline: true,
        })
    }

    pub fn set_audit(&self, audit: Audit) {
        let _ = self.audit.set(audit);
    }

    pub fn start(self: &Arc<Self>, agent: &Agent, on: bool, force: bool) -> Value {
        let cancel = Arc::new(AtomicBool::new(false));
        let serial = {
            let mut tasks = lock(&self.tasks);
            if let Some(old) = tasks.get(&agent.id).filter(|t| !t.phase.is_over()) {
                if old.on == on {
                    drop(tasks);
                    return self.state(agent, false);
                }
                old.cancel.store(true, Ordering::SeqCst);
            }
            let serial = self.serial.fetch_add(1, Ordering::SeqCst);
            tasks.insert(
                agent.id.clone(),
                Task {
                    on,
                    phase: Phase::Working {
                        step: Step::Checking,
                    },
                    since_ms: now_ms(),
                    serial,
                    cancel: cancel.clone(),
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
            host.inner.set_cancel(cancel.clone());
            let id = agent_now.id.clone();
            let sink = me.clone();
            let end = flow::run(&mut host, Options { on, force }, &mut |phase| {
                sink.set(&id, serial, phase)
            });
            if cancel.load(Ordering::SeqCst) {
                return;
            }
            if let Some(audit) = me.audit.get() {
                let result = match end {
                    Phase::Done { .. } => DoneOrFailed::Done,
                    _ => DoneOrFailed::Failed,
                };
                audit(&agent_now, &audit_word(on, force), result);
            }
        };
        if self.inline {
            work();
        } else {
            let spawned = std::thread::Builder::new()
                .name("chrome".to_string())
                .spawn(work);
            if spawned.is_err() {
                self.set(
                    &agent.id,
                    serial,
                    Phase::NeedsYou {
                        need: flow::Need::Failed {
                            text: "Couldn't start the check.".to_string(),
                        },
                    },
                );
            }
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
        let detected = self.hosts.detect(agent);
        json!({
            "offered": offers(agent),
            "task": task.map(|t| {
                let mut value = serde_json::to_value(&t.phase).unwrap_or(Value::Null);
                value["on"] = json!(t.on);
                value["since_ms"] = json!(t.since_ms);
                value
            }),
            "detected": detected,
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
            cancel: None,
        })
    }

    fn detect(&self, agent: &Agent) -> Detected {
        let (Some(acting), AgentTarget::Job(job)) = (self.acting.upgrade(), &agent.target) else {
            return status::detect(None, None);
        };
        let flags = status::read_flags(&job_state_path(&acting.roots, job));
        let default =
            status::default_enabled(&claude_json_for(&acting.roots, agent.config_dir.as_deref()));
        status::detect(flags.as_deref(), default)
    }
}

struct LiveHost {
    acting: Weak<ActingOps>,
    id: String,
    job: String,
    cancel: Option<Arc<AtomicBool>>,
}

impl LiveHost {
    fn acting(&self) -> Option<Arc<ActingOps>> {
        self.acting.upgrade()
    }

    fn agent(&self, acting: &ActingOps) -> Option<Agent> {
        let now = acting.agents_now();
        let by_id = now.iter().find(|a| a.id == self.id).cloned();
        by_id.or_else(|| {
            now.into_iter()
                .find(|a| matches!(&a.target, AgentTarget::Job(job) if *job == self.job))
        })
    }

    fn swap(
        &self,
        acting: &ActingOps,
        agent: &Agent,
        job: &str,
        file: &std::path::Path,
        on: bool,
    ) -> Result<(), String> {
        let before = status::read_flags(file).ok_or_else(|| {
            "Claude Code's file for this session lists no start flags.".to_string()
        })?;
        let mut wanted = status::with_flag(&before, on);
        let transcript = live_files(acting.roots.clone()).transcript(agent);
        if let Some(mode) = transcript
            .as_deref()
            .and_then(|path| status::permission_mode(std::path::Path::new(path)))
        {
            wanted = status::with_mode(&wanted, &mode);
        }
        status::write_pending(
            file,
            &status::Pending {
                at_ms: now_ms(),
                on,
                before: before.clone(),
                wanted: wanted.clone(),
            },
        )
        .map_err(|e| e.to_string())?;
        if let Err(e) = status::set_flags(file, &wanted) {
            status::clear_pending(file);
            return Err(e.to_string());
        }
        if let Err(e) = acting.run_raw(&plan_respawn(job, &acting.roots)) {
            let text = e.to_string();
            if text.contains(TIMED_OUT) {
                return Err("Claude Code is slow to restart the session. It may finish by itself; try again in a minute.".to_string());
            }
            if status::read_flags(file).as_ref() == Some(&wanted) {
                let _ = status::set_flags(file, &before);
            }
            status::clear_pending(file);
            return Err(text);
        }
        status::clear_pending(file);
        let kept = status::read_flags(file).and_then(|f| status::flag_of(&f));
        if kept != Some(on) {
            return Err("Claude Code didn't keep the setting for this session.".to_string());
        }
        Ok(())
    }

    fn superseded(&self) -> bool {
        self.cancel
            .as_ref()
            .is_some_and(|cancel| cancel.load(Ordering::SeqCst))
    }

    fn busy(&self, acting: &ActingOps) -> bool {
        let files = live_files(acting.roots.clone());
        let Some(fresh) = files.row(&self.job).or_else(|| self.agent(acting)) else {
            return false;
        };
        fresh.turn_running()
            || (fresh.group == AgentGroup::Working && files.restarted(&fresh, now_ms()))
    }
}

impl Host for LiveHost {
    fn turn_running(&mut self) -> bool {
        self.acting().is_some_and(|acting| self.busy(&acting))
    }

    fn set_cancel(&mut self, cancel: Arc<AtomicBool>) {
        self.cancel = Some(cancel);
    }

    fn cancelled(&mut self) -> bool {
        self.superseded() || self.acting().is_none_or(|acting| !acting.is_on())
    }

    fn tasks_in_flight(&mut self) -> Option<u32> {
        let acting = self.acting()?;
        self.agent(&acting)?;
        status::tasks_in_flight(&job_state_path(&acting.roots, &self.job))
    }

    fn look(&mut self, goal: Goal) -> Result<Report, Refusal> {
        let acting = self
            .acting()
            .ok_or_else(|| Refusal::Pty("the daemon is stopping".to_string()))?;
        let agent = self.agent(&acting).ok_or(Refusal::NoInput)?;
        acting.locked(&agent, |fresh| {
            let mut pty = acting
                .attach(fresh)
                .map_err(|e| Refusal::Pty(e.to_string()))?;
            let result = drive::run(pty.as_mut(), goal);
            pty.wait_quiet(
                Duration::from_millis(DETACH_QUIET_MS),
                Duration::from_millis(DETACH_TIMEOUT_MS),
            );
            result
        })
    }

    fn restart(&mut self, on: bool) -> Result<(), String> {
        let acting = self
            .acting()
            .ok_or_else(|| "The daemon is stopping.".to_string())?;
        let agent = self
            .agent(&acting)
            .ok_or_else(|| "The session is gone.".to_string())?;
        let AgentTarget::Job(job) = &agent.target else {
            return Err("This session can't be restarted.".to_string());
        };
        let file = job_state_path(&acting.roots, job);
        let result = acting.locked(&agent, |_| {
            if !acting.is_on() {
                return Err("Act on Sessions is off.".to_string());
            }
            if self.superseded() {
                return Err("Cancelled.".to_string());
            }
            if self.busy(&acting) {
                return Err("The session started working. Try again.".to_string());
            }
            acting.respawn_guarded(job, || self.swap(&acting, &agent, job, &file, on))
        });
        acting.note_chrome(
            &agent,
            if on {
                "chrome_restart_on"
            } else {
                "chrome_restart_off"
            },
            if result.is_ok() {
                DoneOrFailed::Done
            } else {
                DoneOrFailed::Failed
            },
        );
        result
    }

    fn sleep(&mut self, ms: u64) {
        std::thread::sleep(Duration::from_millis(ms));
    }

    fn now_ms(&mut self) -> u64 {
        u64::try_from(now_ms()).unwrap_or(0)
    }
}

pub const PENDING_MAX_AGE_MS: i64 = 10 * 60 * 1_000;

fn safe_to_respawn(
    acting: &ActingOps,
    files: &dyn JobFiles,
    job: &str,
    state: &std::path::Path,
) -> bool {
    let Some(agent) = files.row(job) else {
        return false;
    };
    let working = agent.turn_running()
        || (agent.group == AgentGroup::Working && files.restarted(&agent, now_ms()));
    acting.is_on() && !working && status::tasks_in_flight(state) == Some(0)
}

pub fn reconcile(acting: &ActingOps) {
    if !acting.is_on() {
        return;
    }
    let files = live_files(acting.roots.clone());
    for config in notchling_engine::sessions::config_dirs(&acting.roots) {
        let Ok(entries) = std::fs::read_dir(config.dir.join("jobs")) else {
            continue;
        };
        for entry in entries.flatten() {
            let state = entry.path().join("state.json");
            let Some(pending) = status::read_pending(&state) else {
                continue;
            };
            let job = entry.file_name().to_string_lossy().into_owned();
            let current = status::read_flags(&state);
            if current.as_ref() == Some(&pending.wanted) {
                let fresh = now_ms() - pending.at_ms <= PENDING_MAX_AGE_MS;
                if fresh && safe_to_respawn(acting, files.as_ref(), &job, &state) {
                    let _ = acting.respawn(&job);
                } else {
                    let _ = status::set_flags(&state, &pending.before);
                }
            }
            status::clear_pending(&state);
        }
    }
}

pub fn reconcile_when_acting_turns_on(acting: &Arc<ActingOps>) {
    let weak = Arc::downgrade(acting);
    acting.acting_switch().watch(Arc::new(move |on| {
        if !on {
            return;
        }
        let weak = weak.clone();
        let _ = std::thread::Builder::new()
            .name("chrome-reconcile".to_string())
            .spawn(move || {
                if let Some(acting) = weak.upgrade() {
                    reconcile(&acting);
                }
            });
    }));
}
