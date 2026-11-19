use crate::acting_ops::{now_ms, ActingOps};
use notchling_engine::chrome::drive::{self, Goal, Refusal, Report};
use notchling_engine::chrome::flow::{self, Host, Options, Phase, Step};
use notchling_engine::chrome::status::{self, Detected};
use notchling_engine::control::{claude_json_for, job_state_path, plan_respawn};
use notchling_engine::reply::keys::timing::{DETACH_QUIET_MS, DETACH_TIMEOUT_MS};
use notchling_engine::sessions::{Agent, AgentTarget};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, Weak};
use std::time::Duration;

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
}

pub struct ChromeOps {
    hosts: Arc<dyn Hosts>,
    tasks: Mutex<HashMap<String, Task>>,
    serial: AtomicU64,
    inline: bool,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
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
            hosts,
            tasks: Mutex::new(HashMap::new()),
            serial: AtomicU64::new(1),
            inline: false,
        })
    }

    #[cfg(test)]
    pub fn inline(hosts: Arc<dyn Hosts>) -> Arc<ChromeOps> {
        Arc::new(ChromeOps {
            hosts,
            tasks: Mutex::new(HashMap::new()),
            serial: AtomicU64::new(1),
            inline: true,
        })
    }

    pub fn start(self: &Arc<Self>, agent: &Agent, on: bool, force: bool) -> Value {
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
                    on,
                    phase: Phase::Working {
                        step: Step::Checking,
                    },
                    since_ms: now_ms(),
                    serial,
                },
            );
            serial
        };
        let (me, agent_now) = (self.clone(), agent.clone());
        let work = move || {
            let mut host = me.hosts.host(&agent_now);
            let id = agent_now.id.clone();
            let sink = me.clone();
            flow::run(host.as_mut(), Options { on, force }, &mut |phase| {
                sink.set(&id, serial, phase)
            });
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
        Box::new(LiveHost {
            acting: self.acting.clone(),
            id: agent.id.clone(),
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
}

impl LiveHost {
    fn acting(&self) -> Option<Arc<ActingOps>> {
        self.acting.upgrade()
    }

    fn agent(&self, acting: &ActingOps) -> Option<Agent> {
        acting.agents_now().into_iter().find(|a| a.id == self.id)
    }
}

impl Host for LiveHost {
    fn turn_running(&mut self) -> bool {
        self.acting()
            .and_then(|acting| self.agent(&acting))
            .is_some_and(|agent| agent.turn_running())
    }

    fn tasks_in_flight(&mut self) -> u32 {
        let Some(acting) = self.acting() else {
            return 0;
        };
        match self.agent(&acting).map(|a| a.target) {
            Some(AgentTarget::Job(job)) => {
                status::tasks_in_flight(&job_state_path(&acting.roots, &job))
            }
            _ => 0,
        }
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
        let before = status::read_flags(&file).ok_or_else(|| {
            "Claude Code's file for this session lists no start flags.".to_string()
        })?;
        status::set_flags(&file, &status::with_flag(&before, on)).map_err(|e| e.to_string())?;
        if let Err(e) = acting.run_raw(&plan_respawn(job, &acting.roots)) {
            let _ = status::set_flags(&file, &before);
            return Err(e.to_string());
        }
        let kept = status::read_flags(&file).and_then(|f| status::flag_of(&f));
        if kept != Some(on) {
            return Err("Claude Code didn't keep the setting for this session.".to_string());
        }
        Ok(())
    }

    fn sleep(&mut self, ms: u64) {
        std::thread::sleep(Duration::from_millis(ms));
    }

    fn now_ms(&mut self) -> u64 {
        u64::try_from(now_ms()).unwrap_or(0)
    }
}
