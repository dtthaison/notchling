use crate::agy_phone::LogReads;
use crate::agy_sink::{lock, now_secs, Shared};
use crate::every_session::agy_agent;
use notchling_engine::sessions::AgentGroup;
use notchling_engine::tools::agy::app_store;
use notchling_engine::tools::agy::elsewhere::{self, Source};
use notchling_engine::tools::agy::summaries::Row;
use notchling_engine::transcript::model::Conversation;
use serde_json::Value;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};

pub const GAP: Duration = Duration::from_secs(2);

const REUSE: Duration = Duration::from_secs(3);
const KEEP: Duration = Duration::from_secs(600);
const STALE_TURN_SECS: f64 = 120.0;
const STARTS_KEPT: usize = 32;
const STARTS_SECS: f64 = 86_400.0;

pub const NOT_SIDECAR: &str = "This conversation is in the Antigravity app. Choose \"Antigravity app (full)\" in Settings > Agents to reply.";

type Stamp = Option<(u64, SystemTime)>;

struct Watch {
    stamp: Stamp,
    working: bool,
    seen: Instant,
}

#[derive(Default)]
struct Watching {
    watched: HashMap<String, Watch>,
    running: bool,
}

pub(crate) struct AppSessions {
    cli_root: PathBuf,
    root: PathBuf,
    copy_dir: PathBuf,
    shared: Arc<Shared>,
    logs: LogReads,
    starts: Mutex<HashMap<String, (String, f64)>>,
    state: Mutex<Watching>,
    gap: Mutex<Duration>,
}

pub(crate) struct Read {
    pub conversation: Conversation,
    pub omitted: usize,
    pub working: bool,
    pub title: String,
    pub folder: String,
}

impl AppSessions {
    pub(crate) fn new(cli_root: PathBuf, copy_dir: PathBuf, shared: Arc<Shared>) -> AppSessions {
        let root = app_store::root_beside(&cli_root);
        AppSessions {
            logs: LogReads::of(root.clone(), Source::App),
            cli_root,
            root,
            copy_dir,
            shared,
            starts: Mutex::new(HashMap::new()),
            state: Mutex::new(Watching::default()),
            gap: Mutex::new(GAP),
        }
    }

    #[cfg(test)]
    pub(crate) fn set_gap(&self, gap: Duration) {
        *lock(&self.gap) = gap;
    }

    pub(crate) fn is_app(&self, session: &str) -> bool {
        app_store::exists(&self.root, session) || lock(&self.starts).contains_key(session)
    }

    pub(crate) fn rows(&self) -> Vec<Row> {
        let mut rows = app_store::rows_cached(&self.cli_root, &self.copy_dir, now_secs(), REUSE);
        for (id, (title, at)) in lock(&self.starts).iter() {
            if !rows.iter().any(|r| &r.id == id) {
                rows.push(Row {
                    id: id.clone(),
                    title: title.clone(),
                    folder: String::new(),
                    modified: *at,
                    running: true,
                });
            }
        }
        rows
    }

    pub(crate) fn row(&self, session: &str) -> Option<Row> {
        self.rows().into_iter().find(|r| r.id == session)
    }

    pub(crate) fn started(self: &Arc<Self>, session: &str, title: &str) {
        let now = now_secs();
        {
            let mut starts = lock(&self.starts);
            starts.retain(|_, (_, at)| now - *at < STARTS_SECS);
            if starts.len() >= STARTS_KEPT {
                starts.clear();
            }
            starts.insert(session.to_owned(), (title.to_owned(), now));
        }
        self.touch(session);
        self.publish(session, "turn_started", true);
    }

    pub(crate) fn read(&self, session: &str, max_bytes: usize) -> Read {
        let log = self.logs.get(session, max_bytes);
        let row = self.row(session);
        let running = row.as_ref().is_some_and(|r| r.running);
        let fresh = elsewhere::log_stamp(&self.root, session)
            .and_then(|(_, at)| at.elapsed().ok())
            .is_some_and(|age| age.as_secs_f64() < STALE_TURN_SECS);
        let working = running || (log.0.activity.is_some() && fresh);
        let mut conversation = log.0.clone();
        if !working {
            conversation.activity = None;
            conversation.turn_start = None;
        }
        Read {
            conversation,
            omitted: log.1,
            working,
            title: row.as_ref().map(|r| r.title.clone()).unwrap_or_default(),
            folder: row.map(|r| r.folder).unwrap_or_default(),
        }
    }

    pub(crate) fn open(self: &Arc<Self>, session: &str) -> Value {
        self.touch(session);
        let working = self
            .read(session, crate::agy_ops::CONVERSATION_BYTES)
            .working;
        self.publish(session, "loaded", working);
        serde_json::json!({"id": format!("agy:{session}"), "loading": false})
    }

    pub(crate) fn touch(self: &Arc<Self>, session: &str) {
        let stamp = elsewhere::log_stamp(&self.root, session);
        let start = {
            let mut state = lock(&self.state);
            match state.watched.get_mut(session) {
                Some(watch) => watch.seen = Instant::now(),
                None => {
                    state.watched.insert(
                        session.to_owned(),
                        Watch {
                            stamp,
                            working: false,
                            seen: Instant::now(),
                        },
                    );
                }
            }
            !std::mem::replace(&mut state.running, true)
        };
        if start {
            let me = self.clone();
            std::thread::spawn(move || me.watch());
        }
    }

    fn publish(&self, session: &str, change: &str, working: bool) {
        let row = self.row(session).unwrap_or_else(|| Row {
            id: session.to_owned(),
            title: "Antigravity Session".to_owned(),
            folder: String::new(),
            modified: now_secs(),
            running: working,
        });
        let mut agent = agy_agent(&row);
        if working {
            agent.group = AgentGroup::Working;
            agent.label = "Working".to_owned();
        } else {
            agent.group = AgentGroup::Completed;
            agent.label = "Antigravity".to_owned();
        }
        if let Ok(value) = serde_json::to_value(agent) {
            self.shared.session_changed_row(session, change, value);
        }
    }

    fn watch(&self) {
        loop {
            let gap = *lock(&self.gap);
            std::thread::sleep(gap);
            let ids: Vec<String> = {
                let mut state = lock(&self.state);
                state.watched.retain(|_, w| w.seen.elapsed() < KEEP);
                if state.watched.is_empty() {
                    state.running = false;
                    return;
                }
                state.watched.keys().cloned().collect()
            };
            for id in ids {
                self.look(&id);
            }
        }
    }

    fn look(&self, session: &str) {
        let stamp = elsewhere::log_stamp(&self.root, session);
        let working = self
            .read(session, crate::agy_ops::CONVERSATION_BYTES)
            .working;
        let change = {
            let mut state = lock(&self.state);
            let Some(watch) = state.watched.get_mut(session) else {
                return;
            };
            let grew = watch.stamp != stamp;
            let flipped = watch.working != working;
            watch.stamp = stamp;
            watch.working = working;
            match (flipped, grew) {
                (true, _) if working => "turn_started",
                (true, _) => "turn_ended",
                (false, true) => "loaded",
                (false, false) => return,
            }
        };
        self.publish(session, change, working);
    }
}
