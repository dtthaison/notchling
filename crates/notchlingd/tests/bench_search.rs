use notchling_engine::search::chunk::{ChunkKind, ChunkText};
use notchling_engine::search::corpus::{Corpus, Doc, DocStamp, Tier as CorpusTier};
use notchling_engine::search::embed::helper_fake::FakeSpawn;
use notchling_engine::search::embed::{Embedder, HashEmbedder, ModelId};
use notchling_engine::search::query::category::Category;
use notchling_engine::search::query::filter::Filter;
use notchling_engine::search::range::IndexRange;
use notchling_notchlingd::acting_ops::Room;
use notchling_notchlingd::embed_ops::{EmbedOps, EmbedParts, EmbedSettings, PluggedIn};
use serde_json::{json, Value};
use std::sync::Arc;
use std::time::Instant;

const CHUNKS: usize = 50_000;
const QUERIES: usize = 200;
const P95_LIMIT_MS: f64 = 50.0;
const WORDS: [&str; 24] = [
    "bridge", "daemon", "search", "index", "vector", "session", "token", "switch", "account",
    "vault", "phone", "pairing", "render", "layout", "socket", "hook", "cluster", "worker", "lane",
    "budget", "thermal", "helper", "model", "queue",
];

struct Quiet;

impl Room for Quiet {
    fn publish(&self, _topic: &str, _event: &str, _data: &Value) {}

    fn can_answer(&self) -> bool {
        true
    }

    fn can_follow(&self, _topic: &str) -> bool {
        true
    }
}

struct Generated;

fn text_of(i: usize) -> String {
    let pick = |n: usize| WORDS[(i / n + i % n) % WORDS.len()];
    format!(
        "{} {} {} {} number {i}",
        pick(1),
        pick(7),
        pick(31),
        pick(131)
    )
}

impl Corpus for Generated {
    fn id(&self) -> &'static str {
        "sessions"
    }

    fn tier(&self) -> CorpusTier {
        CorpusTier::Agent
    }

    fn scan(&self, _window: Option<f64>, _now: f64) -> Vec<DocStamp> {
        (0..CHUNKS)
            .map(|i| DocStamp {
                key: format!("s{i}"),
                stamp: "1".to_string(),
                at: 100.0 + i as f64,
            })
            .collect()
    }

    fn read(&self, doc: &DocStamp, _now: f64) -> Option<Doc> {
        let i: usize = doc.key.strip_prefix('s')?.parse().ok()?;
        Some(Doc {
            key: doc.key.clone(),
            title: doc.key.clone(),
            at: doc.at,
            facets: vec![("tool".to_string(), "claude".to_string())],
            chunks: vec![ChunkText {
                kind: ChunkKind::Head,
                text: text_of(i),
            }],
        })
    }
}

fn percentile(sorted: &[f64], p: f64) -> f64 {
    let at = ((sorted.len() as f64) * p).ceil() as usize;
    sorted[at.clamp(1, sorted.len()) - 1]
}

#[test]
#[ignore = "50,000 chunks; run in release: cargo test --release -p notchling-notchlingd --test bench_search -- --ignored --nocapture"]
fn hybrid_p95_under_50_ms_at_50k_chunks() {
    let dir = std::env::temp_dir().join(format!("nl-bench-search-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let granite = ModelId("granite-bench@1/p1".to_string());
    let spawn = FakeSpawn::new(&granite);
    let ops = EmbedOps::with_parts(EmbedParts {
        data_dir: dir.clone(),
        room: Arc::new(Quiet),
        settings: Arc::new(|| EmbedSettings {
            on: true,
            paused: false,
            models: Vec::new(),
            range: IndexRange::All,
            categories: vec![Category::Sessions],
        }),
        corpora: Arc::new(|_| vec![Arc::new(Generated) as Arc<dyn Corpus>]),
        spawn: Arc::new(spawn),
        granite_id: granite,
        granite_present: false,
        statics: Arc::new(|| {
            Ok(Some(
                Arc::new(HashEmbedder::new("static-bench@1/p1")) as Arc<dyn Embedder>
            ))
        }),
        fixture: true,
        power: Arc::new(PluggedIn),
        net: notchling_notchlingd::embed_fetch::ModelNet::offline(),
        now: Arc::new(|| 1_000_000.0),
    });
    let built = Instant::now();
    ops.tick();
    eprintln!("indexed {CHUNKS} chunks in {:?}", built.elapsed());
    eprintln!("note: fixture models (HashEmbedder, 64 dims); no real helper in this bench");

    let filter = Filter::default();
    let queries: Vec<String> = (0..QUERIES)
        .map(|i| {
            format!(
                "{} {} {}",
                WORDS[i % WORDS.len()],
                WORDS[(i / 3 + 5) % WORDS.len()],
                i * 7
            )
        })
        .collect();
    for q in queries.iter().take(5) {
        ops.query_lists(q, &filter, &["sessions"]);
    }
    let mut ms: Vec<f64> = queries
        .iter()
        .map(|q| {
            let started = Instant::now();
            let (_, lists) = ops.query_lists(q, &filter, &["sessions"]);
            let took = started.elapsed().as_secs_f64() * 1000.0;
            assert!(lists.get("sessions").is_some_and(|l| l.statics.is_some()));
            took
        })
        .collect();
    let scan = ms.iter().sum::<f64>() / ms.len() as f64;
    ms.sort_by(|a, b| a.total_cmp(b));
    let (p50, p95) = (percentile(&ms, 0.5), percentile(&ms, 0.95));
    eprintln!(
        "bench-json: {}",
        json!({"chunks": CHUNKS, "queries": QUERIES, "dims": 64, "p50_ms": p50, "p95_ms": p95, "scan_ms": scan})
    );
    let _ = std::fs::remove_dir_all(&dir);
    assert!(p95 < P95_LIMIT_MS, "p95 {p95:.1} ms over {P95_LIMIT_MS} ms");
}
