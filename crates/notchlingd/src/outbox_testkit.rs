use crate::acting::Acting;
use crate::acting_ops::ActingOps;
use crate::acting_ops_tests::{row, sessions, setup_in, FakeRoom, Setup, PROMPT};
use crate::late_emit::Events;
use crate::outbox_courier::{ClaudeCourier, Clock, Courier, JobFiles};
use crate::outbox_ops::{OutboxOps, SendArgs};
use crate::outbox_postman::{Couriers, Postmen, Verifier};
use crate::outbox_verify::SafeModes;
use notchling_engine::control::{Command, LaunchError, LaunchOutput, Launcher, PtyScript};
use notchling_engine::outbox::{OutboxStore, QueuedVerdict, Row, Verdict, Via};
use notchling_engine::reply::keys::ENTER;
use notchling_engine::sessions::{Agent, Roots};
use notchling_engine::transcript::queue_state::QueueState;
use notchling_engine::transcript::sent::Seen;
use notchling_events::{MemorySink, Salt};
use serde_json::{json, Value};
use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub(crate) const T0: i64 = 1_794_697_060_000;
pub(crate) const IDLE: &str = "idle0001";
pub(crate) const BUSY: &str = "abcd1234";

pub(crate) const DRAFT: &[&str] = &[
    "────────────────────────────────────────",
    "❯ my own draft",
    "────────────────────────────────────────",
    "  ? for shortcuts",
];

pub(crate) const DIALOG: &[&str] = &["Do you want to proceed?", " 1. Yes", " 2. No"];

pub(crate) const NO_BOX: &[&str] = &["Resume a session", "  1. fix-hook"];

#[derive(Default)]
pub(crate) struct FakeFiles {
    pub rows: Mutex<HashMap<String, Agent>>,
    pub states: Mutex<HashMap<String, String>>,
    pub statuses: Mutex<HashMap<String, String>>,
    pub restarted: AtomicBool,
    pub queue: Mutex<Option<QueueState>>,
    pub processes: Mutex<HashMap<String, (u32, Option<i64>)>>,
    pub born: Mutex<HashMap<String, i64>>,
    pub shared_name: AtomicBool,
}

impl JobFiles for FakeFiles {
    fn row(&self, job: &str) -> Option<Agent> {
        self.rows.lock().unwrap().get(job).cloned()
    }
    fn state(&self, job: &str) -> Option<String> {
        let known = self.rows.lock().unwrap().contains_key(job);
        let state = self.states.lock().unwrap().get(job).cloned();
        state.or_else(|| known.then(String::new))
    }
    fn status(&self, job: &str) -> Option<String> {
        self.statuses.lock().unwrap().get(job).cloned()
    }
    fn restarted(&self, _agent: &Agent, _now_ms: i64) -> bool {
        self.restarted.load(Ordering::SeqCst)
    }
    fn queue(&self, _transcript: &str) -> Option<QueueState> {
        self.queue.lock().unwrap().clone()
    }
    fn process(&self, job: &str) -> Option<(u32, Option<i64>)> {
        self.processes.lock().unwrap().get(job).copied()
    }
    fn born_ms(&self, path: &str) -> Option<i64> {
        let set = self.born.lock().unwrap().get(path).copied();
        set.or_else(|| crate::outbox_courier::made_ms(path))
    }
    fn shares_name(&self, _agent: &Agent, _path: &str) -> bool {
        self.shared_name.load(Ordering::SeqCst)
    }
}

impl FakeFiles {
    pub(crate) fn born_at(&self, path: &std::path::Path, ms: i64) {
        self.born
            .lock()
            .unwrap()
            .insert(path.to_string_lossy().into_owned(), ms);
    }
    pub(crate) fn set_status(&self, job: &str, status: &str) {
        self.statuses
            .lock()
            .unwrap()
            .insert(job.into(), status.into());
    }
    pub(crate) fn set_state(&self, job: &str, state: &str) {
        self.states.lock().unwrap().insert(job.into(), state.into());
    }
    pub(crate) fn vanish(&self, job: &str) -> Option<Agent> {
        self.states.lock().unwrap().remove(job);
        self.rows.lock().unwrap().remove(job)
    }
    pub(crate) fn put(&self, job: &str, agent: Agent) {
        self.rows.lock().unwrap().insert(job.into(), agent);
    }
}

#[derive(Default)]
pub(crate) struct Script {
    pub seen: Mutex<Option<Seen>>,
    pub waits: Mutex<VecDeque<Verdict>>,
    pub watches: Mutex<VecDeque<QueuedVerdict>>,
    pub next_look: Mutex<Option<i64>>,
    pub asked: AtomicUsize,
    pub watched_at: Mutex<Vec<i64>>,
    pub during_wait: Mutex<Option<Box<dyn Fn() + Send>>>,
}

impl Verifier for Script {
    fn seen(&self, _row: &Row) -> Option<Seen> {
        self.asked.fetch_add(1, Ordering::SeqCst);
        *self.seen.lock().unwrap()
    }
    fn wait(&self, _row: &Row, _before: Seen, _entered_ms: i64) -> Verdict {
        self.asked.fetch_add(1, Ordering::SeqCst);
        if let Some(hook) = self.during_wait.lock().unwrap().as_ref() {
            hook();
        }
        self.waits
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or(Verdict::Sent)
    }
    fn watch(&self, _row: &Row, _before: Seen, now_ms: i64) -> QueuedVerdict {
        self.asked.fetch_add(1, Ordering::SeqCst);
        self.watched_at.lock().unwrap().push(now_ms);
        self.watches
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or(QueuedVerdict::Stay)
    }
    fn next_look_ms(&self, _row: &Row) -> Option<i64> {
        *self.next_look.lock().unwrap()
    }
}

#[derive(Default)]
pub(crate) struct Recorder {
    pub runs: Mutex<Vec<Vec<String>>>,
}

impl Launcher for Recorder {
    fn run(&self, command: &Command, _timeout: Duration) -> Result<LaunchOutput, LaunchError> {
        self.runs.lock().unwrap().push(command.argv.clone());
        Ok(LaunchOutput {
            code: 0,
            output: String::new(),
            stderr: String::new(),
        })
    }
    fn run_pty(&self, _script: &PtyScript) -> Result<bool, LaunchError> {
        Err(LaunchError::Unsupported)
    }
}

pub(crate) struct Rig {
    pub postmen: Postmen,
    pub ops: Arc<OutboxOps>,
    pub acting: Arc<ActingOps>,
    pub courier: Arc<ClaudeCourier>,
    pub files: Arc<FakeFiles>,
    pub verifier: Arc<Script>,
    pub clock: Arc<AtomicI64>,
    pub room: Arc<FakeRoom>,
    pub attached: crate::acting_ops_tests::Attaches,
    pub writes: Arc<Mutex<Vec<String>>>,
    pub screen: Arc<Mutex<Vec<String>>>,
    pub deaf: Arc<AtomicBool>,
    pub starting: Arc<std::sync::atomic::AtomicU64>,
    pub human: Arc<Mutex<Vec<(u64, String)>>>,
    pub launched: Arc<Recorder>,
    pub safe: Arc<SafeModes>,
    pub sink: Arc<MemorySink>,
}

pub(crate) struct Options {
    pub screen: &'static [&'static str],
    pub reply_screen: Option<PathBuf>,
    pub courier: Option<Arc<dyn Courier>>,
}

impl Default for Options {
    fn default() -> Options {
        Options {
            screen: PROMPT,
            reply_screen: None,
            courier: None,
        }
    }
}

fn agent(list: &Value, id: &str) -> Agent {
    let found = list
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["id"] == json!(id))
        .unwrap();
    serde_json::from_value(found.clone()).unwrap()
}

pub(crate) fn rig() -> Rig {
    rig_with(Options::default())
}

pub(crate) fn rig_with(options: Options) -> Rig {
    let mut list = sessions();
    list.as_array_mut().unwrap().push(row(
        "job-idle2",
        json!({"job": "idle0002"}),
        "completed",
        Some("/tmp/none/idle2.jsonl"),
    ));
    let roots = Roots {
        main_dir: PathBuf::from("/nonexistent/claude"),
        lanes_file: PathBuf::from("/nonexistent/lanes.json"),
    };
    let launched = Arc::new(Recorder::default());
    let launcher: Arc<dyn Launcher> = launched.clone();
    let Setup {
        ops,
        room,
        attached,
        writes,
        deaf,
        starting,
        screen,
        human,
        ..
    } = setup_in(
        true,
        options.screen,
        list.clone(),
        roots.clone(),
        Some(launcher),
    );
    let ops = match options.reply_screen {
        Some(file) => ops.with_reply_screen_file(file),
        None => ops,
    };
    let sink = Arc::new(MemorySink::new());
    let events = Events::fixed(sink.clone(), Salt([1; 32]));
    let acting = Arc::new(ops.with_events(events.clone()));
    let files = Arc::new(FakeFiles::default());
    let mut idle = agent(&list, "job-idle");
    idle.transcript_path = Some("/tmp/none/idle.jsonl".into());
    files.put(IDLE, idle);
    files.put("idle0002", agent(&list, "job-idle2"));
    files.put(BUSY, agent(&list, "job-a"));
    files.set_status(IDLE, "idle");
    files.set_status("idle0002", "idle");
    files.set_status(BUSY, "busy");
    let clock = Arc::new(AtomicI64::new(T0));
    let ticks = clock.clone();
    let now: Clock = Arc::new(move || ticks.load(Ordering::SeqCst));
    let store = Arc::new(OutboxStore::in_memory_for_tests());
    let switch = acting.acting_switch();
    let outbox = Arc::new(
        OutboxOps::new(
            store,
            Arc::new(Mutex::new(list)),
            roots,
            switch.clone(),
            room.clone(),
            Some(events),
        )
        .with_clock(now.clone()),
    );
    acting.attach_outbox(outbox.clone());
    let safe = Arc::new(SafeModes::default());
    let courier = Arc::new(ClaudeCourier::with_files(
        acting.clone(),
        files.clone(),
        now.clone(),
        safe.clone(),
    ));
    let verifier = Arc::new(Script::default());
    let claude: Arc<dyn Courier> = options.courier.unwrap_or_else(|| courier.clone());
    let postmen = Postmen::build(
        outbox.clone(),
        Couriers::claude_only(claude),
        verifier.clone(),
        switch,
        now,
    );
    Rig {
        postmen,
        ops: outbox,
        acting,
        courier,
        files,
        verifier,
        clock,
        room,
        attached,
        writes,
        screen,
        deaf,
        starting,
        human,
        launched,
        safe,
        sink,
    }
}

impl Rig {
    pub(crate) fn threaded(&self) {
        self.ops.set_wake(self.postmen.waker());
        self.postmen.begin();
    }

    pub(crate) fn send(&self, id: &str, text: &str) -> Row {
        self.send_with(id, text, false)
    }

    pub(crate) fn send_with(&self, id: &str, text: &str, clear_box: bool) -> Row {
        self.ops
            .send(&SendArgs {
                id: id.into(),
                text: text.into(),
                clear_box,
                options: None,
                via: Via::Chat,
            })
            .unwrap()
    }

    pub(crate) fn row(&self, id: &str) -> Option<Row> {
        self.ops.store().get(id).unwrap()
    }

    pub(crate) fn at(&self, ms: i64) -> i64 {
        self.clock.store(ms, Ordering::SeqCst);
        ms
    }

    pub(crate) fn typed(&self) -> String {
        self.writes.lock().unwrap().concat()
    }

    pub(crate) fn returns(&self) -> usize {
        self.writes
            .lock()
            .unwrap()
            .iter()
            .filter(|w| w.as_str() == ENTER)
            .count()
    }

    pub(crate) fn attaches(&self) -> usize {
        self.attached.lock().unwrap().len()
    }

    pub(crate) fn show(&self, lines: &[&str]) {
        *self.screen.lock().unwrap() = lines.iter().map(|l| l.to_string()).collect();
    }

    pub(crate) fn acting_switch(&self) -> Arc<Acting> {
        self.acting.acting_switch()
    }
}
