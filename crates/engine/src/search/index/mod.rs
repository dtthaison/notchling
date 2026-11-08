//! E7: the session index behind ⌘⇧P, ported from the Swift app's `SessionIndex`. It lists the
//! recent conversations (Claude Code transcripts under every config folder, Codex rollouts, and
//! Antigravity titles), reads only the files that changed, and keeps their searchable text in
//! memory and in a cache file so the next start begins from it. Matching and ranking are
//! [`crate::search::rank`].
//!
//! # Roots are parameters
//!
//! [`IndexRoots`] names every folder to read. Nothing here knows `~/.claude` or `~/.codex`; a test
//! passes temp folders and the daemon passes its layout's. Antigravity's titles come from the
//! caller too ([`TitledSession`]): its steps are protobuf and never decoded.
//!
//! # What a refresh does
//!
//! 1. Lists candidates (metadata only): files changed within [`SESSION_WINDOW_SECS`] (14 days).
//! 2. Reuses an entry whose file has the same modification time and size; skips a file that gave
//!    no text last time until it changes; reads the rest, newest first.
//! 3. Drops entries whose file is gone or aged out; saves the cache when anything changed.
//!
//! # Bounds
//!
//! A file over [`WHOLE_FILE_BYTES`] is read as its first [`HEAD_BYTES`] and last [`TAIL_BYTES`]
//! (cut at line boundaries); a line over [`crate::search::MAX_LINE_BYTES`] is skipped; a file that
//! cannot be opened (deleted mid-scan) is left out and tried again next time. Memory per file is
//! at most about 4 MB whatever its size, and an entry holds at most
//! [`crate::search::MAX_CHARS`] characters.
//!
//! # The cache
//!
//! `{"version": 2, "sessions": [{"meta": {...}, "text": "..."}]}`, the Swift app's shape, but the
//! daemon keeps its own file in its own folder (the caller picks the path), so it never writes the
//! Swift app's `search-index.json`. A different version, or damage, rebuilds the index.

mod cache;
mod read;
mod scan;

use super::{rank, IndexedSession, SessionHit, SessionMeta, SessionTool};
use read::Read1;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

/// Only conversations changed this recently are indexed (the Chat sidebar's 14 days).
pub const SESSION_WINDOW_SECS: f64 = super::RECENT_SECS;
/// A transcript up to this size is read whole.
pub const WHOLE_FILE_BYTES: u64 = 4_000_000;
/// Of a bigger one: the first bytes (title, folder)...
pub const HEAD_BYTES: u64 = 200_000;
/// ...and the last (the newest text).
pub const TAIL_BYTES: u64 = 3_500_000;
/// The cache file's version; bump it when the stored fields change.
pub const CACHE_VERSION: u32 = 2;

/// One Claude Code config folder's transcripts.
#[derive(Clone, Debug)]
pub struct ClaudeRoot {
    /// The `projects` folder.
    pub projects: PathBuf,
    /// The lane's folder when it is a lane; `None` for `~/.claude`.
    pub config_dir: Option<String>,
}

/// An Antigravity conversation known only by its title.
#[derive(Clone, Debug, PartialEq)]
pub struct TitledSession {
    pub id: String,
    pub title: String,
    pub folder: String,
    /// Seconds since 1970.
    pub modified: f64,
}

/// Everything a refresh reads.
#[derive(Clone, Debug, Default)]
pub struct IndexRoots {
    pub claude: Vec<ClaudeRoot>,
    /// Codex's `sessions` folder (`YYYY/MM/DD/rollout-*.jsonl` under it).
    pub codex_sessions: Option<PathBuf>,
    pub titled: Vec<TitledSession>,
}

/// What one refresh did.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Refresh {
    /// Another refresh was running, so this one did nothing.
    pub skipped: bool,
    /// Files read.
    pub read: usize,
    /// Entries kept because the file had not changed.
    pub reused: usize,
    /// Entries dropped (file gone or aged out).
    pub dropped: usize,
}

#[derive(Default)]
struct State {
    sessions: Vec<IndexedSession>,
    /// Files that gave no text when last read, with the modification time and size they had.
    empty: HashMap<String, (f64, i64)>,
    loaded: bool,
    refreshed_at: Option<f64>,
}

/// The index. Shared by reference: searching never waits for a refresh's file reads, only for
/// the short moments it swaps results in.
pub struct SessionIndex {
    state: Mutex<State>,
    refreshing: AtomicBool,
    cache: Option<PathBuf>,
}

/// Lets go of the "refreshing" flag however the refresh ends.
struct Flag<'a>(&'a AtomicBool);

impl Drop for Flag<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst);
    }
}

impl SessionIndex {
    /// An index that loads and saves its cache at `cache` (none: memory only).
    pub fn new(cache: Option<PathBuf>) -> SessionIndex {
        SessionIndex {
            state: Mutex::new(State::default()),
            refreshing: AtomicBool::new(false),
            cache,
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Loads the cache once, if there is one. Called by [`SessionIndex::refresh`]; a caller that
    /// wants to answer from the cache before the first refresh calls it first.
    pub fn load_cache(&self) {
        let mut state = self.lock();
        if state.loaded {
            return;
        }
        state.loaded = true;
        if let Some(path) = &self.cache {
            state.sessions = cache::load(path);
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

    /// True when no refresh is running and the last one ended `min_interval` seconds or more
    /// before `now` (or never ran).
    pub fn due(&self, now: f64, min_interval: f64) -> bool {
        !self.is_refreshing()
            && self
                .lock()
                .refreshed_at
                .is_none_or(|t| now - t >= min_interval)
    }

    /// The best sessions for the query's words (lowercase; see [`crate::search::words`]), at most
    /// `limit`. No words: the most recently changed, with no snippet.
    pub fn search<W: AsRef<str>>(&self, words: &[W], now: f64, limit: usize) -> Vec<SessionHit> {
        let state = self.lock();
        if words.iter().all(|w| w.as_ref().is_empty()) {
            return super::latest(&state.sessions, limit);
        }
        rank(words, &state.sessions, now, limit)
    }

    /// Reads what changed under `roots` as of `now` (seconds since 1970). Blocks for the file
    /// reads; a second call meanwhile returns at once with `skipped`.
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
        let mut changed = false;
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
                    batch.push(*session);
                    changed = true;
                    // Search works while the first scan is still going.
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

        // Antigravity's steps aren't read: its sessions are found by title.
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
                batch.push(IndexedSession::new(meta, row.title.clone()));
                changed = true;
            }
        }

        let mut state = self.lock();
        merge_into(&mut state.sessions, batch);
        let before = state.sessions.len();
        state.sessions.retain(|s| live.contains(&s.meta.key));
        stats.dropped = before - state.sessions.len();
        state.empty = empty;
        state.refreshed_at = Some(now);
        if changed || stats.dropped > 0 {
            if let Some(path) = &self.cache {
                // Best effort: an index that can't be cached still works.
                let _ = cache::save(path, &state.sessions);
            }
        }
        stats
    }

    /// Adds finished reads to the index so a search sees them before the scan ends.
    fn merge(&self, batch: Vec<IndexedSession>) {
        merge_into(&mut self.lock().sessions, batch);
    }
}

/// Puts `batch` in, replacing entries with the same key.
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
