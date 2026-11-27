use super::queue::{Queue, WAKE_AT};
use crate::search::corpus::{Corpus, DocStamp};
use crate::search::embed::{Embedder, ModelId};
use crate::search::store::{Coverage, Sets, VecFormat, VectorDb, VectorSet};
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

pub(super) const EMBED_CHUNK: usize = 32;
const PUSH_AHEAD: usize = 256;
const SWAP_AFTER: usize = 512;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PassStats {
    pub read: usize,
    pub reused: usize,
    pub removed: usize,
    pub queued: usize,
    pub embedded_now: usize,
}

#[derive(Default)]
struct Models {
    now: Vec<Arc<dyn Embedder>>,
    batched: Vec<ModelId>,
}

pub struct Pipeline {
    db: Arc<VectorDb>,
    sets: Arc<Sets>,
    queue: Queue,
    models: Mutex<Models>,
    last_now: AtomicU64,
    unswapped: Mutex<HashMap<ModelId, usize>>,
    halted: AtomicBool,
}

fn rank(id: &str) -> u8 {
    match id {
        "settings" => 0,
        "sessions" => 1,
        "pages" => 2,
        _ => 3,
    }
}

impl Pipeline {
    pub fn new(db: Arc<VectorDb>, sets: Arc<Sets>) -> Pipeline {
        Pipeline {
            db,
            sets,
            queue: Queue::new(),
            models: Mutex::new(Models::default()),
            last_now: AtomicU64::new(0f64.to_bits()),
            unswapped: Mutex::new(HashMap::new()),
            halted: AtomicBool::new(false),
        }
    }

    pub fn halt(&self) {
        self.halted.store(true, Ordering::SeqCst);
    }

    fn is_halted(&self) -> bool {
        self.halted.load(Ordering::SeqCst)
    }

    pub fn set_models(&self, now_embed: Vec<Arc<dyn Embedder>>, batched: Vec<ModelId>) {
        let mut models = self.models.lock().unwrap_or_else(|e| e.into_inner());
        let kept: HashSet<ModelId> = batched.iter().cloned().collect();
        for old in &models.batched {
            if !kept.contains(old) {
                self.queue.drop_model(old);
            }
        }
        models.now = now_embed;
        models.batched = batched;
    }

    pub fn queue(&self) -> &Queue {
        &self.queue
    }

    fn active(&self) -> (Vec<Arc<dyn Embedder>>, Vec<ModelId>) {
        let models = self.models.lock().unwrap_or_else(|e| e.into_inner());
        (models.now.clone(), models.batched.clone())
    }

    fn active_ids(&self) -> Vec<ModelId> {
        let (now, batched) = self.active();
        let mut ids: Vec<ModelId> = now.iter().map(|e| e.model().clone()).collect();
        ids.extend(batched);
        ids
    }

    pub fn pass(&self, corpora: &[Arc<dyn Corpus>], window: Option<f64>, now: f64) -> PassStats {
        self.last_now.store(now.to_bits(), Ordering::SeqCst);
        let mut stats = PassStats::default();
        if self.is_halted() {
            return stats;
        }
        let (now_models, batched) = self.active();
        let mut changed = false;
        let mut ordered: Vec<&Arc<dyn Corpus>> = corpora.iter().collect();
        ordered.sort_by_key(|c| rank(c.id()));
        for corpus in &ordered {
            if self.is_halted() {
                return stats;
            }
            changed |= self.sync(corpus.as_ref(), window, now, &mut stats);
        }
        if self.is_halted() {
            return stats;
        }
        let passed: HashSet<&str> = corpora.iter().map(|c| c.id()).collect();
        for id in self.db.corpora() {
            if passed.contains(id.as_str()) {
                continue;
            }
            if self.is_halted() {
                return stats;
            }
            let keys: Vec<String> = self.db.stamps(&id).into_keys().collect();
            stats.removed += keys.len();
            if self.db.remove_docs(&id, &keys).is_ok() {
                changed = true;
            }
        }
        if self.is_halted() {
            return stats;
        }
        if changed {
            let _ = self.db.sweep();
            let pending = self.queue.pending_hashes();
            if !pending.is_empty() {
                let live = self.db.live(&pending);
                self.queue.prune(&live);
            }
        }
        let mut refresh = changed;
        for embedder in &now_models {
            if self.is_halted() {
                return stats;
            }
            let n = self.embed_missing(embedder.as_ref());
            stats.embedded_now += n;
            refresh |= n > 0;
        }
        if self.is_halted() {
            return stats;
        }
        for model in &batched {
            stats.queued += self.fill_queue(model, now);
        }
        self.settle_backlog(&batched);
        self.reload(refresh);
        self.retire();
        stats
    }

    fn sync(
        &self,
        corpus: &dyn Corpus,
        window: Option<f64>,
        now: f64,
        stats: &mut PassStats,
    ) -> bool {
        let id = corpus.id();
        let mut found: Vec<DocStamp> = corpus.scan(window, now);
        found.sort_by(|a, b| b.at.total_cmp(&a.at));
        let known = self.db.stamps(id);
        let listed: HashSet<&str> = found.iter().map(|s| s.key.as_str()).collect();
        let mut changed = false;
        for stamp in &found {
            if known.get(&stamp.key) == Some(&stamp.stamp) {
                stats.reused += 1;
                continue;
            }
            let Some(doc) = corpus.read(stamp, now) else {
                continue;
            };
            stats.read += 1;
            if self.db.put_doc(id, &stamp.stamp, &doc).is_ok() {
                changed = true;
            }
        }
        let gone: Vec<String> = known
            .keys()
            .filter(|k| !listed.contains(k.as_str()))
            .cloned()
            .collect();
        if !gone.is_empty() {
            stats.removed += gone.len();
            if self.db.remove_docs(id, &gone).is_ok() {
                changed = true;
            }
        }
        changed
    }

    fn embed_missing(&self, embedder: &dyn Embedder) -> usize {
        let model = embedder.model().clone();
        let mut failed: HashSet<[u8; 16]> = HashSet::new();
        let mut total = 0;
        loop {
            if self.is_halted() {
                return total;
            }
            let rows: Vec<([u8; 16], String)> = self
                .db
                .missing(&model, EMBED_CHUNK + failed.len())
                .into_iter()
                .filter(|(h, _)| !failed.contains(h))
                .take(EMBED_CHUNK)
                .collect();
            if rows.is_empty() {
                return total;
            }
            let put = self.embed_rows(embedder, &rows, &mut failed);
            if put.is_empty() || self.is_halted() {
                return total;
            }
            if self.db.put_vectors(&model, VecFormat::Q8, &put).is_err() {
                return total;
            }
            total += put.len();
        }
    }

    fn embed_rows(
        &self,
        embedder: &dyn Embedder,
        rows: &[([u8; 16], String)],
        failed: &mut HashSet<[u8; 16]>,
    ) -> Vec<([u8; 16], Vec<f32>)> {
        let texts: Vec<&str> = rows.iter().map(|(_, t)| t.as_str()).collect();
        if let Ok(vectors) = embedder.embed(&texts) {
            if vectors.len() == rows.len() {
                return rows.iter().map(|(h, _)| *h).zip(vectors).collect();
            }
        }
        let mut put = Vec::new();
        for (hash, text) in rows {
            if self.is_halted() {
                return put;
            }
            match embedder.embed(&[text.as_str()]) {
                Ok(mut v) if v.len() == 1 => put.push((*hash, v.remove(0))),
                _ => {
                    failed.insert(*hash);
                }
            }
        }
        put
    }

    fn fill_queue(&self, model: &ModelId, now: f64) -> usize {
        let limit = self.queue.held(model) + PUSH_AHEAD;
        let rows = self.db.missing(model, limit);
        self.queue.push(model, rows, now)
    }

    fn settle_backlog(&self, batched: &[ModelId]) {
        let any_missing = batched.iter().any(|m| !self.db.missing(m, 1).is_empty());
        let bulk = batched
            .iter()
            .any(|m| self.db.missing(m, WAKE_AT).len() >= WAKE_AT);
        self.queue
            .set_backlog((self.queue.backlog() && any_missing) || bulk);
    }

    fn reload(&self, force: bool) {
        let mut ids = self.active_ids();
        for model in self.db.models() {
            if !ids.contains(&model) {
                ids.push(model);
            }
        }
        for model in ids {
            let stale = self.take_unswapped(&model) > 0;
            if force || stale || self.sets.get(&model).is_none() {
                self.sets.swap(&model, VectorSet::load(&self.db, &model));
            }
        }
    }

    fn take_unswapped(&self, model: &ModelId) -> usize {
        self.unswapped
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(model)
            .unwrap_or(0)
    }

    fn swap_now(&self, model: &ModelId) {
        self.take_unswapped(model);
        self.sets.swap(model, VectorSet::load(&self.db, model));
    }

    fn note_unswapped(&self, model: &ModelId, rows: usize) -> usize {
        let mut held = self.unswapped.lock().unwrap_or_else(|e| e.into_inner());
        let n = held.entry(model.clone()).or_insert(0);
        *n += rows;
        *n
    }

    fn refill(&self, batched: &[ModelId]) {
        if !self.queue.backlog() || self.queue.len() >= WAKE_AT {
            return;
        }
        let now = f64::from_bits(self.last_now.load(Ordering::SeqCst));
        for model in batched {
            self.fill_queue(model, now);
        }
    }

    pub fn flush(&self) {
        let pending: Vec<ModelId> = self
            .unswapped
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .keys()
            .cloned()
            .collect();
        for model in pending {
            self.swap_now(&model);
        }
    }

    fn retire(&self) {
        let active = self.active_ids();
        if active.is_empty() {
            return;
        }
        if active.iter().any(|m| !self.db.missing(m, 1).is_empty()) {
            return;
        }
        for model in self.db.models() {
            if active.contains(&model) {
                continue;
            }
            if self.db.drop_model(&model).is_ok() {
                self.sets.remove(&model);
            }
        }
    }

    pub fn accept(&self, model: &ModelId, mut rows: Vec<([u8; 16], Vec<f32>)>) {
        let hashes: Vec<[u8; 16]> = rows.iter().map(|(h, _)| *h).collect();
        if self.is_halted() {
            self.queue.release(model, &hashes);
            return;
        }
        let live = self.db.live(&hashes);
        rows.retain(|(h, _)| live.contains(h));
        if self.is_halted() {
            self.queue.release(model, &hashes);
            return;
        }
        let stored = !rows.is_empty() && self.db.put_vectors(model, VecFormat::F16, &rows).is_ok();
        self.queue.release(model, &hashes);
        let (_, batched) = self.active();
        self.settle_backlog(&batched);
        if stored {
            let waiting = self.note_unswapped(model, rows.len());
            if !self.queue.backlog() || waiting >= SWAP_AFTER {
                self.swap_now(model);
            }
        }
        self.refill(&batched);
        self.retire();
    }

    pub fn coverage(&self) -> Vec<(ModelId, Vec<Coverage>)> {
        self.active_ids()
            .into_iter()
            .map(|m| {
                let coverage = self.db.coverage(&m);
                (m, coverage)
            })
            .collect()
    }
}
