use super::*;
use crate::search::chunk::{hash, ChunkKind, ChunkText};
use crate::search::corpus::Doc;
use crate::search::embed::{quantize, ModelId, Q8};
use std::sync::Arc;
use std::time::Instant;

fn unit(v: Vec<f32>) -> Vec<f32> {
    let n = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    v.into_iter().map(|x| x / n).collect()
}

fn axis(i: usize, dims: usize, tilt: f32) -> Vec<f32> {
    let mut v = vec![0.0; dims];
    v[i] = 1.0;
    v[(i + 1) % dims] = tilt;
    unit(v)
}

fn sdoc(corpus: &str, key: &str, at: f64, facets: &[(&str, &str)]) -> SetDoc {
    SetDoc {
        corpus: corpus.to_string(),
        key: key.to_string(),
        at,
        facets: facets
            .iter()
            .map(|(n, v)| (n.to_string(), v.to_string()))
            .collect(),
    }
}

fn row(doc: usize, tag: &str, v: &[f32]) -> (usize, [u8; 16], Q8) {
    (doc, hash(tag), quantize(v))
}

fn small() -> VectorSet {
    VectorSet::from_parts(
        4,
        vec![
            sdoc("sessions", "a", 10.0, &[("tool", "claude")]),
            sdoc("sessions", "b", 20.0, &[("tool", "codex")]),
            sdoc("pages", "c", 30.0, &[("owner", "x")]),
        ],
        vec![
            row(0, "a1", &axis(0, 4, 0.0)),
            row(0, "a2", &axis(0, 4, 0.5)),
            row(1, "b1", &axis(0, 4, 0.2)),
            row(2, "c1", &axis(1, 4, 0.0)),
        ],
    )
}

#[test]
fn masks_are_exact() {
    let set = small();
    let query = axis(0, 4, 0.0);
    let all = set.scan(&query, 10, None, -1.0);
    assert_eq!(all.len(), 3);
    let mask = set.mask(&|_, facets, _| !facets.contains(&("tool", "claude")));
    let hits = set.scan(&query, 10, Some(&mask), -1.0);
    assert!(hits.iter().all(|h| h.key != "a"));
    assert_eq!(hits.len(), 2);
    let none = set.mask(&|_, _, _| false);
    assert!(set.scan(&query, 10, Some(&none), -1.0).is_empty());
    let pages = set.mask(&|corpus, _, _| corpus == "pages");
    let hits = set.scan(&query, 10, Some(&pages), -1.0);
    assert_eq!(hits.len(), 1);
    assert_eq!(
        (hits[0].corpus.as_str(), hits[0].key.as_str()),
        ("pages", "c")
    );
    let recent = set.mask(&|_, _, at| at >= 20.0);
    let hits = set.scan(&query, 10, Some(&recent), -1.0);
    let keys: Vec<&str> = hits.iter().map(|h| h.key.as_str()).collect();
    assert!(!keys.contains(&"a"));
}

#[test]
fn best_chunk_per_doc() {
    let set = small();
    let hits = set.scan(&axis(0, 4, 0.0), 10, None, -1.0);
    let a: Vec<&DocHit> = hits.iter().filter(|h| h.key == "a").collect();
    assert_eq!(a.len(), 1);
    assert_eq!(a[0].chunk, hash("a1"));
    assert_eq!(hits[0].key, "a");
    assert!(hits.windows(2).all(|w| w[0].score >= w[1].score));
}

#[test]
fn top_chunks_bounds_the_work() {
    let set = small();
    let hits = set.scan(&axis(0, 4, 0.0), 1, None, -1.0);
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].key, "a");
    assert!(set.scan(&axis(0, 4, 0.0), 0, None, -1.0).is_empty());
}

#[test]
fn floor_drops_low_scores() {
    let set = small();
    let hits = set.scan(&axis(0, 4, 0.0), 10, None, 0.5);
    assert!(hits.iter().all(|h| h.score >= 0.5));
    assert!(hits.iter().all(|h| h.key != "c"));
    assert!(!hits.is_empty());
}

#[test]
fn a_query_of_the_wrong_size_finds_nothing() {
    let set = small();
    assert!(set.scan(&[1.0, 0.0], 10, None, -1.0).is_empty());
}

#[test]
fn a_query_reads_a_snapshot_while_a_swap_happens() {
    let sets = Arc::new(Sets::default());
    let model = ModelId("m/p1".to_string());
    sets.swap(&model, small());
    let old_len = sets.get(&model).unwrap().len();
    let reader = {
        let sets = sets.clone();
        let model = model.clone();
        std::thread::spawn(move || {
            let mut seen = Vec::new();
            for _ in 0..300 {
                let set = sets.get(&model).unwrap();
                let hits = set.scan(&axis(0, 4, 0.0), 10, None, -1.0);
                seen.push((set.len(), hits.len()));
            }
            seen
        })
    };
    for i in 0..300 {
        let set = if i % 2 == 0 {
            VectorSet::from_parts(
                4,
                vec![sdoc("sessions", "z", 1.0, &[])],
                vec![row(0, "z1", &axis(0, 4, 0.0))],
            )
        } else {
            small()
        };
        sets.swap(&model, set);
    }
    for (len, hits) in reader.join().unwrap() {
        assert!(len == 1 || len == old_len, "{len}");
        assert_eq!(hits, if len == 1 { 1 } else { 3 });
    }
    sets.remove(&model);
    assert!(sets.get(&model).is_none());
}

#[test]
fn load_reads_docs_chunks_facets_and_only_embedded_chunks() {
    let db = VectorDb::memory().unwrap();
    let model = ModelId("m/p1".to_string());
    let chunk = |t: &str| ChunkText {
        kind: ChunkKind::Turn,
        text: t.to_string(),
    };
    let doc = |key: &str, tool: &str, texts: &[&str]| Doc {
        key: key.to_string(),
        title: key.to_string(),
        at: 5.0,
        facets: vec![("tool".to_string(), tool.to_string())],
        chunks: texts.iter().map(|t| chunk(t)).collect(),
    };
    db.put_doc("sessions", "s", &doc("a", "claude", &["one", "two"]))
        .unwrap();
    db.put_doc("sessions", "s", &doc("b", "codex", &["three"]))
        .unwrap();
    db.put_vectors(
        &model,
        VecFormat::F16,
        &[
            (hash("one"), axis(0, 8, 0.0)),
            (hash("three"), axis(1, 8, 0.0)),
        ],
    )
    .unwrap();
    let set = VectorSet::load(&db, &model);
    assert_eq!(set.len(), 2);
    let only_codex = set.mask(&|_, facets, _| facets.contains(&("tool", "codex")));
    let hits = set.scan(&axis(1, 8, 0.0), 10, Some(&only_codex), 0.5);
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].key, "b");
    assert!(hits[0].score > 0.9);
    let other = ModelId("other/p1".to_string());
    assert_eq!(VectorSet::load(&db, &other).len(), 0);
}

#[test]
fn scan_50k_is_fast() {
    let dims = 384;
    let rows = 50_000;
    let mut state = 11u64;
    let mut next = || {
        state = state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        ((state >> 33) as f32 / (1u64 << 31) as f32) * 2.0 - 1.0
    };
    let docs: Vec<SetDoc> = (0..rows / 5)
        .map(|i| {
            sdoc(
                "sessions",
                &format!("k{i}"),
                i as f64,
                &[("tool", "claude")],
            )
        })
        .collect();
    let data: Vec<(usize, [u8; 16], Q8)> = (0..rows)
        .map(|i| {
            let v: Vec<f32> = (0..dims).map(|_| next()).collect();
            (i / 5, hash(&i.to_string()), quantize(&v))
        })
        .collect();
    let set = VectorSet::from_parts(dims, docs, data);
    let query = unit((0..dims).map(|_| next()).collect());
    let mask = set.mask(&|_, _, at| at >= 100.0);
    let start = Instant::now();
    let hits = set.scan(&query, 300, Some(&mask), -1.0);
    let took = start.elapsed();
    assert!(!hits.is_empty());
    let limit = if cfg!(debug_assertions) { 2000 } else { 20 };
    assert!(took.as_millis() < limit, "{took:?}");
    let start = Instant::now();
    let _ = set.scan(&query, 300, None, -1.0);
    println!("scan 50k x 384 unmasked: {:?}", start.elapsed());
}
