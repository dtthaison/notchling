use super::static_model::{StaticEmbedder, MAGIC, MODEL};
use super::{quantize, Embedder, ModelId};
use std::path::{Path, PathBuf};

const TINY_TOKENIZER: &str = r#"{
  "version": "1.0",
  "truncation": null,
  "padding": null,
  "added_tokens": [
    {"id": 0, "content": "<s>", "single_word": false, "lstrip": false, "rstrip": false, "normalized": false, "special": true},
    {"id": 1, "content": "</s>", "single_word": false, "lstrip": false, "rstrip": false, "normalized": false, "special": true},
    {"id": 2, "content": "<unk>", "single_word": false, "lstrip": false, "rstrip": false, "normalized": false, "special": true},
    {"id": 3, "content": "<pad>", "single_word": false, "lstrip": false, "rstrip": false, "normalized": false, "special": true}
  ],
  "normalizer": null,
  "pre_tokenizer": {"type": "Whitespace"},
  "post_processor": null,
  "decoder": null,
  "model": {
    "type": "BPE", "dropout": null, "unk_token": "<unk>", "continuing_subword_prefix": null,
    "end_of_word_suffix": null, "fuse_unk": false, "byte_fallback": false, "ignore_merges": false,
    "vocab": {"<s>": 0, "</s>": 1, "<unk>": 2, "<pad>": 3, "a": 4, "b": 5, "c": 6, "ab": 7, "abc": 8},
    "merges": ["a b", "ab c"]
  }
}"#;

const ROWS: usize = 10;
const DIMS: usize = 8;

fn lcg(state: &mut u64) -> f32 {
    *state = state
        .wrapping_mul(6364136223846793005)
        .wrapping_add(1442695040888963407);
    ((*state >> 33) as f32 / (1u64 << 31) as f32) * 2.0 - 1.0
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("nl-static-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("scratch dir");
    dir
}

fn word_rows() -> Vec<Vec<f32>> {
    let mut state = 17u64;
    (0..ROWS)
        .map(|_| (0..DIMS).map(|_| lcg(&mut state)).collect())
        .collect()
}

fn table_bytes(rows: &[Vec<f32>]) -> Vec<u8> {
    let quantized: Vec<_> = rows.iter().map(|r| quantize(r)).collect();
    let mut out = MAGIC.to_vec();
    out.extend((rows.len() as u32).to_le_bytes());
    out.extend((rows[0].len() as u32).to_le_bytes());
    for q in &quantized {
        out.extend(q.scale.to_le_bytes());
    }
    for q in &quantized {
        out.extend(q.vec.iter().map(|&v| v as u8));
    }
    out
}

fn dequantized(rows: &[Vec<f32>], id: usize) -> Vec<f32> {
    let q = quantize(&rows[id]);
    q.vec.iter().map(|&v| f32::from(v) * q.scale).collect()
}

fn unit(v: Vec<f32>) -> Vec<f32> {
    let n = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    v.into_iter().map(|x| x / n).collect()
}

fn files(dir: &Path, table: &[u8]) -> (PathBuf, PathBuf) {
    let table_path = dir.join("table.bin");
    let tokenizer = dir.join("tokenizer.json");
    std::fs::write(&table_path, table).expect("table");
    std::fs::write(&tokenizer, TINY_TOKENIZER).expect("tokenizer");
    (table_path, tokenizer)
}

fn cosine(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

#[test]
fn round_trips_a_tiny_table() {
    let dir = scratch("round");
    let rows = word_rows();
    let (table, tokenizer) = files(&dir, &table_bytes(&rows));
    let e = StaticEmbedder::open(&table, &tokenizer).expect("open");
    assert_eq!(e.model(), &ModelId(MODEL.to_string()));
    assert_eq!(MODEL, "static-mrl@b68f412/512i8/p1");
    assert_eq!(e.dims(), DIMS);

    let out = e.embed(&["abc", "ab c", ""]).expect("embed");
    let one = unit(dequantized(&rows, 8));
    assert!(cosine(&out[0], &one) > 0.9999, "{:?} {:?}", out[0], one);

    let a = dequantized(&rows, 7);
    let b = dequantized(&rows, 6);
    let mean = unit(a.iter().zip(&b).map(|(x, y)| (x + y) / 2.0).collect());
    assert!(cosine(&out[1], &mean) > 0.9999);

    let norm: f32 = out[0].iter().map(|x| x * x).sum::<f32>().sqrt();
    assert!((norm - 1.0).abs() < 1e-5);
    assert_eq!(out[2], vec![0.0; DIMS]);
}

#[test]
fn repeating_a_token_keeps_the_mean() {
    let dir = scratch("repeat");
    let rows = word_rows();
    let (table, tokenizer) = files(&dir, &table_bytes(&rows));
    let e = StaticEmbedder::open(&table, &tokenizer).expect("open");
    let once = e.embed(&["abc"]).expect("once");
    let twice = e.embed(&["abc abc"]).expect("twice");
    assert!(cosine(&once[0], &twice[0]) > 0.9999);
}

#[test]
fn a_bad_table_is_refused() {
    let dir = scratch("bad");
    let rows = word_rows();
    let good = table_bytes(&rows);

    let mut wrong_magic = good.clone();
    wrong_magic[0] ^= 0xff;
    let truncated = good[..good.len() - 5].to_vec();
    let short_header = good[..12].to_vec();
    let mut no_rows = good.clone();
    no_rows[8..12].copy_from_slice(&0u32.to_le_bytes());
    let mut no_dims = good.clone();
    no_dims[12..16].copy_from_slice(&0u32.to_le_bytes());
    let mut huge = good.clone();
    huge[8..12].copy_from_slice(&u32::MAX.to_le_bytes());
    let mut padded = good.clone();
    padded.extend([0u8; 3]);

    for (name, bytes) in [
        ("magic", wrong_magic),
        ("truncated", truncated),
        ("header", short_header),
        ("rows", no_rows),
        ("dims", no_dims),
        ("huge", huge),
        ("padded", padded),
        ("empty", Vec::new()),
    ] {
        let (table, tokenizer) = files(&dir, &bytes);
        let refused = StaticEmbedder::open(&table, &tokenizer);
        assert!(refused.is_err(), "{name} was accepted");
    }
    let (table, tokenizer) = files(&dir, &good);
    assert!(StaticEmbedder::open(&table, &tokenizer).is_ok());
    assert!(StaticEmbedder::open(&dir.join("missing.bin"), &tokenizer).is_err());
    assert!(StaticEmbedder::open(&table, &dir.join("missing.json")).is_err());
}

#[test]
fn a_token_past_the_table_adds_nothing() {
    let dir = scratch("short");
    let rows: Vec<Vec<f32>> = word_rows().into_iter().take(8).collect();
    let (table, tokenizer) = files(&dir, &table_bytes(&rows));
    let e = StaticEmbedder::open(&table, &tokenizer).expect("open");
    let out = e.embed(&["abc", "ab"]).expect("embed");
    assert_eq!(out[0], vec![0.0; DIMS]);
    assert!(cosine(&out[1], &unit(dequantized(&rows, 7))) > 0.9999);
}

fn models_dir() -> PathBuf {
    PathBuf::from(std::env::var("NOTCHLING_EMBED_MODELS").expect("NOTCHLING_EMBED_MODELS"))
}

fn data(name: &str) -> serde_json::Value {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tools/embed-eval/data")
        .join(name);
    serde_json::from_slice(&std::fs::read(path).expect("research data")).expect("research json")
}

#[test]
#[ignore = "needs NOTCHLING_EMBED_MODELS and NOTCHLING_EMBED_STATIC_TABLE"]
fn static_matches_the_research() {
    let table = PathBuf::from(
        std::env::var("NOTCHLING_EMBED_STATIC_TABLE").expect("NOTCHLING_EMBED_STATIC_TABLE"),
    );
    let tokenizer = models_dir()
        .join("sentence-transformers__static-similarity-mrl-multilingual-v1")
        .join("0_StaticEmbedding")
        .join("tokenizer.json");
    let e = StaticEmbedder::open(&table, &tokenizer).expect("open");
    assert_eq!(e.dims(), 512);

    let pair = e.embed(&["dang nhap", "đăng nhập"]).expect("pair");
    let c = cosine(&pair[0], &pair[1]);
    assert!(c >= 0.7, "cosine {c}");

    let chat = data("chat.json");
    let docs = chat["docs"].as_object().expect("docs");
    let keys: Vec<&String> = docs.keys().collect();
    let texts: Vec<&str> = keys
        .iter()
        .map(|k| docs[*k].as_str().expect("doc"))
        .collect();
    let doc_vectors = e.embed(&texts).expect("docs");
    let mut hits = 0usize;
    let mut total = 0usize;
    for q in chat["queries"].as_array().expect("queries") {
        if q[2].as_str() != Some("vi-nodiac") {
            continue;
        }
        let v = &e.embed(&[q[0].as_str().expect("query")]).expect("query")[0];
        let best = (0..keys.len())
            .max_by(|&a, &b| cosine(v, &doc_vectors[a]).total_cmp(&cosine(v, &doc_vectors[b])))
            .expect("a doc");
        let gold: Vec<&str> = q[1]
            .as_array()
            .expect("gold")
            .iter()
            .filter_map(|g| g.as_str())
            .collect();
        total += 1;
        hits += usize::from(gold.contains(&keys[best].as_str()));
    }
    let research = data("res_static-mrl-multilingual.json");
    let want = research["chat"]["dense"]["vi-nodiac"][0]
        .as_f64()
        .expect("research r1");
    assert!(total > 0);
    let got = hits as f64 / total as f64;
    assert!(
        (got - want).abs() <= 0.05,
        "R@1 {got} against research {want}"
    );
}
