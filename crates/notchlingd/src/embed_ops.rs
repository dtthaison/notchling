use crate::acting_ops::Room;
use crate::embed_fetch::{entry_of, off_error, progress_json, Fetcher, ModelNet};
use crate::embed_paths;
use crate::op_group::OpGroup;
use notchling_engine::search::corpus::{Corpus, SettingsCorpus};
use notchling_engine::search::embed::helper::{HelperEmbedder, HelperSpawn, HelperState};
use notchling_engine::search::embed::{Embedder, HashEmbedder, ModelId};
use notchling_engine::search::hybrid::{
    Floors, Lists, Semantic, FLOOR_GRANITE, FLOOR_STATIC, TOP_CHUNKS,
};
use notchling_engine::search::models::manifest;
use notchling_engine::search::pipeline::Pipeline;
use notchling_engine::search::pipeline::{Budget, Paused, Power, BATCH};
use notchling_engine::search::query::category::Category;
use notchling_engine::search::query::filter::Filter;
use notchling_engine::search::range::IndexRange;
use notchling_engine::search::store::{DocHit, Sets, VectorDb};
use notchling_local_api::{ops, topics, ApiError, Caller, OpSpec, Tier};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, Weak};
use std::time::{Duration, Instant};

pub const REFRESH_EVERY: Duration = Duration::from_secs(60);
pub const MAX_SETTINGS_DOCS: usize = 200;
pub const MAX_SETTINGS_CHARS: usize = 4000;
pub const TOP_DOCS: usize = 100;
const READY_POLL: Duration = Duration::from_secs(30);
const READY_STEP: Duration = Duration::from_millis(10);
const REST_CAP: Duration = Duration::from_secs(5);
const STOP_WAIT: Duration = Duration::from_secs(10);
const CLOSE_WAIT: Duration = Duration::from_secs(2);
const OPS: [&str; 5] = [
    ops::EMBED_STATUS,
    ops::EMBED_SETTINGS,
    ops::EMBED_REMOVE,
    ops::EMBED_FETCH,
    ops::EMBED_FETCH_CANCEL,
];
const TOPICS: [&str; 1] = [topics::EMBED];

#[derive(Clone, Debug, PartialEq)]
pub struct EmbedSettings {
    pub on: bool,
    pub paused: bool,
    pub models: Vec<String>,
    pub range: IndexRange,
    pub categories: Vec<Category>,
}

pub trait PowerSource: Send + Sync {
    fn read(&self, user_paused: bool) -> Power;
}

pub struct PluggedIn;

impl PowerSource for PluggedIn {
    fn read(&self, user_paused: bool) -> Power {
        Power {
            on_battery: false,
            low_power: false,
            thermal_serious: false,
            user_paused,
        }
    }
}

pub struct SystemPower;

impl PowerSource for SystemPower {
    fn read(&self, user_paused: bool) -> Power {
        Power {
            user_paused,
            ..Power::from(notchling_platform::power::state())
        }
    }
}

pub type Corpora = Arc<dyn Fn(&EmbedSettings) -> Vec<Arc<dyn Corpus>> + Send + Sync>;
pub type StaticOpen = Arc<dyn Fn() -> Result<Option<Arc<dyn Embedder>>, String> + Send + Sync>;

pub struct EmbedParts {
    pub data_dir: std::path::PathBuf,
    pub room: Arc<dyn Room>,
    pub settings: Arc<dyn Fn() -> EmbedSettings + Send + Sync>,
    pub corpora: Corpora,
    pub spawn: Arc<dyn HelperSpawn>,
    pub granite_id: ModelId,
    pub granite_present: bool,
    pub statics: StaticOpen,
    pub fixture: bool,
    pub power: Arc<dyn PowerSource>,
    pub net: ModelNet,
    pub now: Arc<dyn Fn() -> f64 + Send + Sync>,
}

enum Granite {
    Helper(Arc<HelperEmbedder>),
    Inline(Arc<dyn Embedder>),
}

pub struct Loaded {
    db: Arc<VectorDb>,
    sets: Arc<Sets>,
    pipeline: Pipeline,
    granite: Option<Granite>,
    statics: Option<Arc<dyn Embedder>>,
    statics_problem: Option<String>,
    floors: Floors,
    budget: Mutex<Budget>,
    incomplete: Mutex<Option<bool>>,
    retired: Mutex<bool>,
    working: Mutex<()>,
}

impl Loaded {
    pub(crate) fn retired(&self) -> bool {
        *lock(&self.retired)
    }

    fn retire(&self) {
        *lock(&self.retired) = true;
        self.pipeline.halt();
        if let Some(Granite::Helper(helper)) = &self.granite {
            helper.stop();
        }
    }

    fn stale(&self) {
        *lock(&self.incomplete) = None;
    }

    #[cfg(test)]
    pub(crate) fn passing(&self) -> bool {
        self.working.try_lock().is_err()
    }

    fn incomplete(&self) -> bool {
        let mut held = lock(&self.incomplete);
        *held.get_or_insert_with(|| {
            self.pipeline
                .coverage()
                .iter()
                .any(|(_, rows)| rows.iter().any(|c| c.embedded < c.chunks))
        })
    }
}

impl Drop for Loaded {
    fn drop(&mut self) {
        if let Some(Granite::Helper(helper)) = &self.granite {
            helper.stop();
        }
    }
}

struct Wake {
    stirs: Mutex<u64>,
    signal: Condvar,
}

pub struct EmbedOps {
    parts: EmbedParts,
    fetcher: Arc<Fetcher>,
    loaded: Mutex<Option<Arc<Loaded>>>,
    epoch: AtomicU64,
    settings_docs: Mutex<Vec<(String, String)>>,
    settings_corpus: Arc<SettingsCorpus>,
    last_changed: Mutex<Option<f64>>,
    last_shown: Mutex<Option<(String, String, String)>>,
    error: Mutex<Option<String>>,
    reload: AtomicBool,
    stop_wait: Mutex<Duration>,
    wake: Wake,
    closed: Mutex<bool>,
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

fn state_name(state: Semantic) -> &'static str {
    match state {
        Semantic::Off => "off",
        Semantic::NoModel => "no_model",
        Semantic::Warming => "warming",
        Semantic::Indexing => "indexing",
        Semantic::Ready => "ready",
        Semantic::Paused => "paused",
    }
}

impl EmbedOps {
    pub fn with_parts(parts: EmbedParts) -> Arc<EmbedOps> {
        Arc::new_cyclic(|me: &Weak<EmbedOps>| {
            let announce = me.clone();
            let finished = me.clone();
            let fetcher = Arc::new(Fetcher::new(
                parts.data_dir.clone(),
                parts.net.clone(),
                Arc::new(move |fetch| {
                    if let Some(ops) = announce.upgrade() {
                        ops.announce(fetch);
                    }
                }),
                Arc::new(move |_, ok| {
                    if ok {
                        if let Some(ops) = finished.upgrade() {
                            ops.model_arrived();
                        }
                    }
                }),
            ));
            EmbedOps::assemble(parts, fetcher)
        })
    }

    fn assemble(parts: EmbedParts, fetcher: Arc<Fetcher>) -> EmbedOps {
        EmbedOps {
            parts,
            fetcher,
            loaded: Mutex::new(None),
            epoch: AtomicU64::new(0),
            settings_docs: Mutex::new(Vec::new()),
            settings_corpus: Arc::new(SettingsCorpus::new()),
            last_changed: Mutex::new(None),
            last_shown: Mutex::new(None),
            error: Mutex::new(None),
            reload: AtomicBool::new(false),
            stop_wait: Mutex::new(STOP_WAIT),
            wake: Wake {
                stirs: Mutex::new(0),
                signal: Condvar::new(),
            },
            closed: Mutex::new(false),
        }
    }

    pub fn loaded_weak(&self) -> Weak<Loaded> {
        match &*lock(&self.loaded) {
            Some(loaded) => Arc::downgrade(loaded),
            None => Weak::new(),
        }
    }

    #[cfg(test)]
    pub(crate) fn epoch(&self) -> u64 {
        self.epoch.load(Ordering::SeqCst)
    }

    fn current(&self) -> Option<Arc<Loaded>> {
        lock(&self.loaded).clone()
    }

    fn retire(&self) -> Option<Arc<Loaded>> {
        let old = {
            let mut slot = lock(&self.loaded);
            self.epoch.fetch_add(1, Ordering::SeqCst);
            slot.take()
        };
        if let Some(old) = &old {
            old.retire();
        }
        old
    }

    fn settle(&self, loaded: &Loaded) {
        drop(lock(&loaded.working));
    }

    fn seat(&self, loaded: Arc<Loaded>, epoch: u64) -> Option<Arc<Loaded>> {
        if !(self.parts.settings)().on {
            loaded.retire();
            return None;
        }
        let mut slot = lock(&self.loaded);
        if self.closed() || self.epoch.load(Ordering::SeqCst) != epoch {
            drop(slot);
            loaded.retire();
            return None;
        }
        let old = slot.replace(loaded.clone());
        drop(slot);
        if let Some(old) = old {
            old.retire();
        }
        Some(loaded)
    }

    fn open(&self) -> Result<Arc<Loaded>, String> {
        let db = Arc::new(VectorDb::open(&embed_paths::vectors_db(
            &self.parts.data_dir,
        ))?);
        let sets = Arc::new(Sets::default());
        let pipeline = Pipeline::new(db.clone(), sets.clone());
        let (statics, statics_problem, statics_floor) = if self.parts.fixture {
            (
                Some(Arc::new(HashEmbedder::new(embed_paths::STATIC_ID)) as Arc<dyn Embedder>),
                None,
                0.0,
            )
        } else {
            match (self.parts.statics)() {
                Ok(opened) => (opened, None, FLOOR_STATIC),
                Err(problem) => (None, Some(problem), FLOOR_STATIC),
            }
        };
        let (granite, granite_floor) = if self.parts.fixture {
            (
                Some(Granite::Inline(Arc::new(HashEmbedder::new(
                    &self.parts.granite_id.0,
                )))),
                0.0,
            )
        } else if self.parts.granite_present || embed_paths::granite_present(&self.parts.data_dir) {
            (
                Some(Granite::Helper(Arc::new(HelperEmbedder::new(
                    self.parts.spawn.clone(),
                    self.parts.granite_id.clone(),
                )))),
                FLOOR_GRANITE,
            )
        } else {
            (None, FLOOR_GRANITE)
        };
        let mut now: Vec<Arc<dyn Embedder>> = statics.iter().cloned().collect();
        let mut batched = Vec::new();
        match &granite {
            Some(Granite::Inline(e)) => now.push(e.clone()),
            Some(Granite::Helper(h)) => batched.push(h.model().clone()),
            None => {}
        }
        pipeline.set_models(now, batched);
        Ok(Arc::new(Loaded {
            db,
            sets,
            pipeline,
            granite,
            statics,
            statics_problem,
            floors: Floors {
                granite: granite_floor,
                statics: statics_floor,
            },
            budget: Mutex::new(Budget::new()),
            incomplete: Mutex::new(None),
            retired: Mutex::new(false),
            working: Mutex::new(()),
        }))
    }

    fn reconcile(&self, settings: &EmbedSettings) -> Option<Arc<Loaded>> {
        if !settings.on {
            self.fetcher.cancel_all();
            self.retire();
            *lock(&self.error) = None;
            return None;
        }
        let reload = self.reload.swap(false, Ordering::SeqCst);
        if let Some(loaded) = self.current().filter(|_| !reload) {
            return Some(loaded);
        }
        let epoch = self.epoch.load(Ordering::SeqCst);
        match self.open() {
            Ok(loaded) => {
                *lock(&self.error) = None;
                let seated = self.seat(loaded, epoch);
                if seated.is_none() && reload {
                    self.reload.store(true, Ordering::SeqCst);
                }
                seated
            }
            Err(e) => {
                *lock(&self.error) = Some(e);
                if reload {
                    self.reload.store(true, Ordering::SeqCst);
                }
                None
            }
        }
    }

    pub fn tick(&self) {
        if self.closed() {
            return;
        }
        let since = self.stirs();
        let settings = (self.parts.settings)();
        let Some(loaded) = self.reconcile(&settings) else {
            self.publish_changed();
            return;
        };
        self.recovered(&loaded);
        if !settings.paused && !self.halted(&loaded, since) {
            let docs = lock(&self.settings_docs).clone();
            self.settings_corpus.set(docs);
            let mut corpora = (self.parts.corpora)(&settings);
            if settings.categories.contains(&Category::Settings) {
                corpora.push(self.settings_corpus.clone());
            }
            {
                let _working = lock(&loaded.working);
                if !loaded.retired() {
                    loaded.pipeline.pass(
                        &corpora,
                        settings.range.window_secs(),
                        (self.parts.now)(),
                    );
                    loaded.stale();
                }
            }
            self.feed(&loaded, since);
        }
        self.publish_changed();
    }

    pub fn settings_corpus(&self) -> Arc<SettingsCorpus> {
        self.settings_corpus.clone()
    }

    #[cfg(test)]
    pub(crate) fn fetcher(&self) -> &Arc<Fetcher> {
        &self.fetcher
    }

    #[cfg(test)]
    pub(crate) fn helper_ready(&self) -> bool {
        match self.current().as_deref().and_then(|l| l.granite.as_ref()) {
            Some(Granite::Helper(h)) => h.state() == HelperState::Ready,
            _ => false,
        }
    }

    #[cfg(test)]
    pub(crate) fn set_stop_wait(&self, wait: Duration) {
        *lock(&self.stop_wait) = wait;
    }

    fn closed(&self) -> bool {
        *lock(&self.closed)
    }

    fn halted(&self, loaded: &Loaded, since: u64) -> bool {
        self.closed() || loaded.retired() || self.stirs() != since
    }

    fn await_ready(&self, helper: &HelperEmbedder, loaded: &Loaded, since: u64) -> bool {
        let started = Instant::now();
        while started.elapsed() < READY_POLL && !self.halted(loaded, since) {
            match helper.state() {
                HelperState::Ready => {
                    *lock(&self.error) = None;
                    return true;
                }
                HelperState::Failed(e) => {
                    *lock(&self.error) = Some(e);
                    return false;
                }
                _ => {
                    self.rest(since, READY_STEP);
                }
            }
        }
        false
    }

    fn recovered(&self, loaded: &Loaded) {
        if let Some(Granite::Helper(helper)) = &loaded.granite {
            if helper.state() == HelperState::Ready && !loaded.retired() {
                *lock(&self.error) = None;
            }
        }
    }

    fn feeding(&self, loaded: &Loaded, since: u64) -> Option<Power> {
        if self.halted(loaded, since) {
            return None;
        }
        let settings = (self.parts.settings)();
        let power = self.parts.power.read(settings.paused);
        (settings.on && lock(&loaded.budget).paused(&power) == Paused::No).then_some(power)
    }

    fn keep(&self, loaded: &Loaded, model: &ModelId, rows: Vec<([u8; 16], Vec<f32>)>) -> bool {
        let retired = lock(&loaded.retired);
        if *retired {
            return false;
        }
        loaded.pipeline.accept(model, rows);
        true
    }

    fn feed(&self, loaded: &Loaded, since: u64) {
        let Some(Granite::Helper(helper)) = &loaded.granite else {
            return;
        };
        let queue = loaded.pipeline.queue();
        while queue.due((self.parts.now)()) {
            let Some(power) = self.feeding(loaded, since) else {
                break;
            };
            let Some(model) = queue.next_model() else {
                break;
            };
            helper.warm();
            if !self.await_ready(helper, loaded, since) {
                break;
            }
            let rows = queue.take(BATCH);
            if rows.is_empty() {
                break;
            }
            let hashes: Vec<[u8; 16]> = rows.iter().map(|(h, _)| *h).collect();
            let texts: Vec<&str> = rows.iter().map(|(_, t)| t.as_str()).collect();
            let started = Instant::now();
            let kept = match helper.embed_batch(&texts) {
                Ok(vectors) if vectors.len() == rows.len() => {
                    *lock(&self.error) = None;
                    self.keep(
                        loaded,
                        &model,
                        hashes.iter().copied().zip(vectors).collect(),
                    )
                }
                Ok(_) => false,
                Err(e) => {
                    if !loaded.retired() && !self.closed() {
                        *lock(&self.error) = Some(format!("{e:?}"));
                    }
                    false
                }
            };
            if !kept {
                queue.release(&model, &hashes);
                break;
            }
            loaded.stale();
            let wall = started.elapsed();
            let rest = lock(&loaded.budget).rest_after(wall, wall, &power);
            if self.rest(since, rest.min(REST_CAP)) {
                break;
            }
        }
        if !loaded.retired() {
            loaded.pipeline.flush();
        }
    }

    pub fn publish_changed(&self) {
        let now = (self.parts.now)();
        let status = self.status();
        let shown = (
            status["state"].as_str().unwrap_or_default().to_string(),
            status["budget"]["paused"]
                .as_str()
                .unwrap_or_default()
                .to_string(),
            status["corpora"].to_string(),
        );
        {
            let mut last = lock(&self.last_changed);
            let mut seen = lock(&self.last_shown);
            if seen.as_ref() == Some(&shown)
                && last.is_some_and(|at| (0.0..1.0).contains(&(now - at)))
            {
                return;
            }
            *last = Some(now);
            *seen = Some(shown);
        }
        self.send_changed_with(&status, Value::Null);
    }

    fn send_changed(&self, fetch: Value) {
        self.send_changed_with(&self.status(), fetch);
    }

    fn send_changed_with(&self, status: &Value, fetch: Value) {
        self.parts.room.publish(
            topics::EMBED,
            topics::EMBED_CHANGED,
            &json!({"state": status["state"], "coverage": status["corpora"], "fetch": fetch}),
        );
    }

    pub fn announce(&self, fetch: Value) {
        self.send_changed(fetch);
    }

    pub(crate) fn model_arrived(&self) {
        self.reload.store(true, Ordering::SeqCst);
        self.refresh();
    }

    pub(crate) fn stirs(&self) -> u64 {
        *lock(&self.wake.stirs)
    }

    pub(crate) fn rest(&self, since: u64, rest: Duration) -> bool {
        let stirs = lock(&self.wake.stirs);
        let (stirs, _) = self
            .wake
            .signal
            .wait_timeout_while(stirs, rest, |now| *now == since)
            .unwrap_or_else(|e| e.into_inner());
        *stirs != since
    }

    pub fn refresh(&self) {
        *lock(&self.wake.stirs) += 1;
        self.wake.signal.notify_all();
    }

    pub fn start(self: &Arc<Self>) {
        let me = Arc::downgrade(self);
        std::thread::spawn(move || {
            let _ = notchling_platform::power::background_thread(true);
            Self::work(me)
        });
    }

    fn work(me: Weak<Self>) {
        loop {
            let Some(ops) = me.upgrade() else {
                return;
            };
            if ops.closed() {
                return;
            }
            let since = ops.stirs();
            ops.tick();
            ops.rest(since, REFRESH_EVERY);
        }
    }

    pub fn close(&self) {
        *lock(&self.closed) = true;
        self.refresh();
        self.retire();
        self.fetcher.cancel_all();
        self.fetcher.wait_idle(CLOSE_WAIT);
    }

    fn semantic(&self, settings: &EmbedSettings, loaded: &Loaded) -> Semantic {
        if !settings.on {
            return Semantic::Off;
        }
        if loaded.statics.is_none() && loaded.granite.is_none() {
            return Semantic::NoModel;
        }
        if settings.paused {
            return Semantic::Paused;
        }
        if let Some(Granite::Helper(h)) = &loaded.granite {
            if h.state() != HelperState::Ready {
                return Semantic::Warming;
            }
        }
        if loaded.incomplete() {
            Semantic::Indexing
        } else {
            Semantic::Ready
        }
    }

    pub fn chunk_text(&self, hash: &[u8; 16]) -> Option<String> {
        self.current()?.db.chunk_text(hash)
    }

    pub fn semantic_json(&self, state: Semantic) -> Value {
        let (chunks, vectors) = match self.current().filter(|_| state != Semantic::Off) {
            Some(loaded) => {
                let stats = loaded.db.stats();
                (stats.chunks, stats.vectors)
            }
            None => (0, 0),
        };
        json!({"state": state_name(state), "coverage": {"chunks": chunks, "vectors": vectors}})
    }

    pub fn state(&self) -> Semantic {
        let settings = (self.parts.settings)();
        match self.current() {
            Some(loaded) if settings.on => self.semantic(&settings, &loaded),
            _ if settings.on => Semantic::NoModel,
            _ => Semantic::Off,
        }
    }

    pub fn query_lists(
        &self,
        query: &str,
        filter: &Filter,
        corpora: &[&str],
    ) -> (Semantic, HashMap<String, Lists>) {
        self.query_lists_within(query, filter, corpora, &|_, _| true)
    }

    pub fn query_lists_within(
        &self,
        query: &str,
        filter: &Filter,
        corpora: &[&str],
        allow: &dyn Fn(&str, &str) -> bool,
    ) -> (Semantic, HashMap<String, Lists>) {
        let settings = (self.parts.settings)();
        let Some(loaded) = self.current().filter(|_| settings.on) else {
            return (
                if settings.on {
                    Semantic::NoModel
                } else {
                    Semantic::Off
                },
                HashMap::new(),
            );
        };
        let mut state = self.semantic(&settings, &loaded);
        let mut out: HashMap<String, Lists> = corpora
            .iter()
            .map(|c| {
                (
                    c.to_string(),
                    Lists {
                        floors: loaded.floors,
                        ..Lists::default()
                    },
                )
            })
            .collect();
        let keep = |corpus: &str, key: &str, facets: &[(&str, &str)], at: f64| {
            corpora.contains(&corpus) && allow(corpus, key) && filter.matches(facets, Some(at))
        };
        let scan = |embedder: &dyn Embedder, floor: f32| -> Option<Vec<DocHit>> {
            let vector = embedder.embed(&[query]).ok()?.pop()?;
            let set = loaded.sets.get(embedder.model())?;
            Some(set.scan(&vector, TOP_CHUNKS, Some(&set.mask_keyed(&keep)), floor))
        };
        let spread = |hits: Vec<DocHit>, out: &mut HashMap<String, Lists>, granite: bool| {
            for list in out.values_mut() {
                let slot = if granite {
                    &mut list.granite
                } else {
                    &mut list.statics
                };
                slot.get_or_insert_with(Vec::new);
            }
            for hit in hits {
                if let Some(list) = out.get_mut(&hit.corpus) {
                    let slot = if granite {
                        &mut list.granite
                    } else {
                        &mut list.statics
                    };
                    if let Some(v) = slot.as_mut().filter(|v| v.len() < TOP_DOCS) {
                        v.push(hit);
                    }
                }
            }
        };
        if let Some(embedder) = &loaded.statics {
            if let Some(hits) = scan(embedder.as_ref(), loaded.floors.statics) {
                spread(hits, &mut out, false);
            }
        }
        match &loaded.granite {
            Some(Granite::Inline(embedder)) => {
                if let Some(hits) = scan(embedder.as_ref(), loaded.floors.granite) {
                    spread(hits, &mut out, true);
                }
            }
            Some(Granite::Helper(helper)) => match helper.embed_query(query) {
                Ok(vector) => {
                    self.recovered(&loaded);
                    if let Some(set) = loaded.sets.get(helper.model()) {
                        let hits = set.scan(
                            &vector,
                            TOP_CHUNKS,
                            Some(&set.mask_keyed(&keep)),
                            loaded.floors.granite,
                        );
                        spread(hits, &mut out, true);
                    }
                }
                Err(_) => {
                    if matches!(state, Semantic::Ready | Semantic::Indexing) {
                        state = Semantic::Warming;
                    }
                }
            },
            None => {}
        }
        if !(self.parts.settings)().on {
            return (Semantic::Off, HashMap::new());
        }
        (state, out)
    }

    pub fn status(&self) -> Value {
        let settings = (self.parts.settings)();
        let loaded = self.current().filter(|_| settings.on);
        let state = match &loaded {
            Some(l) => self.semantic(&settings, l),
            None if settings.on => Semantic::NoModel,
            None => Semantic::Off,
        };
        let data = &self.parts.data_dir;
        let models = vec![
            json!({"id": embed_paths::GRANITE_ID, "present": embed_paths::granite_present(data) || self.parts.granite_present, "verified": false}),
            json!({"id": embed_paths::STATIC_ID, "present": embed_paths::static_present(data), "verified": false}),
        ];
        let (mut corpora, mut budget) = (
            Vec::<Value>::new(),
            json!({"cpu_ms": 0, "wall_ms": 0, "paused": "no"}),
        );
        if let Some(loaded) = &loaded {
            let mut rows: HashMap<String, (usize, usize, serde_json::Map<String, Value>)> =
                HashMap::new();
            for (model, coverage) in loaded.pipeline.coverage() {
                for c in coverage {
                    let row = rows.entry(c.corpus.clone()).or_default();
                    row.0 = c.docs;
                    row.1 = c.chunks;
                    row.2.insert(model.0.clone(), json!(c.embedded));
                }
            }
            let mut names: Vec<&String> = rows.keys().collect();
            names.sort();
            for name in names {
                let (docs, chunks, embedded) = &rows[name];
                corpora.push(
                    json!({"corpus": name, "docs": docs, "chunks": chunks, "embedded": embedded}),
                );
            }
            let (cpu, wall) = lock(&loaded.budget).totals();
            let power = self.parts.power.read(settings.paused);
            let paused = match lock(&loaded.budget).paused(&power) {
                Paused::No => "no",
                Paused::User => "user",
                Paused::LowPower => "low_power",
                Paused::Thermal => "thermal",
            };
            budget = json!({"cpu_ms": cpu.as_millis() as u64, "wall_ms": wall.as_millis() as u64, "paused": paused});
        }
        let error = lock(&self.error)
            .clone()
            .or_else(|| loaded.as_ref().and_then(|l| l.statics_problem.clone()));
        json!({
            "state": state_name(state),
            "models": models,
            "corpora": corpora,
            "paused": settings.paused,
            "budget": budget,
            "error": error,
            "backlog": loaded.as_ref().map(|l| l.pipeline.queue().len()).unwrap_or(0),
            "stored": loaded.as_ref().map(|l| l.db.stats().vectors).unwrap_or(0),
            "fetch": self.fetcher.list(),
        })
    }

    fn store_settings(&self, args: &Value) -> Result<Value, ApiError> {
        let usage = || {
            ApiError::bad_request(
                "embed.settings takes {\"docs\": [{\"id\": string, \"text\": string}]}",
            )
        };
        let docs = args
            .get("docs")
            .and_then(Value::as_array)
            .ok_or_else(usage)?;
        if docs.len() > MAX_SETTINGS_DOCS {
            return Err(ApiError::bad_request(format!(
                "at most {MAX_SETTINGS_DOCS} docs"
            )));
        }
        let mut stored = Vec::with_capacity(docs.len());
        for doc in docs {
            let id = doc.get("id").and_then(Value::as_str).ok_or_else(usage)?;
            let text = doc.get("text").and_then(Value::as_str).ok_or_else(usage)?;
            if text.chars().count() > MAX_SETTINGS_CHARS {
                return Err(ApiError::bad_request(format!(
                    "at most {MAX_SETTINGS_CHARS} characters a doc"
                )));
            }
            stored.push((id.to_string(), text.to_string()));
        }
        let n = stored.len();
        *lock(&self.settings_docs) = stored;
        self.refresh();
        Ok(json!({"stored": n}))
    }

    fn fetch(&self, args: &Value) -> Result<Value, ApiError> {
        let entry = entry_of(args, ops::EMBED_FETCH)?;
        if !(self.parts.settings)().on {
            return Err(off_error());
        }
        self.fetcher.start(entry)?;
        Ok(json!({"state": "downloading"}))
    }

    fn fetch_cancel(&self, args: &Value) -> Result<Value, ApiError> {
        let entry = entry_of(args, ops::EMBED_FETCH_CANCEL)?;
        Ok(json!({"cancelled": self.fetcher.cancel(&entry.id)}))
    }

    fn remove(&self, args: &Value) -> Result<Value, ApiError> {
        let model = args
            .get("model")
            .and_then(Value::as_str)
            .ok_or_else(|| ApiError::bad_request("embed.remove takes {\"model\": string}"))?;
        let id = ModelId(model.to_string());
        let entry = manifest().model(model);
        let (deleted, loaded) = match entry {
            Some(entry) => {
                let early = self.retire();
                let wait = *lock(&self.stop_wait);
                let deleted = match self.fetcher.remove(entry, wait) {
                    Ok(deleted) => deleted,
                    Err(e) => {
                        self.refresh();
                        return Err(e);
                    }
                };
                let second = self.retire();
                if let Some(l) = &early {
                    self.settle(l);
                }
                if let Some(l) = &second {
                    self.settle(l);
                }
                (deleted, second.or(early))
            }
            None => (false, self.current()),
        };
        let dropped = match loaded {
            Some(loaded) => {
                let dropped = loaded.db.drop_model(&id).is_ok();
                loaded.sets.remove(&id);
                loaded.stale();
                dropped
            }
            None => {
                let path = embed_paths::vectors_db(&self.parts.data_dir);
                path.is_file() && VectorDb::open(&path).is_ok_and(|db| db.drop_model(&id).is_ok())
            }
        };
        if entry.is_some() {
            if let Some(late) = self.retire() {
                self.settle(&late);
            }
            self.refresh();
        }
        self.send_changed(progress_json(model, "absent", 0, 0, None));
        Ok(json!({"removed": dropped || deleted}))
    }
}

impl OpGroup for EmbedOps {
    fn spec(&self, op: &str) -> Option<OpSpec> {
        match op {
            ops::EMBED_STATUS => Some(OpSpec::AGENT_READ),
            ops::EMBED_SETTINGS
            | ops::EMBED_REMOVE
            | ops::EMBED_FETCH
            | ops::EMBED_FETCH_CANCEL => Some(OpSpec::APP),
            _ => None,
        }
    }

    fn call(&self, caller: &Caller, op: &str, args: &Value) -> Result<Value, ApiError> {
        match op {
            ops::EMBED_STATUS => Ok(self.status_for(caller)),
            ops::EMBED_SETTINGS => self.store_settings(args),
            ops::EMBED_REMOVE => self.remove(args),
            ops::EMBED_FETCH => self.fetch(args),
            ops::EMBED_FETCH_CANCEL => self.fetch_cancel(args),
            other => Err(ApiError::internal(format!("{other} has no handler"))),
        }
    }

    fn topics(&self) -> &[&'static str] {
        &TOPICS
    }

    fn topic_tier(&self, topic: &str) -> Option<Tier> {
        TOPICS.contains(&topic).then_some(Tier::App)
    }
}

impl EmbedOps {
    pub fn ops() -> &'static [&'static str] {
        &OPS
    }
}
