mod read;
mod scan;

use super::fts::SearchDb;
use super::{rank, snippet, IndexedSession, SessionHit, SessionMeta, SessionTool, DEFAULT_RADIUS};
use read::Read1;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

pub const SESSION_WINDOW_SECS: f64 = super::RECENT_SECS;
pub const WHOLE_FILE_BYTES: u64 = 4_000_000;
pub const HEAD_BYTES: u64 = 200_000;
pub const TAIL_BYTES: u64 = 3_500_000;

#[derive(Clone, Debug)]
pub struct ClaudeRoot {
    pub projects: PathBuf,
    pub config_dir: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TitledSession {
    pub id: String,
    pub title: String,
    pub folder: String,
    pub modified: f64,
}

#[derive(Clone, Debug, Default)]
pub struct IndexRoots {
    pub claude: Vec<ClaudeRoot>,
    pub codex_sessions: Option<PathBuf>,
    pub titled: Vec<TitledSession>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Refresh {
    pub skipped: bool,
    pub read: usize,
    pub reused: usize,
    pub dropped: usize,
}

#[derive(Default)]
struct State {
    sessions: Vec<IndexedSession>,
    empty: HashMap<String, (f64, i64)>,
    loaded: bool,
    refreshed_at: Option<f64>,
}

pub struct SessionIndex {
    state: Mutex<State>,
    refreshing: AtomicBool,
    db: Option<SearchDb>,
}

struct Flag<'a>(&'a AtomicBool);

impl Drop for Flag<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst);
    }
}

impl SessionIndex {
    pub fn new(db: Option<PathBuf>) -> SessionIndex {
        let db = db
            .and_then(|path| SearchDb::open(&path).ok())
            .or_else(|| SearchDb::memory().ok());
        SessionIndex {
            state: Mutex::new(State::default()),
            refreshing: AtomicBool::new(false),
            db,
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn load_cache(&self) {
        let mut state = self.lock();
        if state.loaded {
            return;
        }
        state.loaded = true;
        if let Some(db) = &self.db {
            state.sessions = db.sessions();
        }
    }

    pub fn is_refreshing(&self) -> bool {
        self.refreshing.load(Ordering::SeqCst)
    }

    pub fn len(&self) -> usize {
        self.lock().sessions.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn due(&self, now: f64, min_interval: f64) -> bool {
        !self.is_refreshing()
            && self
                .lock()
                .refreshed_at
                .is_none_or(|t| now - t >= min_interval)
    }

    pub fn search<W: AsRef<str>>(&self, words: &[W], now: f64, limit: usize) -> Vec<SessionHit> {
        let state = self.lock();
        if words.iter().all(|w| w.as_ref().is_empty()) {
            return super::latest(&state.sessions, limit);
        }
        let ranked = self
            .db
            .as_ref()
            .map(|db| db.rank(words, now, limit))
            .unwrap_or_default();
        let by_key: HashMap<&str, &IndexedSession> = state
            .sessions
            .iter()
            .map(|s| (s.meta.key.as_str(), s))
            .collect();
        let hits: Vec<SessionHit> = ranked
            .into_iter()
            .filter_map(|(key, score)| {
                let s = by_key.get(key.as_str())?;
                Some(SessionHit {
                    meta: s.meta.clone(),
                    snippet: snippet(s, words, DEFAULT_RADIUS),
                    score,
                })
            })
            .collect();
        if hits.is_empty() {
            return rank(words, &state.sessions, now, limit);
        }
        hits
    }

    pub fn refresh(&self, roots: &IndexRoots, now: f64) -> Refresh {
        if self.refreshing.swap(true, Ordering::SeqCst) {
            return Refresh {
                skipped: true,
                ..Refresh::default()
            };
        }
        let _flag = Flag(&self.refreshing);
        self.load_cache();

        let mut candidates = scan::claude(&roots.claude, now);
        if let Some(sessions) = &roots.codex_sessions {
            candidates.extend(scan::codex(sessions, now));
        }
        candidates.sort_by(|a, b| {
            b.meta
                .modified
                .partial_cmp(&a.meta.modified)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        let (known, skip) = {
            let state = self.lock();
            let known: HashMap<String, (f64, i64)> = state
                .sessions
                .iter()
                .map(|s| (s.meta.key.clone(), (s.meta.modified, s.meta.size)))
                .collect();
            (known, state.empty.clone())
        };

        let mut stats = Refresh::default();
        let mut live: HashSet<String> = HashSet::new();
        let mut empty: HashMap<String, (f64, i64)> = HashMap::new();
        let mut batch: Vec<IndexedSession> = Vec::new();
        let mut written: Vec<IndexedSession> = Vec::new();
        for candidate in candidates {
            let key = candidate.meta.key.clone();
            let stamp = (candidate.meta.modified, candidate.meta.size);
            live.insert(key.clone());
            if known.get(&key) == Some(&stamp) {
                stats.reused += 1;
                continue;
            }
            if skip.get(&key) == Some(&stamp) {
                empty.insert(key, stamp);
                continue;
            }
            match read::read(&candidate.meta, candidate.tool) {
                Read1::Built(session) => {
                    stats.read += 1;
                    written.push((*session).clone());
                    batch.push(*session);
                    if batch.len() >= 25 {
                        self.merge(std::mem::take(&mut batch));
                    }
                }
                Read1::Empty => {
                    empty.insert(key, stamp);
                }
                Read1::Gone => {}
            }
        }

        {
            let state = self.lock();
            let existing: HashMap<&str, &SessionMeta> = state
                .sessions
                .iter()
                .map(|s| (s.meta.key.as_str(), &s.meta))
                .collect();
            for row in &roots.titled {
                let meta = SessionMeta {
                    key: format!("agy:{}", row.id),
                    tool_key: SessionTool::Antigravity.key().into(),
                    session_id: row.id.clone(),
                    title: row.title.clone(),
                    folder: row.folder.clone(),
                    path: String::new(),
                    config_dir: None,
                    modified: row.modified,
                    size: 0,
                };
                live.insert(meta.key.clone());
                if existing.get(meta.key.as_str()) == Some(&&meta) {
                    continue;
                }
                let session = IndexedSession::new(meta, row.title.clone());
                written.push(session.clone());
                batch.push(session);
            }
        }

        let mut state = self.lock();
        merge_into(&mut state.sessions, batch);
        let gone: Vec<String> = state
            .sessions
            .iter()
            .filter(|s| !live.contains(&s.meta.key))
            .map(|s| s.meta.key.clone())
            .collect();
        state.sessions.retain(|s| live.contains(&s.meta.key));
        stats.dropped = gone.len();
        state.empty = empty;
        state.refreshed_at = Some(now);
        if let Some(db) = &self.db {
            let _ = db.put(&written);
            let _ = db.remove(&gone);
        }
        stats
    }

    fn merge(&self, batch: Vec<IndexedSession>) {
        merge_into(&mut self.lock().sessions, batch);
    }
}

fn merge_into(sessions: &mut Vec<IndexedSession>, batch: Vec<IndexedSession>) {
    if batch.is_empty() {
        return;
    }
    let keys: HashSet<&str> = batch.iter().map(|s| s.meta.key.as_str()).collect();
    sessions.retain(|s| !keys.contains(s.meta.key.as_str()));
    sessions.extend(batch);
}

#[cfg(test)]
mod tests;
