use crate::acting::Acting;
use crate::inbox_ops::{InboxOps, Responder};
use crate::shells_ops::{progress_lock, Progress, ShellsOps};
use notchling_engine::inbox::{Action, ActionId, Item, Kind, Source, State, SHELL_STUCK_ID_PREFIX};
use notchling_engine::sessions::agent::Agent;
use notchling_engine::transcript::conversation;
use notchling_engine::transcript::model::Conversation;
use notchling_engine::transcript::shells_progress::{cpu_table, tree_cpu_ms, CpuProc, Sample};
use notchling_engine::transcript::shells_stuck::running_secs;
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

pub const TICK: Duration = Duration::from_secs(60);
const NAME_MAX: usize = 64;

pub struct Candidate {
    pub name: String,
    pub transcript: String,
}

pub type Candidates = Arc<dyn Fn() -> Vec<Candidate> + Send + Sync>;
pub type Reader = Arc<dyn Fn(&str) -> Arc<Conversation> + Send + Sync>;
pub type Cpu = Arc<dyn Fn() -> Option<Vec<CpuProc>> + Send + Sync>;
pub type Sizer = Arc<dyn Fn(&str) -> Option<u64> + Send + Sync>;

pub fn candidates_in(sessions: Arc<Mutex<Value>>) -> Candidates {
    Arc::new(move || {
        let list = sessions.lock().unwrap_or_else(PoisonError::into_inner);
        list.as_array()
            .map(|rows| {
                rows.iter()
                    .filter_map(|row| serde_json::from_value::<Agent>(row.clone()).ok())
                    .filter_map(|agent| {
                        let transcript = agent.transcript_path?;
                        (!transcript.is_empty()).then_some(Candidate {
                            name: agent.name,
                            transcript,
                        })
                    })
                    .collect()
            })
            .unwrap_or_default()
    })
}

#[derive(Clone)]
struct Flag {
    id: String,
    key: String,
    session: String,
    transcript: String,
    shell: String,
}

enum Msg {
    Answer(Flag, ActionId),
    Stop,
}

struct Handoff {
    tx: Sender<Msg>,
    acting: Arc<Acting>,
    flag: Flag,
}

impl Responder for Handoff {
    fn answered(&self, _item: &Item, action: ActionId) -> bool {
        match action {
            ActionId::Stop if !self.acting.is_on() => false,
            ActionId::Stop | ActionId::KeepWaiting => {
                self.tx.send(Msg::Answer(self.flag.clone(), action)).is_ok()
            }
            _ => false,
        }
    }
}

struct Pending {
    name: String,
    transcript: String,
    session: String,
    shell: String,
    key: String,
    started: Option<f64>,
}

pub struct ShellsWatch {
    ops: Arc<ShellsOps>,
    inbox: Arc<InboxOps>,
    acting: Arc<Acting>,
    candidates: Candidates,
    read: Reader,
    cpu: Cpu,
    size: Sizer,
    raised: Mutex<HashMap<String, Flag>>,
    tx: Sender<Msg>,
    rx: Mutex<Option<Receiver<Msg>>>,
    thread: Mutex<Option<JoinHandle<()>>>,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

fn plain(text: &str) -> String {
    text.chars()
        .filter(|c| !c.is_control())
        .take(NAME_MAX)
        .collect::<String>()
        .trim()
        .to_string()
}

fn detail(name: &str, minutes: u64) -> String {
    if name.is_empty() {
        format!("A shell has made no progress for {minutes} min.")
    } else {
        format!("A shell in \u{201c}{name}\u{201d} has made no progress for {minutes} min.")
    }
}

fn session_of(transcript: &str) -> Option<String> {
    Path::new(transcript)
        .file_stem()
        .and_then(|stem| stem.to_str())
        .filter(|stem| !stem.is_empty())
        .map(str::to_owned)
}

impl ShellsWatch {
    pub fn new(
        ops: Arc<ShellsOps>,
        inbox: Arc<InboxOps>,
        acting: Arc<Acting>,
        candidates: Candidates,
    ) -> ShellsWatch {
        let (tx, rx) = channel();
        ShellsWatch {
            ops,
            inbox,
            acting,
            candidates,
            read: Arc::new(conversation::read_shared),
            cpu: Arc::new(cpu_table),
            size: Arc::new(|path| Some(std::fs::metadata(path).ok()?.len())),
            raised: Mutex::new(HashMap::new()),
            tx,
            rx: Mutex::new(Some(rx)),
            thread: Mutex::new(None),
        }
    }

    pub fn with_seams(mut self, read: Reader, cpu: Cpu, size: Sizer) -> ShellsWatch {
        self.read = read;
        self.cpu = cpu;
        self.size = size;
        self
    }

    pub fn progress(&self) -> Progress {
        self.ops.progress().clone()
    }

    fn pending(&self) -> Vec<Pending> {
        let mut seen = HashSet::new();
        let mut out = Vec::new();
        for candidate in (self.candidates)() {
            let Some(session) = session_of(&candidate.transcript) else {
                continue;
            };
            let conv = (self.read)(&candidate.transcript);
            for shell in &conv.shells {
                if shell.output_path.is_empty()
                    || conv.finished_agents.contains_key(&shell.id)
                    || !seen.insert(shell.output_path.clone())
                {
                    continue;
                }
                out.push(Pending {
                    name: plain(&candidate.name),
                    transcript: candidate.transcript.clone(),
                    session: session.clone(),
                    shell: shell.id.clone(),
                    key: shell.output_path.clone(),
                    started: shell.started,
                });
            }
        }
        out
    }

    pub fn tick(&self) {
        let now_f = self.ops.clock_s();
        let now = now_f.max(0.0) as u64;
        let pending = self.pending();
        let paths: Vec<String> = pending.iter().map(|p| p.key.clone()).collect();
        let writers = if paths.is_empty() {
            HashMap::new()
        } else {
            self.ops.writers_of(&paths)
        };
        let running: Vec<(Pending, Vec<i32>)> = pending
            .into_iter()
            .filter_map(|p| {
                let pids = writers.get(&p.key).filter(|pids| !pids.is_empty())?.clone();
                Some((p, pids))
            })
            .collect();
        let table = if running.is_empty() {
            None
        } else {
            (self.cpu)()
        };
        let mut flagged: Vec<(Pending, u64)> = Vec::new();
        {
            let progress = self.ops.progress();
            let mut tracker = progress_lock(progress);
            let live: HashSet<String> = running.iter().map(|(p, _)| p.key.clone()).collect();
            tracker.forget_except(&live);
            if let Some(table) = &table {
                for (p, pids) in &running {
                    let Some(out_bytes) = (self.size)(&p.key) else {
                        continue;
                    };
                    let roots: Vec<i32> = pids.iter().copied().filter(|pid| *pid > 0).collect();
                    tracker.record(
                        &p.key,
                        Sample {
                            at: now,
                            cpu_ms: tree_cpu_ms(table, &roots),
                            out_bytes,
                        },
                    );
                }
            }
            for (p, _) in running {
                let secs = running_secs(p.started, now_f);
                if tracker.snoozed(&p.key, now) {
                    continue;
                }
                if let Some(quiet) = tracker.verdict(&p.key, now, secs) {
                    flagged.push((p, quiet.minutes()));
                }
            }
        }
        self.reconcile(flagged);
    }

    fn reconcile(&self, flagged: Vec<(Pending, u64)>) {
        let wanted: HashSet<&str> = flagged.iter().map(|(p, _)| p.key.as_str()).collect();
        let gone: Vec<String> = lock(&self.raised)
            .iter()
            .filter(|(_, flag)| !wanted.contains(flag.key.as_str()))
            .map(|(id, _)| id.clone())
            .collect();
        for id in gone {
            lock(&self.raised).remove(&id);
            self.inbox.resolve(&id, State::Withdrawn, None, None);
        }
        for (p, minutes) in flagged {
            if lock(&self.raised).values().any(|flag| flag.key == p.key) {
                continue;
            }
            self.raise(&p, minutes, (self.ops.clock_s() * 1000.0) as i64);
        }
    }

    fn raise(&self, p: &Pending, minutes: u64, now_ms: i64) {
        let id = format!("{SHELL_STUCK_ID_PREFIX}{}:{}:{now_ms}", p.session, p.shell);
        let item = Item {
            id: id.clone(),
            kind: Kind::ShellStuck,
            title: "Shell Not Making Progress".to_string(),
            detail: detail(&p.name, minutes),
            source: Source {
                node: None,
                session: Some(p.session.clone()),
                job: None,
            },
            created_ms: now_ms,
            expires_ms: None,
            actions: vec![
                Action {
                    id: ActionId::Stop,
                    label: "Stop".to_string(),
                    primary: true,
                },
                Action {
                    id: ActionId::KeepWaiting,
                    label: "Keep Waiting".to_string(),
                    primary: false,
                },
            ],
            state: State::Open,
            answer: None,
            answered_by: None,
            seen: false,
            open: None,
            closed_ms: None,
            switch: None,
            ..Default::default()
        };
        let flag = Flag {
            id: id.clone(),
            key: p.key.clone(),
            session: p.session.clone(),
            transcript: p.transcript.clone(),
            shell: p.shell.clone(),
        };
        lock(&self.raised).insert(id, flag.clone());
        let responder = Arc::new(Handoff {
            tx: self.tx.clone(),
            acting: self.acting.clone(),
            flag,
        });
        self.inbox.raise(item, Some(responder));
    }

    pub fn drain(&self) {
        let mut answers = Vec::new();
        if let Some(rx) = lock(&self.rx).as_ref() {
            while let Ok(msg) = rx.try_recv() {
                answers.push(msg);
            }
        }
        for msg in answers {
            if let Msg::Answer(flag, action) = msg {
                self.apply(&flag, action);
            }
        }
    }

    fn apply(&self, flag: &Flag, action: ActionId) {
        lock(&self.raised).remove(&flag.id);
        let now = (self.ops.clock_s().max(0.0)) as u64;
        match action {
            ActionId::KeepWaiting => progress_lock(self.ops.progress()).snooze(&flag.key, now),
            ActionId::Stop => {
                let conv = (self.read)(&flag.transcript);
                let ended = self
                    .ops
                    .stop_for_inbox(&conv, &flag.session, &flag.shell)
                    .ok()
                    .is_some_and(|reply| {
                        reply.get("state").and_then(Value::as_str) == Some("ended")
                    });
                let mut tracker = progress_lock(self.ops.progress());
                if ended {
                    tracker.forget(&flag.key);
                } else {
                    tracker.snooze(&flag.key, now);
                }
            }
            _ => {}
        }
    }

    pub fn spawn_loop(self: &Arc<Self>) {
        let mut thread = lock(&self.thread);
        if thread.is_some() {
            return;
        }
        let Some(rx) = lock(&self.rx).take() else {
            return;
        };
        let watch = self.clone();
        *thread = std::thread::Builder::new()
            .name("notchling-shells-watch".into())
            .spawn(move || watch.run(rx))
            .ok();
    }

    fn run(&self, rx: Receiver<Msg>) {
        let mut next = Instant::now();
        loop {
            let wait = next.saturating_duration_since(Instant::now());
            match rx.recv_timeout(wait) {
                Ok(Msg::Stop) | Err(RecvTimeoutError::Disconnected) => return,
                Ok(Msg::Answer(flag, action)) => self.apply(&flag, action),
                Err(RecvTimeoutError::Timeout) => {
                    self.tick();
                    next = Instant::now() + TICK;
                }
            }
        }
    }

    pub fn close(&self) {
        let _ = self.tx.send(Msg::Stop);
        let thread = lock(&self.thread).take();
        if let Some(handle) = thread {
            let _ = handle.join();
        }
    }
}

#[cfg(test)]
#[path = "shells_watch_tests.rs"]
mod tests;
