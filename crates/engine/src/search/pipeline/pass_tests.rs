use super::pass::EMBED_CHUNK;
use super::*;
use crate::search::chunk::{hash, ChunkKind, ChunkText};
use crate::search::corpus::{Corpus, Doc, DocStamp, Tier};
use crate::search::embed::{EmbedError, Embedder, HashEmbedder, ModelId, TableEmbedder};
use crate::search::store::{Sets, VectorDb};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

type Log = Arc<Mutex<Vec<String>>>;

struct FakeCorpus {
    id: &'static str,
    docs: Mutex<Vec<(String, String, Vec<String>)>>,
    reads: AtomicUsize,
    log: Log,
}

impl FakeCorpus {
    fn new(id: &'static str, log: &Log) -> Arc<FakeCorpus> {
        Arc::new(FakeCorpus {
            id,
            docs: Mutex::new(Vec::new()),
            reads: AtomicUsize::new(0),
            log: log.clone(),
        })
    }

    fn set(&self, key: &str, stamp: &str, texts: &[&str]) {
        let mut docs = self.docs.lock().unwrap();
        docs.retain(|(k, _, _)| k != key);
        docs.push((
            key.to_string(),
            stamp.to_string(),
            texts.iter().map(|t| t.to_string()).collect(),
        ));
    }

    fn remove(&self, key: &str) {
        self.docs.lock().unwrap().retain(|(k, _, _)| k != key);
    }

    fn reads(&self) -> usize {
        self.reads.load(Ordering::SeqCst)
    }
}

impl Corpus for FakeCorpus {
    fn id(&self) -> &'static str {
        self.id
    }

    fn tier(&self) -> Tier {
        Tier::Agent
    }

    fn scan(&self, _window: Option<f64>, _now: f64) -> Vec<DocStamp> {
        self.docs
            .lock()
            .unwrap()
            .iter()
            .enumerate()
            .map(|(i, (key, stamp, _))| DocStamp {
                key: key.clone(),
                stamp: stamp.clone(),
                at: i as f64,
            })
            .collect()
    }

    fn read(&self, doc: &DocStamp, _now: f64) -> Option<Doc> {
        self.reads.fetch_add(1, Ordering::SeqCst);
        self.log
            .lock()
            .unwrap()
            .push(format!("{}:{}", self.id, doc.key));
        let docs = self.docs.lock().unwrap();
        let (key, _, texts) = docs.iter().find(|(k, _, _)| *k == doc.key)?;
        Some(Doc {
            key: key.clone(),
            title: key.clone(),
            at: doc.at,
            facets: Vec::new(),
            chunks: texts
                .iter()
                .map(|t| ChunkText {
                    kind: ChunkKind::Turn,
                    text: t.clone(),
                })
                .collect(),
        })
    }
}

struct Recording {
    inner: HashEmbedder,
    seen: Mutex<Vec<String>>,
}

impl Recording {
    fn new(model: &str) -> Arc<Recording> {
        Arc::new(Recording {
            inner: HashEmbedder::new(model),
            seen: Mutex::new(Vec::new()),
        })
    }

    fn take(&self) -> Vec<String> {
        let mut seen = self.seen.lock().unwrap();
        let mut out = std::mem::take(&mut *seen);
        out.sort();
        out
    }
}

impl Embedder for Recording {
    fn model(&self) -> &ModelId {
        self.inner.model()
    }

    fn dims(&self) -> usize {
        self.inner.dims()
    }

    fn embed(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>, EmbedError> {
        self.seen
            .lock()
            .unwrap()
            .extend(texts.iter().map(|t| t.to_string()));
        self.inner.embed(texts)
    }
}

fn setup() -> (Arc<VectorDb>, Arc<Sets>, Pipeline, Log) {
    let db = Arc::new(VectorDb::memory().unwrap());
    let sets = Arc::new(Sets::default());
    let pipeline = Pipeline::new(db.clone(), sets.clone());
    (db, sets, pipeline, Log::default())
}

fn as_corpora(list: &[&Arc<FakeCorpus>]) -> Vec<Arc<dyn Corpus>> {
    list.iter()
        .map(|c| (*c).clone() as Arc<dyn Corpus>)
        .collect()
}

fn model(name: &str) -> ModelId {
    ModelId(name.to_string())
}

#[test]
fn an_unchanged_stamp_reads_nothing() {
    let (_db, _sets, pipeline, log) = setup();
    let sessions = FakeCorpus::new("sessions", &log);
    sessions.set("a", "1", &["one", "two"]);
    sessions.set("b", "1", &["three"]);
    pipeline.set_models(vec![Recording::new("static")], Vec::new());
    let corpora = as_corpora(&[&sessions]);
    let first = pipeline.pass(&corpora, None, 10.0);
    assert_eq!((first.read, first.reused, first.removed), (2, 0, 0));
    assert_eq!(first.embedded_now, 3);
    assert_eq!(sessions.reads(), 2);
    let second = pipeline.pass(&corpora, None, 20.0);
    assert_eq!((second.read, second.reused, second.removed), (0, 2, 0));
    assert_eq!(second.embedded_now, 0);
    assert_eq!(sessions.reads(), 2);
}

#[test]
fn growth_embeds_only_new_chunks() {
    let (_db, _sets, pipeline, log) = setup();
    let embedder = Recording::new("static");
    let sessions = FakeCorpus::new("sessions", &log);
    sessions.set("a", "1", &["alpha", "beta"]);
    pipeline.set_models(vec![embedder.clone()], Vec::new());
    let corpora = as_corpora(&[&sessions]);
    pipeline.pass(&corpora, None, 10.0);
    assert_eq!(embedder.take(), vec!["alpha", "beta"]);
    sessions.set("a", "2", &["alpha", "beta", "gamma"]);
    let stats = pipeline.pass(&corpora, None, 20.0);
    assert_eq!(stats.read, 1);
    assert_eq!(embedder.take(), vec!["gamma"]);
    assert_eq!(stats.embedded_now, 1);
}

#[test]
fn deletes_sweep_vectors() {
    let (db, sets, pipeline, log) = setup();
    let sessions = FakeCorpus::new("sessions", &log);
    sessions.set("a", "1", &["alpha"]);
    sessions.set("b", "1", &["beta"]);
    let embedder = Recording::new("static");
    pipeline.set_models(vec![embedder.clone()], Vec::new());
    let corpora = as_corpora(&[&sessions]);
    pipeline.pass(&corpora, None, 10.0);
    assert_eq!(db.stats().vectors, 2);
    assert_eq!(sets.get(&model("static")).unwrap().len(), 2);
    sessions.remove("a");
    let stats = pipeline.pass(&corpora, None, 20.0);
    assert_eq!(stats.removed, 1);
    assert_eq!(db.stats().docs, 1);
    assert_eq!(db.stats().vectors, 1);
    assert_eq!(sets.get(&model("static")).unwrap().len(), 1);
}

#[test]
fn a_changed_doc_drops_the_vectors_of_text_it_no_longer_has() {
    let (db, _sets, pipeline, log) = setup();
    let sessions = FakeCorpus::new("sessions", &log);
    sessions.set("a", "1", &["alpha", "beta"]);
    pipeline.set_models(vec![Recording::new("static")], Vec::new());
    let corpora = as_corpora(&[&sessions]);
    pipeline.pass(&corpora, None, 10.0);
    sessions.set("a", "2", &["alpha"]);
    pipeline.pass(&corpora, None, 20.0);
    assert_eq!(db.stats().chunks, 1);
    assert_eq!(db.stats().vectors, 1);
}

#[test]
fn a_model_change_refills_from_stored_text() {
    let (db, sets, pipeline, log) = setup();
    let sessions = FakeCorpus::new("sessions", &log);
    sessions.set("a", "1", &["alpha", "beta"]);
    sessions.set("b", "1", &["gamma"]);
    let corpora = as_corpora(&[&sessions]);
    pipeline.set_models(vec![Recording::new("old")], Vec::new());
    pipeline.pass(&corpora, None, 10.0);
    assert!(sets.get(&model("old")).is_some());
    let reads = sessions.reads();
    let next = Recording::new("new");
    pipeline.set_models(vec![next.clone()], Vec::new());
    let stats = pipeline.pass(&corpora, None, 20.0);
    assert_eq!(sessions.reads(), reads, "a refill reads no corpus");
    assert_eq!(stats.read, 0);
    assert_eq!(next.take(), vec!["alpha", "beta", "gamma"]);
    assert!(sets.get(&model("old")).is_none());
    assert_eq!(sets.get(&model("new")).unwrap().len(), 3);
    assert_eq!(db.stats().vectors, 3);
    let covered = pipeline.coverage();
    assert_eq!(covered.len(), 1);
    assert_eq!(covered[0].0, model("new"));
    assert_eq!(covered[0].1[0].embedded, 3);
}

#[test]
fn the_old_model_stays_until_the_new_one_covers_every_chunk() {
    let (db, sets, pipeline, log) = setup();
    let sessions = FakeCorpus::new("sessions", &log);
    sessions.set("a", "1", &["alpha", "beta"]);
    let corpora = as_corpora(&[&sessions]);
    pipeline.set_models(vec![Recording::new("old")], Vec::new());
    pipeline.pass(&corpora, None, 10.0);
    let half = Arc::new(TableEmbedder::new(
        "new",
        2,
        vec![("alpha".to_string(), vec![1.0, 0.0])],
    ));
    pipeline.set_models(vec![half], Vec::new());
    pipeline.pass(&corpora, None, 20.0);
    assert!(sets.get(&model("old")).is_some(), "old stays usable");
    assert_eq!(sets.get(&model("new")).unwrap().len(), 1);
    assert_eq!(db.stats().vectors, 3);
    let full = Arc::new(TableEmbedder::new(
        "new",
        2,
        vec![
            ("alpha".to_string(), vec![1.0, 0.0]),
            ("beta".to_string(), vec![0.0, 1.0]),
        ],
    ));
    pipeline.set_models(vec![full], Vec::new());
    pipeline.pass(&corpora, None, 30.0);
    assert!(sets.get(&model("old")).is_none());
    assert_eq!(sets.get(&model("new")).unwrap().len(), 2);
    assert_eq!(db.stats().vectors, 2);
}

#[test]
fn an_unpassed_corpus_is_emptied() {
    let (db, sets, pipeline, log) = setup();
    let sessions = FakeCorpus::new("sessions", &log);
    let pages = FakeCorpus::new("pages", &log);
    sessions.set("a", "1", &["alpha"]);
    pages.set("p", "1", &["page text"]);
    pipeline.set_models(vec![Recording::new("static")], Vec::new());
    pipeline.pass(&as_corpora(&[&sessions, &pages]), None, 10.0);
    assert_eq!(db.stats().docs, 2);
    let stats = pipeline.pass(&as_corpora(&[&sessions]), None, 20.0);
    assert_eq!(stats.removed, 1);
    assert_eq!(db.stats().docs, 1);
    assert!(db.stamps("pages").is_empty());
    assert_eq!(db.stats().vectors, 1);
    assert_eq!(sets.get(&model("static")).unwrap().len(), 1);
}

#[test]
fn static_embeds_at_once_granite_waits_in_the_queue() {
    let (db, sets, pipeline, log) = setup();
    let sessions = FakeCorpus::new("sessions", &log);
    sessions.set("a", "1", &["alpha", "beta", "gamma"]);
    let granite = model("granite");
    pipeline.set_models(vec![Recording::new("static")], vec![granite.clone()]);
    let stats = pipeline.pass(&as_corpora(&[&sessions]), None, 100.0);
    assert_eq!(stats.embedded_now, 3);
    assert_eq!(stats.queued, 3);
    assert_eq!(pipeline.queue().len(), 3);
    assert!(!pipeline.queue().due(100.0));
    assert!(pipeline.queue().due(100.0 + WAKE_AFTER));
    assert!(sets.get(&granite).is_none_or(|s| s.is_empty()));
    let again = pipeline.pass(&as_corpora(&[&sessions]), None, 110.0);
    assert_eq!(again.queued, 0, "a queued chunk is not queued twice");
    let rows = pipeline.queue().take(BATCH);
    assert_eq!(rows.len(), 3);
    let filled: Vec<([u8; 16], Vec<f32>)> = rows
        .iter()
        .map(|(h, _)| (*h, vec![1.0, 0.0, 0.0]))
        .collect();
    pipeline.accept(&granite, filled);
    assert_eq!(pipeline.queue().held(&granite), 0);
    assert_eq!(sets.get(&granite).unwrap().len(), 3);
    assert_eq!(db.coverage(&granite)[0].embedded, 3);
    let last = pipeline.pass(&as_corpora(&[&sessions]), None, 120.0);
    assert_eq!(last.queued, 0);
    assert!(pipeline.queue().is_empty());
    assert!(!pipeline.queue().backlog());
}

#[test]
fn a_bulk_sets_the_backlog_until_nothing_is_missing() {
    let (_db, _sets, pipeline, log) = setup();
    let sessions = FakeCorpus::new("sessions", &log);
    let texts: Vec<String> = (0..40).map(|i| format!("chunk number {i}")).collect();
    let refs: Vec<&str> = texts.iter().map(|t| t.as_str()).collect();
    sessions.set("a", "1", &refs);
    let granite = model("granite");
    pipeline.set_models(Vec::new(), vec![granite.clone()]);
    pipeline.pass(&as_corpora(&[&sessions]), None, 0.0);
    assert!(pipeline.queue().backlog());
    assert!(pipeline.queue().due(0.0));
    loop {
        let rows = pipeline.queue().take(BATCH);
        if rows.is_empty() {
            break;
        }
        let filled = rows.iter().map(|(h, _)| (*h, vec![0.5, 0.5])).collect();
        pipeline.accept(&granite, filled);
        pipeline.pass(&as_corpora(&[&sessions]), None, 1.0);
    }
    assert!(!pipeline.queue().backlog());
    assert_eq!(pipeline.coverage()[0].1[0].embedded, 40);
}

#[test]
fn a_pass_runs_settings_then_sessions_then_pages_then_the_rest() {
    let (_db, _sets, pipeline, log) = setup();
    let sessions = FakeCorpus::new("sessions", &log);
    let pages = FakeCorpus::new("pages", &log);
    let settings = FakeCorpus::new("settings", &log);
    let other = FakeCorpus::new("zzz", &log);
    for c in [&sessions, &pages, &settings, &other] {
        c.set("k", "1", &["text"]);
    }
    pipeline.set_models(Vec::new(), Vec::new());
    pipeline.pass(
        &as_corpora(&[&other, &pages, &sessions, &settings]),
        None,
        0.0,
    );
    let order = log.lock().unwrap().clone();
    assert_eq!(order, vec!["settings:k", "sessions:k", "pages:k", "zzz:k"]);
}

#[test]
fn a_session_is_read_newest_first() {
    let (_db, _sets, pipeline, log) = setup();
    let sessions = FakeCorpus::new("sessions", &log);
    sessions.set("old", "1", &["a"]);
    sessions.set("mid", "1", &["b"]);
    sessions.set("new", "1", &["c"]);
    pipeline.set_models(Vec::new(), Vec::new());
    pipeline.pass(&as_corpora(&[&sessions]), None, 0.0);
    let order = log.lock().unwrap().clone();
    assert_eq!(order, vec!["sessions:new", "sessions:mid", "sessions:old"]);
}

#[test]
fn a_failing_embedder_stops_without_looping() {
    let (db, _sets, pipeline, log) = setup();
    let sessions = FakeCorpus::new("sessions", &log);
    sessions.set("a", "1", &["alpha", "beta"]);
    let broken = Arc::new(TableEmbedder::new("broken", 2, Vec::new()));
    pipeline.set_models(vec![broken], Vec::new());
    let stats = pipeline.pass(&as_corpora(&[&sessions]), None, 0.0);
    assert_eq!(stats.embedded_now, 0);
    assert_eq!(db.stats().vectors, 0);
}

fn many(sessions: &Arc<FakeCorpus>, n: usize) {
    let texts: Vec<String> = (0..n).map(|i| format!("chunk number {i}")).collect();
    let refs: Vec<&str> = texts.iter().map(|t| t.as_str()).collect();
    sessions.set("a", "1", &refs);
}

fn drain(pipeline: &Pipeline, granite: &ModelId, mut seen: impl FnMut(&Pipeline)) {
    while pipeline.queue().due(0.0) {
        let rows = pipeline.queue().take(BATCH);
        if rows.is_empty() {
            break;
        }
        let filled = rows.iter().map(|(h, _)| (*h, vec![0.5, 0.5])).collect();
        pipeline.accept(granite, filled);
        seen(pipeline);
    }
}

#[test]
fn a_backlog_drains_without_waiting_for_the_next_pass() {
    let (db, _sets, pipeline, log) = setup();
    let sessions = FakeCorpus::new("sessions", &log);
    many(&sessions, 600);
    let granite = model("granite");
    pipeline.set_models(Vec::new(), vec![granite.clone()]);
    pipeline.pass(&as_corpora(&[&sessions]), None, 0.0);
    assert!(pipeline.queue().backlog());
    drain(&pipeline, &granite, |_| {});
    assert!(db.missing(&granite, 1).is_empty());
    assert_eq!(pipeline.coverage()[0].1[0].embedded, 600);
    assert!(!pipeline.queue().backlog());
    assert!(pipeline.queue().is_empty());
}

#[test]
fn a_backlog_does_not_reload_the_whole_set_for_every_batch() {
    let (_db, sets, pipeline, log) = setup();
    let sessions = FakeCorpus::new("sessions", &log);
    many(&sessions, 600);
    let granite = model("granite");
    pipeline.set_models(Vec::new(), vec![granite.clone()]);
    pipeline.pass(&as_corpora(&[&sessions]), None, 0.0);
    let mut lens = Vec::new();
    drain(&pipeline, &granite, |_| {
        lens.push(sets.get(&granite).map_or(0, |s| s.len()));
    });
    assert_eq!(lens[0], 0, "the first batch of a backlog waits for more");
    let reloads = lens.windows(2).filter(|w| w[0] != w[1]).count();
    assert!(reloads <= 3, "{reloads} reloads for {} batches", lens.len());
    assert_eq!(sets.get(&granite).unwrap().len(), 600);
}

#[test]
fn a_flush_shows_what_a_stopped_backlog_left_out() {
    let (_db, sets, pipeline, log) = setup();
    let sessions = FakeCorpus::new("sessions", &log);
    many(&sessions, 600);
    let granite = model("granite");
    pipeline.set_models(Vec::new(), vec![granite.clone()]);
    pipeline.pass(&as_corpora(&[&sessions]), None, 0.0);
    let rows = pipeline.queue().take(BATCH);
    let filled = rows.iter().map(|(h, _)| (*h, vec![0.5, 0.5])).collect();
    pipeline.accept(&granite, filled);
    assert_eq!(sets.get(&granite).unwrap().len(), 0);
    pipeline.flush();
    assert_eq!(sets.get(&granite).unwrap().len(), BATCH);
}

#[test]
fn removing_docs_prunes_queued_chunks_and_allows_refilling() {
    let (_db, _sets, pipeline, log) = setup();
    let sessions = FakeCorpus::new("sessions", &log);
    sessions.set("a", "1", &["alpha", "beta"]);
    sessions.set("b", "1", &["gamma"]);
    let granite = model("granite");
    pipeline.set_models(Vec::new(), vec![granite.clone()]);
    let corpora = as_corpora(&[&sessions]);
    pipeline.pass(&corpora, None, 10.0);
    assert_eq!(pipeline.queue().len(), 3);
    sessions.remove("a");
    pipeline.pass(&corpora, None, 20.0);
    assert_eq!(pipeline.queue().len(), 1);
    let taken = pipeline.queue().take(BATCH);
    assert_eq!(taken, vec![(hash("gamma"), "gamma".to_string())]);
    pipeline.accept(&granite, vec![(hash("gamma"), vec![0.5, 0.5])]);
    sessions.set("c", "1", &["alpha", "delta"]);
    let stats = pipeline.pass(&corpora, None, 30.0);
    assert_eq!(stats.queued, 2);
    assert_eq!(pipeline.queue().len(), 2);
}

#[test]
fn accept_drops_vectors_for_chunks_deleted_while_in_flight() {
    let (db, _sets, pipeline, log) = setup();
    let sessions = FakeCorpus::new("sessions", &log);
    sessions.set("a", "1", &["alpha", "beta"]);
    let granite = model("granite");
    pipeline.set_models(Vec::new(), vec![granite.clone()]);
    let corpora = as_corpora(&[&sessions]);
    pipeline.pass(&corpora, None, 10.0);
    let rows = pipeline.queue().take(BATCH);
    assert_eq!(rows.len(), 2);
    sessions.remove("a");
    pipeline.pass(&corpora, None, 20.0);
    assert_eq!(pipeline.queue().held(&granite), 2);
    let filled = rows.iter().map(|(h, _)| (*h, vec![0.5, 0.5])).collect();
    pipeline.accept(&granite, filled);
    assert_eq!(pipeline.queue().held(&granite), 0);
    assert_eq!(db.stats().vectors, 0);
    assert_eq!(db.sweep().unwrap(), 0);
}

#[test]
fn accept_stores_only_the_live_chunks_of_an_in_flight_batch() {
    let (db, sets, pipeline, log) = setup();
    let sessions = FakeCorpus::new("sessions", &log);
    sessions.set("a", "1", &["alpha", "beta"]);
    sessions.set("b", "1", &["gamma"]);
    let granite = model("granite");
    pipeline.set_models(Vec::new(), vec![granite.clone()]);
    let corpora = as_corpora(&[&sessions]);
    pipeline.pass(&corpora, None, 10.0);
    let rows = pipeline.queue().take(BATCH);
    assert_eq!(rows.len(), 3);
    sessions.remove("a");
    pipeline.pass(&corpora, None, 20.0);
    let filled = rows.iter().map(|(h, _)| (*h, vec![0.5, 0.5])).collect();
    pipeline.accept(&granite, filled);
    assert_eq!(pipeline.queue().held(&granite), 0);
    assert_eq!(db.stats().vectors, 1);
    assert_eq!(db.coverage(&granite)[0].embedded, 1);
    assert_eq!(sets.get(&granite).unwrap().len(), 1);
    assert_eq!(db.sweep().unwrap(), 0);
}

struct HaltingEmbedder {
    inner: HashEmbedder,
    pipeline: Arc<Pipeline>,
    halt_on_call: usize,
    calls: AtomicUsize,
}

impl Embedder for HaltingEmbedder {
    fn model(&self) -> &ModelId {
        self.inner.model()
    }

    fn dims(&self) -> usize {
        self.inner.dims()
    }

    fn embed(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>, EmbedError> {
        if self.calls.fetch_add(1, Ordering::SeqCst) == self.halt_on_call {
            self.pipeline.halt();
        }
        self.inner.embed(texts)
    }
}

#[test]
fn halt_stops_a_pass_early_without_reloading_or_filling_queue() {
    let log = Log::default();
    let sessions = FakeCorpus::new("sessions", &log);
    many(&sessions, 80);
    let corpora = as_corpora(&[&sessions]);

    let db = Arc::new(VectorDb::memory().unwrap());
    let sets = Arc::new(Sets::default());
    let pipeline = Arc::new(Pipeline::new(db.clone(), sets.clone()));
    let embedder = Arc::new(HaltingEmbedder {
        inner: HashEmbedder::new("static"),
        pipeline: pipeline.clone(),
        halt_on_call: 1,
        calls: AtomicUsize::new(0),
    });
    pipeline.set_models(vec![embedder], vec![model("granite")]);
    let stats = pipeline.pass(&corpora, None, 10.0);
    assert_eq!(stats.embedded_now, EMBED_CHUNK);
    assert_eq!(db.coverage(&model("static"))[0].embedded, EMBED_CHUNK);
    assert!(sets.get(&model("static")).is_none());
    assert!(pipeline.queue().is_empty());

    let db2 = Arc::new(VectorDb::memory().unwrap());
    let sets2 = Arc::new(Sets::default());
    let pipeline2 = Arc::new(Pipeline::new(db2.clone(), sets2.clone()));
    let immediate = Arc::new(HaltingEmbedder {
        inner: HashEmbedder::new("static"),
        pipeline: pipeline2.clone(),
        halt_on_call: 0,
        calls: AtomicUsize::new(0),
    });
    pipeline2.set_models(vec![immediate], Vec::new());
    let stats2 = pipeline2.pass(&corpora, None, 10.0);
    assert_eq!(stats2.embedded_now, 0);
    assert_eq!(db2.stats().vectors, 0);
}

#[test]
fn halt_makes_accept_release_without_storing() {
    let (db, _sets, pipeline, log) = setup();
    let sessions = FakeCorpus::new("sessions", &log);
    sessions.set("a", "1", &["alpha", "beta"]);
    let granite = model("granite");
    pipeline.set_models(Vec::new(), vec![granite.clone()]);
    pipeline.pass(&as_corpora(&[&sessions]), None, 10.0);
    let rows = pipeline.queue().take(BATCH);
    pipeline.halt();
    let filled = rows.iter().map(|(h, _)| (*h, vec![0.5, 0.5])).collect();
    pipeline.accept(&granite, filled);
    assert_eq!(pipeline.queue().held(&granite), 0);
    assert_eq!(db.stats().vectors, 0);
}
