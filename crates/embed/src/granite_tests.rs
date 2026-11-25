use crate::bench::{self, Report};
use crate::granite::{Granite, Words};
use crate::modernbert::ModernBert;
use crate::scratch::Scratch;
use crate::testing::{cosine, tiny_config, tiny_weights, write_tiny_model, TINY};
use crate::tokens::Tokens;
use candle_core::{DType, Device};
use candle_nn::VarBuilder;
use std::path::PathBuf;

fn tiny_granite() -> Granite {
    let config = tiny_config();
    let mut weights = tiny_weights(&config, 11);
    let table = weights
        .remove("embeddings.tok_embeddings.weight")
        .expect("word table");
    let rows: Vec<Vec<f32>> = table.to_vec2().expect("rows");
    let bytes: Vec<u8> = rows
        .iter()
        .flatten()
        .flat_map(|&x| half::bf16::from_f32(x).to_bits().to_le_bytes())
        .collect();
    let words = Words::from_bytes(
        Box::new(bytes),
        0,
        config.vocab_size,
        config.hidden_size,
        DType::BF16,
    )
    .expect("words");
    let vb = VarBuilder::from_tensors(weights, DType::F32, &Device::Cpu);
    let model = ModernBert::load_encoder(vb, &config).expect("encoder");
    let tokens = Tokens::from_tokenizer_slice(TINY.as_bytes()).expect("tokens");
    Granite::from_parts(model, words, tokens, 64)
}

#[test]
fn padding_does_not_change_a_vector() {
    let g = tiny_granite();
    let short = "abc";
    let long = "abc ab c a b c abc abc ab ab c c a b abc ab c a abc b c ab abc";
    let alone = g.embed(&[short]).expect("alone");
    let batch = g.embed_padded(&[long, short, "ab"]).expect("batch");
    let c = cosine(&alone[0], &batch[1]);
    assert!(c >= 0.9999, "cosine {c}");
    let long_alone = g.embed(&[long]).expect("long");
    assert!(cosine(&long_alone[0], &batch[0]) >= 0.9999);
    let unpadded = g.embed(&[long, short, "ab"]).expect("one by one");
    for (a, b) in unpadded.iter().zip(&batch) {
        assert!(cosine(a, b) >= 0.9999);
    }
}

#[test]
fn vectors_are_unit_length() {
    let g = tiny_granite();
    let out = g.embed(&["abc ab", "c"]).expect("embed");
    assert_eq!(out.len(), 2);
    for v in &out {
        assert_eq!(v.len(), 32);
        let n: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt();
        assert!((n - 1.0).abs() < 1e-4, "norm {n}");
    }
    assert!(g.embed(&[]).expect("empty").is_empty());
}

#[test]
fn a_short_word_table_is_refused() {
    let bytes = vec![0u8; 10];
    assert!(Words::from_bytes(Box::new(bytes), 0, 4, 4, DType::BF16).is_err());
}

#[test]
fn a_word_table_that_overflows_or_is_empty_is_refused() {
    let big = usize::MAX / 2;
    assert!(Words::from_bytes(Box::new(vec![0u8; 16]), 0, big, 4, DType::BF16).is_err());
    assert!(Words::from_bytes(Box::new(vec![0u8; 16]), usize::MAX, 1, 1, DType::F32).is_err());
    assert!(Words::from_bytes(Box::new(vec![0u8; 16]), 0, 0, 4, DType::BF16).is_err());
    assert!(Words::from_bytes(Box::new(vec![0u8; 16]), 0, 4, 0, DType::BF16).is_err());
}

const LONG: &str = "abc ab c a b c abc abc ab ab c c a b abc ab c a abc b c ab abc abc ab c a b c abc abc ab ab c c a b abc";

#[test]
fn a_model_folder_loads_and_embeds() {
    let dir = Scratch::new("nl-granite-test").expect("scratch");
    write_tiny_model(dir.path(), 1024);
    let g = Granite::load(dir.path()).expect("load");
    assert_eq!(g.embed(&[LONG]).expect("embed")[0].len(), 32);
}

#[test]
fn a_short_position_table_caps_the_text_instead_of_failing() {
    let dir = Scratch::new("nl-granite-test").expect("scratch");
    write_tiny_model(dir.path(), 16);
    let g = Granite::load(dir.path()).expect("load");
    assert!(g.tokens(LONG) <= 16, "{} tokens", g.tokens(LONG));
    let v = g.embed(&[LONG]).expect("a long text under a short table");
    assert_eq!(v[0].len(), 32);
}

#[test]
fn a_zero_position_table_is_refused_not_a_panic() {
    let dir = Scratch::new("nl-granite-test").expect("scratch");
    write_tiny_model(dir.path(), 0);
    assert!(Granite::load(dir.path()).is_err());
}

#[test]
fn a_broken_native_tokenizer_falls_back_to_tokenizer_json() {
    let dir = Scratch::new("nl-granite-test").expect("scratch");
    write_tiny_model(dir.path(), 1024);
    std::fs::write(
        dir.join(crate::granite::TOKENS_FILE),
        b"notchling-tokens 1\n{",
    )
    .expect("part");
    let g = Granite::load(dir.path()).expect("load with a bad native file");
    assert_eq!(g.embed(&["abc"]).expect("embed")[0].len(), 32);
}

#[test]
fn a_missing_tokenizer_is_still_an_error() {
    let dir = Scratch::new("nl-granite-test").expect("scratch");
    write_tiny_model(dir.path(), 1024);
    std::fs::remove_file(dir.join("tokenizer.json")).expect("remove");
    std::fs::write(dir.join(crate::granite::TOKENS_FILE), b"junk").expect("junk");
    assert!(Granite::load(dir.path()).is_err());
}

#[test]
fn serving_caps_the_thread_pools_before_the_model_loads() {
    let dir = Scratch::new("nl-granite-test").expect("scratch");
    std::env::remove_var("RAYON_NUM_THREADS");
    std::env::remove_var("VECLIB_MAXIMUM_THREADS");
    assert!(crate::serve::load(&dir.join("no-such-model")).is_err());
    assert_eq!(std::env::var("RAYON_NUM_THREADS").as_deref(), Ok("1"));
    assert_eq!(std::env::var("VECLIB_MAXIMUM_THREADS").as_deref(), Ok("1"));
}

#[test]
fn bench_reads_its_queries_and_chunks() {
    let data = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tools/embed-eval/data");
    let queries = bench::queries(&data).expect("queries");
    assert_eq!(queries.len(), 80);
    let chunks = bench::chunks(&data, 64, 1200).expect("chunks");
    assert_eq!(chunks.len(), 64);
    assert!(chunks.iter().all(|c| c.chars().count() == 1200));
}

#[test]
#[ignore = "needs NOTCHLING_EMBED_MODELS and a release build"]
fn granite_gates() {
    let root =
        PathBuf::from(std::env::var("NOTCHLING_EMBED_MODELS").expect("NOTCHLING_EMBED_MODELS"));
    let dir = root.join("ibm-granite__granite-embedding-97m-multilingual-r2");
    let data = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tools/embed-eval/data");
    bench::limit_threads(1);
    let report: Report = bench::run(&dir, &data, 0.5).expect("bench");
    println!(
        "bench-json: {}",
        serde_json::to_string(&report).expect("json")
    );
    assert!(
        report.query_p50_ms <= 5.0,
        "query p50 {} ms",
        report.query_p50_ms
    );
    assert!(
        report.chunks_per_s_at_budget >= 12.0,
        "{} chunks/s at the budget",
        report.chunks_per_s_at_budget
    );
    assert!(
        report.peak_rss_mb <= 350.0,
        "peak RSS {} MB",
        report.peak_rss_mb
    );
}

#[test]
#[ignore = "needs NOTCHLING_EMBED_MODELS"]
fn granite_ranks_a_paraphrase_first() {
    let root =
        PathBuf::from(std::env::var("NOTCHLING_EMBED_MODELS").expect("NOTCHLING_EMBED_MODELS"));
    let g = Granite::load(&root.join("ibm-granite__granite-embedding-97m-multilingual-r2"))
        .expect("load");
    let v = g
        .embed(&[
            "đăng nhập không được",
            "Tôi không thể đăng nhập vào tài khoản của mình",
            "Cách nấu phở bò ngon tại nhà",
            "keep the Mac awake while a job runs",
            "prevent the computer from sleeping during a long task",
        ])
        .expect("embed");
    assert_eq!(v[0].len(), 384);
    assert!(cosine(&v[0], &v[1]) > cosine(&v[0], &v[2]) + 0.1);
    assert!(cosine(&v[3], &v[4]) > cosine(&v[3], &v[2]) + 0.1);
}
