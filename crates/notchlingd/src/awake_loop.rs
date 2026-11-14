use crate::agy_ops::AgyOps;
use crate::codex_ops::CodexOps;
use crate::layout::Layout;
use notchling_engine::awake::{
    self, Awake, FakePower, Kind, Outcome, Power, RealPower, Setting, Work,
};
use notchling_engine::sessions::Agent;
use notchling_engine::settings::Settings;
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

pub struct Sources {
    pub setting: Box<dyn Fn() -> Option<Setting> + Send + Sync>,
    pub agents: Box<dyn Fn() -> Vec<Agent> + Send + Sync>,
    pub work: Box<dyn Fn() -> Work + Send + Sync>,
}

pub struct AwakeLoop {
    state: Mutex<Awake>,
    sources: Sources,
    tick: Duration,
    stop: Mutex<Option<Sender<()>>>,
    thread: Mutex<Option<JoinHandle<()>>>,
}

pub fn holds_for_real(layout: &Layout) -> bool {
    !layout.fixture
}

pub fn power_for(layout: &Layout) -> Box<dyn Power> {
    if holds_for_real(layout) {
        Box::new(RealPower)
    } else {
        Box::new(FakePower::new())
    }
}

pub fn store_setting(file: PathBuf) -> impl Fn() -> Option<Setting> + Send + Sync {
    move || Settings::open(&file).ok().map(|s| awake::read(&s))
}

pub fn agents_in(sessions: Arc<Mutex<Value>>) -> impl Fn() -> Vec<Agent> + Send + Sync {
    move || {
        let list = lock(&sessions).clone();
        serde_json::from_value(list).unwrap_or_default()
    }
}

fn say(outcome: &Outcome) {
    match outcome {
        Outcome::Unchanged => {}
        Outcome::Held(Kind::System) => eprintln!("notchling: keeping the Mac awake"),
        Outcome::Held(Kind::Display) => {
            eprintln!("notchling: keeping the Mac and its display awake")
        }
        Outcome::Released => eprintln!("notchling: no longer keeping the Mac awake"),
        Outcome::Failed(why) => eprintln!("notchling: couldn't keep the Mac awake ({why})"),
    }
}

impl AwakeLoop {
    pub fn new(sources: Sources, power: Box<dyn Power>) -> AwakeLoop {
        AwakeLoop {
            state: Mutex::new(Awake::new(power)),
            sources,
            tick: TICK_EVERY,
            stop: Mutex::new(None),
            thread: Mutex::new(None),
        }
    }

    pub fn for_daemon(
        layout: &Layout,
        sessions: Arc<Mutex<Value>>,
        codex: Arc<CodexOps>,
        agy: Arc<AgyOps>,
        power: Box<dyn Power>,
    ) -> AwakeLoop {
        AwakeLoop::new(
            Sources {
                setting: Box::new(store_setting(layout.data_dir.join("settings.json"))),
                agents: Box::new(agents_in(sessions)),
                work: Box::new(move || Work {
                    codex_turns: codex.runs().running().len(),
                    agy_sessions: agy.working_count(),
                }),
            },
            power,
        )
    }

    pub fn with_tick(mut self, tick: Duration) -> AwakeLoop {
        self.tick = tick;
        self
    }

    pub fn holding(&self) -> Option<Kind> {
        lock(&self.state).holding()
    }

    pub fn tick_once(&self) -> Outcome {
        let Some(setting) = (self.sources.setting)() else {
            return Outcome::Unchanged;
        };
        let outcome = lock(&self.state).apply(setting, || {
            awake::busy(&(self.sources.agents)(), &(self.sources.work)())
        });
        say(&outcome);
        outcome
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
            .name("notchling-awake".into())
            .spawn(move || loop {
                let Some(awake) = me.upgrade() else { return };
                awake.tick_once();
                drop(awake);
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
        say(&lock(&self.state).release());
    }
}
