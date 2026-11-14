use crate::acting_ops::{ActingOps, NoLog, Room};
use crate::awake_loop::agents_in;
use crate::held_ops::now_ms;
use crate::layout::Layout;
use notchling_engine::control::wake::{self, sleep_cut};
use notchling_engine::control::{Nudge, WakeResumer};
use notchling_engine::reply::plan;
use notchling_engine::sessions::Agent;
use notchling_engine::settings::Settings;
use notchling_local_api::ApiError;
use notchling_platform::awake::{PowerEvent, PowerEvents};
use serde_json::Value;
use std::path::PathBuf;
use std::sync::mpsc::{channel, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::Duration;

pub const TICK_EVERY: Duration = Duration::from_secs(2);

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

pub trait Wakes: Send {
    fn woke(&mut self) -> bool;
}

pub struct NoWakes;

impl Wakes for NoWakes {
    fn woke(&mut self) -> bool {
        false
    }
}

struct SystemWakes(PowerEvents);

impl Wakes for SystemWakes {
    fn woke(&mut self) -> bool {
        let mut woke = false;
        while let Ok(event) = self.0.events().try_recv() {
            woke |= event == PowerEvent::DidWake;
        }
        woke
    }
}

pub fn listens_for_real(layout: &Layout) -> bool {
    !layout.fixture
}

pub fn wakes_for(layout: &Layout) -> Box<dyn Wakes> {
    if !listens_for_real(layout) {
        return Box::new(NoWakes);
    }
    match PowerEvents::start() {
        Ok(events) => Box::new(SystemWakes(events)),
        Err(e) => {
            eprintln!("notchling: can't tell when the Mac wakes ({e})");
            Box::new(NoWakes)
        }
    }
}

pub fn store_enabled(file: PathBuf) -> impl Fn() -> bool + Send + Sync {
    move || {
        Settings::open(&file)
            .map(|s| wake::enabled(&s))
            .unwrap_or(false)
    }
}

impl ActingOps {
    pub(crate) fn nudge(&self, agent: &Agent, text: &str) -> Result<(), ApiError> {
        self.play(agent, &plan::reply(text), &mut NoLog)?;
        Ok(())
    }
}

pub type CutOff = Box<dyn Fn(&Agent) -> Option<String> + Send + Sync>;
pub type Nudger = Box<dyn Fn(&Agent, &str) -> Result<(), String> + Send + Sync>;

pub struct Sources {
    pub enabled: Box<dyn Fn() -> bool + Send + Sync>,
    pub can_act: Box<dyn Fn() -> bool + Send + Sync>,
    pub agents: Box<dyn Fn() -> Vec<Agent> + Send + Sync>,
    pub cut_off: CutOff,
    pub now_ms: Box<dyn Fn() -> i64 + Send + Sync>,
    pub nudge: Nudger,
}

pub struct WakeLoop {
    resumer: Mutex<WakeResumer>,
    wakes: Mutex<Box<dyn Wakes>>,
    sources: Sources,
    tick: Duration,
    stop: Mutex<Option<Sender<()>>>,
    thread: Mutex<Option<JoinHandle<()>>>,
}

impl WakeLoop {
    pub fn new(sources: Sources, wakes: Box<dyn Wakes>) -> WakeLoop {
        WakeLoop {
            resumer: Mutex::new(WakeResumer::new()),
            wakes: Mutex::new(wakes),
            sources,
            tick: TICK_EVERY,
            stop: Mutex::new(None),
            thread: Mutex::new(None),
        }
    }

    pub fn for_daemon(
        layout: &Layout,
        sessions: Arc<Mutex<Value>>,
        acting: Arc<ActingOps>,
        room: Arc<dyn Room>,
        wakes: Box<dyn Wakes>,
    ) -> WakeLoop {
        let may = acting.clone();
        WakeLoop::new(
            Sources {
                enabled: Box::new(store_enabled(layout.data_dir.join("settings.json"))),
                can_act: Box::new(move || may.is_on() && room.can_answer()),
                agents: Box::new(agents_in(sessions)),
                cut_off: Box::new(sleep_cut),
                now_ms: Box::new(now_ms),
                nudge: Box::new(move |agent, text| {
                    acting.nudge(agent, text).map_err(|e| e.message)
                }),
            },
            wakes,
        )
    }

    pub fn with_tick(mut self, tick: Duration) -> WakeLoop {
        self.tick = tick;
        self
    }

    pub fn with_clock(mut self, now_ms: Box<dyn Fn() -> i64 + Send + Sync>) -> WakeLoop {
        self.sources.now_ms = now_ms;
        self
    }

    pub fn tick_once(&self) -> Vec<Nudge> {
        if lock(&self.wakes).woke() {
            lock(&self.resumer).did_wake((self.sources.now_ms)());
        }
        if !lock(&self.resumer).waiting() || !(self.sources.enabled)() || !(self.sources.can_act)()
        {
            return Vec::new();
        }
        let agents = (self.sources.agents)();
        let now = (self.sources.now_ms)();
        let nudges = lock(&self.resumer).check(&agents, now, true, &*self.sources.cut_off);
        for nudge in &nudges {
            if let Some(agent) = agents.iter().find(|a| a.id == nudge.agent_id) {
                match (self.sources.nudge)(agent, &nudge.text) {
                    Ok(()) => eprintln!(
                        "notchling: told job {} to continue after sleep",
                        nudge.job_id
                    ),
                    Err(why) => eprintln!(
                        "notchling: couldn't tell job {} to continue after sleep ({why})",
                        nudge.job_id
                    ),
                }
            }
            lock(&self.resumer).finished(&nudge.agent_id);
        }
        nudges
    }

    pub fn spawn_loop(self: &Arc<Self>) {
        let mut thread = lock(&self.thread);
        if thread.is_some() {
            return;
        }
        let (stop_tx, stop_rx) = channel::<()>();
        *lock(&self.stop) = Some(stop_tx);
        let me = Arc::downgrade(self);
        let tick = self.tick;
        *thread = std::thread::Builder::new()
            .name("notchling-wake".into())
            .spawn(move || loop {
                let Some(wake) = me.upgrade() else { return };
                wake.tick_once();
                drop(wake);
                if !matches!(stop_rx.recv_timeout(tick), Err(RecvTimeoutError::Timeout)) {
                    return;
                }
            })
            .ok();
    }

    pub fn close(&self) {
        let stop = lock(&self.stop).take();
        drop(stop);
        let thread = lock(&self.thread).take();
        if let Some(handle) = thread {
            let _ = handle.join();
        }
    }
}
