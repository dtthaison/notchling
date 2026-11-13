use super::events::{parse_line, Event};
use super::plan::{failure_of, turn_spec, Pick, Turn, TurnSpec};
use super::process::{Next, Stream, ToolProcess, ToolSpawner};
use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{channel, Sender};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

pub const MAX_RUNNING: usize = 8;

pub const ANNOUNCE_WAIT: Duration = Duration::from_secs(30);

#[derive(Clone, Debug)]
pub struct Request {
    pub text: String,
    pub session: Option<String>,
    pub folder: String,
    pub pick: Pick,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TurnEvent {
    Started {
        session: String,
    },
    Queued {
        session: String,
        waiting: usize,
    },
    Ended {
        session: Option<String>,
        failure: Option<String>,
    },
}

pub trait TurnSink: Send + Sync {
    fn event(&self, event: TurnEvent);
}

pub type MayStart = Arc<dyn Fn() -> bool + Send + Sync>;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Sent {
    Started { session: String },
    Queued { session: String, waiting: usize },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SendError {
    TooMany,
    Spawn(String),
    NotStarted(String),
    NotAllowed,
}

impl std::fmt::Display for SendError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SendError::TooMany => write!(f, "{MAX_RUNNING} Codex turns are running already"),
            SendError::Spawn(why) => f.write_str(why),
            SendError::NotStarted(why) => f.write_str(why),
            SendError::NotAllowed => f.write_str("Codex turns may not start right now"),
        }
    }
}

impl std::error::Error for SendError {}

struct Live {
    process: Arc<dyn ToolProcess>,
    folder: String,
}

struct Waiting {
    text: String,
    pick: Pick,
    folder: String,
}

#[derive(Default)]
struct State {
    running: HashMap<String, Live>,
    reserved: HashMap<String, String>,
    outbox: HashMap<String, VecDeque<Waiting>>,
    active: usize,
}

struct Inner {
    spawner: Arc<dyn ToolSpawner>,
    codex: Mutex<PathBuf>,
    path: Mutex<Option<String>>,
    home: OnceLock<PathBuf>,
    sink: Arc<dyn TurnSink>,
    network: AtomicBool,
    announce_ms: AtomicU64,
    may_start: OnceLock<MayStart>,
    state: Mutex<State>,
}

#[derive(Clone)]
pub struct CodexRuns {
    inner: Arc<Inner>,
}

type Announce = Sender<Result<String, String>>;

impl CodexRuns {
    pub fn new(
        spawner: Arc<dyn ToolSpawner>,
        codex: PathBuf,
        path: Option<String>,
        sink: Arc<dyn TurnSink>,
    ) -> CodexRuns {
        CodexRuns {
            inner: Arc::new(Inner {
                spawner,
                codex: Mutex::new(codex),
                path: Mutex::new(path),
                home: OnceLock::new(),
                sink,
                network: AtomicBool::new(true),
                announce_ms: AtomicU64::new(ANNOUNCE_WAIT.as_millis() as u64),
                may_start: OnceLock::new(),
                state: Mutex::new(State::default()),
            }),
        }
    }

    pub fn set_network(&self, on: bool) {
        self.inner.network.store(on, Ordering::SeqCst);
    }

    pub fn set_command(&self, codex: PathBuf, path: Option<String>) {
        *self.inner.codex.lock().unwrap_or_else(|e| e.into_inner()) = codex;
        *self.inner.path.lock().unwrap_or_else(|e| e.into_inner()) = path;
    }

    pub fn set_codex_home(&self, home: PathBuf) {
        let _ = self.inner.home.set(home);
    }

    pub fn set_announce_wait(&self, wait: Duration) {
        let ms = u64::try_from(wait.as_millis()).unwrap_or(u64::MAX);
        self.inner.announce_ms.store(ms, Ordering::SeqCst);
    }

    pub fn set_may_start(&self, may_start: MayStart) {
        let _ = self.inner.may_start.set(may_start);
    }

    fn may_start(&self) -> bool {
        self.inner.may_start.get().is_none_or(|may| may())
    }

    fn state(&self) -> std::sync::MutexGuard<'_, State> {
        self.inner.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn running(&self) -> Vec<String> {
        let mut ids: Vec<String> = self.state().running.keys().cloned().collect();
        ids.sort();
        ids
    }

    pub fn is_running(&self, session: &str) -> bool {
        let state = self.state();
        state.running.contains_key(session) || state.reserved.contains_key(session)
    }

    pub fn waiting(&self, session: &str) -> usize {
        self.state().outbox.get(session).map_or(0, VecDeque::len)
    }

    pub fn active(&self) -> usize {
        self.state().active
    }

    pub fn send(&self, request: Request) -> Result<Sent, SendError> {
        if let Some(session) = request.session.clone() {
            let mut state = self.state();
            if !self.may_start() {
                return Err(SendError::NotAllowed);
            }
            if state.running.contains_key(&session) || state.reserved.contains_key(&session) {
                let folder = if request.folder.is_empty() {
                    let live = state.running.get(&session).map(|live| live.folder.clone());
                    live.or_else(|| state.reserved.get(&session).cloned())
                        .unwrap_or_default()
                } else {
                    request.folder
                };
                let queue = state.outbox.entry(session.clone()).or_default();
                queue.push_back(Waiting {
                    text: request.text,
                    pick: request.pick,
                    folder,
                });
                let waiting = queue.len();
                drop(state);
                self.inner.sink.event(TurnEvent::Queued {
                    session: session.clone(),
                    waiting,
                });
                return Ok(Sent::Queued { session, waiting });
            }
            if state.active >= MAX_RUNNING {
                return Err(SendError::TooMany);
            }
            state.active += 1;
            state
                .reserved
                .insert(session.clone(), request.folder.clone());
            drop(state);
            return match self.begin(&request, Some(&session), None) {
                Ok(_) => Ok(Sent::Started { session }),
                Err(e) => {
                    let mut state = self.state();
                    state.reserved.remove(&session);
                    state.active -= 1;
                    Err(e)
                }
            };
        }
        {
            let mut state = self.state();
            if !self.may_start() {
                return Err(SendError::NotAllowed);
            }
            if state.active >= MAX_RUNNING {
                return Err(SendError::TooMany);
            }
            state.active += 1;
        }
        let (tx, rx) = channel();
        let process = match self.begin(&request, None, Some(tx)) {
            Ok(process) => process,
            Err(e) => {
                self.state().active -= 1;
                return Err(e);
            }
        };
        match rx.recv_timeout(Duration::from_millis(
            self.inner.announce_ms.load(Ordering::SeqCst),
        )) {
            Ok(Ok(session)) => Ok(Sent::Started { session }),
            Ok(Err(why)) => Err(SendError::NotStarted(why)),
            Err(_) => {
                process.interrupt();
                Err(SendError::NotStarted(
                    "Codex didn't name the session in time".to_string(),
                ))
            }
        }
    }

    fn spec_for(&self, request: &Request, session: Option<&str>) -> TurnSpec {
        let codex = self.inner.codex.lock().unwrap_or_else(|e| e.into_inner());
        let path = self.inner.path.lock().unwrap_or_else(|e| e.into_inner());
        turn_spec(
            &codex,
            &Turn {
                prompt: &request.text,
                session,
                folder: &request.folder,
                pick: &request.pick,
                network: self.inner.network.load(Ordering::SeqCst),
                codex_home: self.inner.home.get().map(PathBuf::as_path),
            },
            path.as_deref(),
        )
    }

    fn begin(
        &self,
        request: &Request,
        session: Option<&str>,
        announce: Option<Announce>,
    ) -> Result<Arc<dyn ToolProcess>, SendError> {
        let spec = self.spec_for(request, session);
        let process = self
            .inner
            .spawner
            .spawn(&spec)
            .map_err(|e| SendError::Spawn(e.to_string()))?;
        if let Some(session) = session {
            let mut state = self.state();
            state.reserved.remove(session);
            state.running.insert(
                session.to_string(),
                Live {
                    process: process.clone(),
                    folder: request.folder.clone(),
                },
            );
        }
        if !self.may_start() {
            process.interrupt();
        }
        if let Some(session) = session {
            self.inner.sink.event(TurnEvent::Started {
                session: session.to_string(),
            });
        }
        let runs = self.clone();
        let following = process.clone();
        let owner = session.map(String::from);
        let folder = request.folder.clone();
        std::thread::Builder::new()
            .name("codex-turn".into())
            .spawn(move || runs.follow(following, owner, folder, announce))
            .map_err(|e| {
                process.interrupt();
                if let Some(session) = session {
                    self.state().running.remove(session);
                    self.inner.sink.event(TurnEvent::Ended {
                        session: Some(session.to_string()),
                        failure: Some(e.to_string()),
                    });
                }
                SendError::Spawn(e.to_string())
            })?;
        Ok(process)
    }

    fn follow(
        &self,
        mut process: Arc<dyn ToolProcess>,
        mut session: Option<String>,
        folder: String,
        mut announce: Option<Announce>,
    ) {
        loop {
            let mut last_error = String::new();
            let exit = loop {
                match process.next() {
                    Next::Exited(info) => break info,
                    Next::Line(line) => match line.stream {
                        Stream::Err => {
                            if line.text.contains("rror") {
                                last_error = line.text;
                            }
                        }
                        Stream::Out => match parse_line(&line.text) {
                            Event::ThreadStarted { thread_id } => {
                                if session.is_none() {
                                    self.state().running.insert(
                                        thread_id.clone(),
                                        Live {
                                            process: process.clone(),
                                            folder: folder.clone(),
                                        },
                                    );
                                    self.inner.sink.event(TurnEvent::Started {
                                        session: thread_id.clone(),
                                    });
                                    session = Some(thread_id.clone());
                                }
                                if let Some(tx) = announce.take() {
                                    let _ = tx.send(Ok(thread_id));
                                }
                            }
                            Event::Failed {
                                message: Some(message),
                            } => last_error = message,
                            _ => {}
                        },
                    },
                }
            };
            let mut failure = failure_of(&exit, &last_error);
            if session.is_none() && failure.is_none() {
                failure = Some("Codex didn't start a session.".to_string());
            }
            if let Some(tx) = announce.take() {
                let _ = tx.send(Err(failure
                    .clone()
                    .unwrap_or_else(|| "Codex didn't start a session.".to_string())));
            }
            let next = self.finish(session.as_deref());
            self.inner.sink.event(TurnEvent::Ended {
                session: session.clone(),
                failure,
            });
            let (Some(waiting), Some(id)) = (next, session.clone()) else {
                return;
            };
            let request = Request {
                text: waiting.text,
                session: Some(id.clone()),
                folder: waiting.folder,
                pick: waiting.pick,
            };
            match self.restart(&request, &id) {
                Ok(next) => {
                    process = next;
                    self.inner.sink.event(TurnEvent::Started { session: id });
                }
                Err(why) => {
                    let dropped = {
                        let mut state = self.state();
                        state.running.remove(&id);
                        state.active -= 1;
                        state.outbox.remove(&id).map_or(0, |q| q.len())
                    };
                    let more = if dropped > 0 {
                        format!(" ({dropped} more waiting messages were dropped)")
                    } else {
                        String::new()
                    };
                    self.inner.sink.event(TurnEvent::Ended {
                        session: Some(id),
                        failure: Some(format!("couldn't start the next message: {why}{more}")),
                    });
                    return;
                }
            }
        }
    }

    fn finish(&self, session: Option<&str>) -> Option<Waiting> {
        let mut state = self.state();
        let allowed = self.may_start();
        if let Some(s) = session.filter(|_| !allowed) {
            state.outbox.remove(s);
        }
        let next = session.filter(|_| allowed).and_then(|s| {
            let queue = state.outbox.get_mut(s)?;
            let next = queue.pop_front();
            if queue.is_empty() {
                state.outbox.remove(s);
            }
            next
        });
        if next.is_none() {
            if let Some(s) = session {
                state.running.remove(s);
            }
            state.active -= 1;
        }
        next
    }

    fn restart(&self, request: &Request, session: &str) -> Result<Arc<dyn ToolProcess>, String> {
        let spec = self.spec_for(request, Some(session));
        let process = self.inner.spawner.spawn(&spec).map_err(|e| e.to_string())?;
        self.state().running.insert(
            session.to_string(),
            Live {
                process: process.clone(),
                folder: request.folder.clone(),
            },
        );
        if !self.may_start() {
            process.interrupt();
        }
        Ok(process)
    }

    pub fn stop(&self, session: &str) -> bool {
        let process = self.state().running.get(session).map(|l| l.process.clone());
        match process {
            Some(process) => {
                process.interrupt();
                true
            }
            None => false,
        }
    }

    pub fn stop_all(&self) {
        let processes: Vec<Arc<dyn ToolProcess>> = {
            let mut state = self.state();
            state.outbox.clear();
            state.running.values().map(|l| l.process.clone()).collect()
        };
        for process in processes {
            process.interrupt();
        }
    }

    pub fn forget(&self, session: &str) -> bool {
        self.state().outbox.remove(session);
        self.stop(session)
    }
}

#[cfg(test)]
#[path = "runs_tests.rs"]
mod tests;
