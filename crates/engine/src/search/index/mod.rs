pub(crate) mod read;
pub(crate) mod scan;
mod toggle;

use super::corpus_pages;
use super::PageEntry;
use crate::pages::PageLibrary;

use super::fts::SearchDb;
use super::range::IndexRange;
use super::{rank, snippet, IndexedSession, SessionHit, SessionMeta, SessionTool, DEFAULT_RADIUS};
use read::Read1;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use toggle::Toggle;

use super::facets::{tally, FacetValue};
use super::meta::UNKNOWN_ACCOUNT;
use super::query::filter::Filter;

pub type AccountOf = Arc<dyn Fn(Option<&str>, f64) -> String + Send + Sync>;

pub(crate) fn stamp_account(accounts: Option<&AccountOf>, meta: &mut SessionMeta) {
    if let Some(accounts) = accounts {
        let at = meta.started.unwrap_or(meta.modified);
        let account = accounts(meta.config_dir.as_deref(), at);
        meta.account = Some(if account.is_empty() {
            UNKNOWN_ACCOUNT.to_owned()
        } else {
            account
        });
    }
}

impl TitledSession {
    pub(crate) fn meta(&self) -> SessionMeta {
        SessionMeta {
            key: format!("agy:{}", self.id),
            tool_key: SessionTool::Antigravity.key().into(),
            session_id: self.id.clone(),
            title: self.title.clone(),
            folder: self.folder.clone(),
            path: String::new(),
            config_dir: None,
            modified: self.modified,
            size: 0,
            parent: None,
            account: None,
            started: None,
        }
    }
}

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
    sessions: Vec<SessionMeta>,
    range: Option<f64>,
    empty: HashMap<String, (f64, i64)>,
    loaded: bool,
    complete: bool,
    refreshed_at: Option<f64>,
    stale: bool,
    epoch: u64,
    sessions_on: Toggle,
    pages_on: Toggle,
}

pub struct SessionIndex {
    state: Mutex<State>,
    writes: Mutex<()>,
    pages_writes: Mutex<()>,
    refreshing: AtomicBool,
    db: Option<SearchDb>,
    accounts: Option<AccountOf>,
}

struct Flag<'a>(&'a AtomicBool);

impl Drop for Flag<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst);
    }
}

impl SessionIndex {
    pub fn new(db: Option<PathBuf>) -> SessionIndex {
        SessionIndex::with_range(db, Some(SESSION_WINDOW_SECS))
    }

    pub fn with_range(db: Option<PathBuf>, range: Option<f64>) -> SessionIndex {
        let db = db
            .and_then(|path| SearchDb::open(&path).ok())
            .or_else(|| SearchDb::memory().ok());
        SessionIndex {
            state: Mutex::new(State {
                range,
                ..State::default()
            }),
            writes: Mutex::new(()),
            pages_writes: Mutex::new(()),
            refreshing: AtomicBool::new(false),
            db,
            accounts: None,
        }
    }

    pub fn with_accounts(mut self, accounts: AccountOf) -> SessionIndex {
        self.accounts = Some(accounts);
        self
    }

    fn stamped(&self, meta: &mut SessionMeta) {
        stamp_account(self.accounts.as_ref(), meta);
    }

    pub fn facet_values(
        &self,
        name: &str,
        prefix: &str,
        limit: usize,
        keep: &dyn Fn(&SessionMeta) -> bool,
    ) -> Vec<FacetValue> {
        self.facet_values_as(name, prefix, limit, keep, &|v| v)
    }

    pub fn facet_values_as(
        &self,
        name: &str,
        prefix: &str,
        limit: usize,
        keep: &dyn Fn(&SessionMeta) -> bool,
        shown: &dyn Fn(String) -> String,
    ) -> Vec<FacetValue> {
        let state = self.lock();
        let values: Vec<String> = state
            .sessions
            .iter()
            .filter(|m| keep(m))
            .flat_map(|m| m.facets())
            .filter(|(n, _)| *n == name)
            .map(|(_, v)| shown(v))
            .collect();
        tally(values, name, prefix, limit)
    }

    pub fn indexed_since(&self, now: f64) -> Option<f64> {
        let state = self.lock();
        match state.range {
            Some(range) => Some(now - range),
            None => {
                if state.complete {
                    return None;
                }
                Some(
                    state
                        .sessions
                        .iter()
                        .map(|s| s.modified)
                        .fold(now, f64::min),
                )
            }
        }
    }

    pub fn refresh_pages(&self, library: &PageLibrary) -> Refresh {
        self.refresh_pages_with(library, &|| {})
    }

    fn refresh_pages_with(&self, library: &PageLibrary, held: &dyn Fn()) -> Refresh {
        let _pages = self.pages_writes.lock().unwrap_or_else(|e| e.into_inner());
        if self.lock().pages_on.off() {
            return Refresh::default();
        }
        held();
        match &self.db {
            Some(db) => corpus_pages::refresh(db, library),
            None => Refresh::default(),
        }
    }

    pub fn search_pages<W: AsRef<str>>(
        &self,
        words: &[W],
        now: f64,
        limit: usize,
    ) -> Vec<corpus_pages::PageContentHit> {
        match &self.db {
            Some(db) => corpus_pages::search(db, words, now, limit),
            None => Vec::new(),
        }
    }

    pub fn page_snippet<W: AsRef<str>>(&self, entry: &PageEntry, words: &[W]) -> Option<String> {
        corpus_pages::snippet_of(self.db.as_ref()?, entry, words)
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn load_cache(&self) {
        if self.lock().loaded {
            return;
        }
        let _writes = self.writes.lock().unwrap_or_else(|e| e.into_inner());
        let mut state = self.lock();
        if state.loaded {
            return;
        }
        state.loaded = true;
        if let Some(db) = &self.db {
            state.sessions = db.session_metas();
        }
    }

    pub fn resident_bytes(&self) -> usize {
        self.lock()
            .sessions
            .iter()
            .map(|m| m.key.len() + m.title.len() + m.folder.len() + m.path.len())
            .sum()
    }

    pub fn set_range(&self, range: IndexRange) {
        self.lock().range = range.window_secs();
    }

    pub fn clear(&self) -> Result<(), String> {
        let _writes = self.writes.lock().unwrap_or_else(|e| e.into_inner());
        self.drain()
    }

    fn drain(&self) -> Result<(), String> {
        wipe(&mut self.lock());
        self.drop_sessions()
    }

    fn drop_sessions(&self) -> Result<(), String> {
        match &self.db {
            Some(db) => db.clear_corpus(super::fts::CORPUS_SESSIONS),
            None => Ok(()),
        }
    }

    pub fn set_sessions_on(&self, on: bool) -> Result<(), String> {
        self.switch(|s| &mut s.sessions_on, &self.writes, on, wipe, &|| {
            self.drop_sessions()
        })
    }

    pub fn set_pages_on(&self, on: bool) -> Result<(), String> {
        self.switch(|s| &mut s.pages_on, &self.pages_writes, on, |_| {}, &|| {
            self.drop_pages()
        })
    }

    fn switch(
        &self,
        pick: fn(&mut State) -> &mut Toggle,
        fence: &Mutex<()>,
        on: bool,
        forget: fn(&mut State),
        clear: &dyn Fn() -> Result<(), String>,
    ) -> Result<(), String> {
        let Some(ticket) = pick(&mut self.lock()).turn(on) else {
            return Ok(());
        };
        let _fence = fence.lock().unwrap_or_else(|e| e.into_inner());
        let upto = {
            let mut state = self.lock();
            let Some(upto) = pick(&mut state).owed(ticket).filter(|_| !on) else {
                return Ok(());
            };
            forget(&mut state);
            upto
        };
        clear()?;
        pick(&mut self.lock()).settle(upto);
        Ok(())
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

    pub fn keys_where(&self, keep: &dyn Fn(&SessionMeta) -> bool) -> HashSet<String> {
        self.lock()
            .sessions
            .iter()
            .filter(|m| keep(m))
            .map(|m| m.key.clone())
            .collect()
    }

    pub fn meta_of(&self, key: &str) -> Option<SessionMeta> {
        self.lock().sessions.iter().find(|m| m.key == key).cloned()
    }

    pub fn due(&self, now: f64, min_interval: f64) -> bool {
        if self.is_refreshing() {
            return false;
        }
        let state = self.lock();
        state.stale || state.refreshed_at.is_none_or(|t| now - t >= min_interval)
    }

    pub fn invalidate(&self) {
        self.lock().stale = true;
    }

    pub fn clear_pages(&self) -> Result<(), String> {
        let _pages = self.pages_writes.lock().unwrap_or_else(|e| e.into_inner());
        self.drop_pages()
    }

    fn drop_pages(&self) -> Result<(), String> {
        match &self.db {
            Some(db) => db.clear_corpus(corpus_pages::CORPUS_PAGES),
            None => Ok(()),
        }
    }

    pub fn search<W: AsRef<str>>(&self, words: &[W], now: f64, limit: usize) -> Vec<SessionHit> {
        self.search_filtered(words, &Filter::default(), now, limit)
    }

    pub fn search_filtered<W: AsRef<str>>(
        &self,
        words: &[W],
        filter: &Filter,
        now: f64,
        limit: usize,
    ) -> Vec<SessionHit> {
        if words.iter().all(|w| w.as_ref().is_empty()) {
            return latest(&self.lock().sessions, filter, limit);
        }
        let Some(db) = &self.db else {
            return Vec::new();
        };
        let ranked = db.rank_filtered(words, filter, now, limit);
        let metas: Vec<(SessionMeta, f64)> = {
            let state = self.lock();
            let by_key: HashMap<&str, &SessionMeta> =
                state.sessions.iter().map(|m| (m.key.as_str(), m)).collect();
            ranked
                .into_iter()
                .filter_map(|(key, score)| {
                    Some((by_key.get(key.as_str())?.to_owned().clone(), score))
                })
                .collect()
        };
        let hits: Vec<SessionHit> = metas
            .into_iter()
            .map(|(meta, score)| {
                let snippet = db
                    .doc_body(super::fts::CORPUS_SESSIONS, &meta.key)
                    .and_then(|body| {
                        snippet(
                            &IndexedSession::new(meta.clone(), body),
                            words,
                            DEFAULT_RADIUS,
                        )
                    });
                SessionHit {
                    meta,
                    snippet,
                    score,
                }
            })
            .collect();
        if hits.is_empty() {
            return scan_rank(db, words, filter, now, limit);
        }
        hits
    }

    pub fn refresh(&self, roots: &IndexRoots, now: f64) -> Refresh {
        self.refresh_with(roots, now, &|| {})
    }

    fn refresh_with(&self, roots: &IndexRoots, now: f64, read_done: &dyn Fn()) -> Refresh {
        if self.refreshing.swap(true, Ordering::SeqCst) {
            return Refresh {
                skipped: true,
                ..Refresh::default()
            };
        }
        let _flag = Flag(&self.refreshing);
        self.load_cache();

        let (range, epoch) = {
            let mut state = self.lock();
            if state.sessions_on.off() {
                return Refresh {
                    skipped: true,
                    ..Refresh::default()
                };
            }
            state.stale = false;
            (state.range, state.epoch)
        };
        let mut candidates = scan::claude(&roots.claude, now, range);
        if let Some(sessions) = &roots.codex_sessions {
            candidates.extend(scan::codex(sessions, now, range));
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
                .map(|m| (m.key.clone(), (m.modified, m.size)))
                .collect();
            (known, state.empty.clone())
        };

        let mut stats = Refresh::default();
        let mut live: HashSet<String> = HashSet::new();
        let mut empty: HashMap<String, (f64, i64)> = HashMap::new();
        let mut batch: Vec<IndexedSession> = Vec::new();
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
                Read1::Built(mut session) => {
                    stats.read += 1;
                    self.stamped(&mut session.meta);
                    batch.push(*session);
                    if batch.len() >= 25 {
                        self.merge(std::mem::take(&mut batch), epoch);
                    }
                }
                Read1::Empty => {
                    empty.insert(key, stamp);
                }
                Read1::Gone => {}
            }
        }

        read_done();
        {
            let state = self.lock();
            let existing: HashMap<&str, &SessionMeta> =
                state.sessions.iter().map(|m| (m.key.as_str(), m)).collect();
            for row in &roots.titled {
                if range.is_some_and(|w| now - row.modified >= w) {
                    continue;
                }
                let mut meta = row.meta();
                self.stamped(&mut meta);
                live.insert(meta.key.clone());
                if existing.get(meta.key.as_str()) == Some(&&meta) {
                    continue;
                }
                batch.push(IndexedSession::new(meta, row.title.clone()));
            }
        }

        let _writes = self.writes.lock().unwrap_or_else(|e| e.into_inner());
        if self.lock().epoch != epoch {
            return stats;
        }
        self.put(batch);
        let gone: Vec<String> = {
            let mut state = self.lock();
            let gone: Vec<String> = state
                .sessions
                .iter()
                .filter(|m| !live.contains(&m.key))
                .map(|m| m.key.clone())
                .collect();
            state.sessions.retain(|m| live.contains(&m.key));
            state.empty = empty;
            state.refreshed_at = Some(now);
            state.complete = range.is_none();
            gone
        };
        stats.dropped = gone.len();
        if let Some(db) = &self.db {
            let _ = db.remove(&gone);
        }
        stats
    }

    fn merge(&self, batch: Vec<IndexedSession>, epoch: u64) {
        let _writes = self.writes.lock().unwrap_or_else(|e| e.into_inner());
        if self.lock().epoch != epoch {
            return;
        }
        self.put(batch);
    }

    fn put(&self, batch: Vec<IndexedSession>) {
        if let Some(db) = &self.db {
            let _ = db.put(&batch);
        }
        merge_into(
            &mut self.lock().sessions,
            batch.into_iter().map(|s| s.meta).collect(),
        );
    }
}

fn wipe(state: &mut State) {
    state.sessions.clear();
    state.empty.clear();
    state.complete = false;
    state.refreshed_at = None;
    state.loaded = true;
    state.epoch += 1;
}

fn merge_into(sessions: &mut Vec<SessionMeta>, batch: Vec<SessionMeta>) {
    if batch.is_empty() {
        return;
    }
    let keys: HashSet<&str> = batch.iter().map(|m| m.key.as_str()).collect();
    sessions.retain(|m| !keys.contains(m.key.as_str()));
    sessions.extend(batch);
}

const SCAN_CHUNK: usize = 64;

fn scan_rank<W: AsRef<str>>(
    db: &SearchDb,
    words: &[W],
    filter: &Filter,
    now: f64,
    limit: usize,
) -> Vec<SessionHit> {
    let mut best: Vec<SessionHit> = Vec::new();
    let mut after = 0;
    loop {
        let (mut chunk, last) = db.session_chunk(after, SCAN_CHUNK);
        if chunk.is_empty() {
            break;
        }
        after = last;
        chunk.retain(|s| s.meta.matches(filter));
        best.extend(rank(words, &chunk, now, limit));
        best.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then(
                    b.meta
                        .modified
                        .partial_cmp(&a.meta.modified)
                        .unwrap_or(std::cmp::Ordering::Equal),
                )
        });
        best.truncate(limit);
    }
    best
}

fn latest(sessions: &[SessionMeta], filter: &Filter, limit: usize) -> Vec<SessionHit> {
    let mut sorted: Vec<&SessionMeta> = sessions
        .iter()
        .filter(|m| m.parent.is_none() && m.matches(filter))
        .collect();
    sorted.sort_by(|a, b| {
        b.modified
            .partial_cmp(&a.modified)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    sorted
        .into_iter()
        .take(limit)
        .map(|m| SessionHit {
            meta: m.clone(),
            snippet: None,
            score: 0.0,
        })
        .collect()
}

#[cfg(test)]
mod children_tests;
#[cfg(test)]
mod coverage_tests;
#[cfg(test)]
mod facets_tests;
#[cfg(test)]
mod fence_tests;
#[cfg(test)]
mod fork_tests;
#[cfg(test)]
mod range_tests;
#[cfg(test)]
mod tests;
