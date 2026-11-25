#[allow(dead_code)]
#[path = "../examples/eval/run.rs"]
mod run;

use notchling_embed::granite::Granite;
use notchling_engine::search::embed::{Embedder, StaticEmbedder};
use run::{Corpus, Model};
use std::path::{Path, PathBuf};
use std::rc::Rc;

const GRANITE_DIR: &str = "ibm-granite__granite-embedding-97m-multilingual-r2";
const STATIC_DIR: &str = "sentence-transformers__static-similarity-mrl-multilingual-v1";

fn models_dir() -> PathBuf {
    PathBuf::from(std::env::var("NOTCHLING_EMBED_MODELS").expect("NOTCHLING_EMBED_MODELS"))
}

fn models() -> Vec<Model> {
    let dir = models_dir();
    let g = Rc::new(Granite::load(&dir.join(GRANITE_DIR)).expect("granite"));
    let gq = Rc::clone(&g);
    let table = PathBuf::from(
        std::env::var("NOTCHLING_EMBED_STATIC_TABLE").expect("NOTCHLING_EMBED_STATIC_TABLE"),
    );
    let tokenizer = dir
        .join(STATIC_DIR)
        .join("0_StaticEmbedding/tokenizer.json");
    let s = Rc::new(StaticEmbedder::open(&table, &tokenizer).expect("static"));
    let sq = Rc::clone(&s);
    vec![
        Model {
            name: "granite".to_string(),
            queries: Box::new(move |t| gq.embed(t)),
            docs: Box::new(move |t| g.embed(t)),
        },
        Model {
            name: "static".to_string(),
            queries: Box::new(move |t| sq.embed(t).map_err(|e| format!("{e:?}"))),
            docs: Box::new(move |t| s.embed(t).map_err(|e| format!("{e:?}"))),
        },
    ]
}

fn score(e: &run::Evaluation, label: &str) -> Option<(f64, f64)> {
    e.report
        .rows
        .iter()
        .find(|(l, _)| l == label)
        .map(|(_, s)| (s.r1, s.r5))
}

fn within(name: &str, got: f64, want: f64) {
    assert!(
        (got - want).abs() <= 0.05,
        "{name}: R@1 {got}, research {want}"
    );
}

fn run_set(file: &str, models: &[Model]) -> run::Evaluation {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tools/embed-eval/data")
        .join(file);
    let (corpus, queries): (Corpus, _) = run::research_set(&path).expect("set");
    run::evaluate(&corpus, queries, models, &[vec![0], vec![0, 1]]).expect("evaluation")
}

#[test]
#[ignore = "needs NOTCHLING_EMBED_MODELS and NOTCHLING_EMBED_STATIC_TABLE"]
fn eval_reproduces_research_numbers() {
    let models = models();
    let chat = run_set("chat.json", &models);
    let settings = run_set("settings.json", &models);
    println!("{}", notchling_embed::eval::render(&chat.report));
    println!("{}", notchling_embed::eval::render(&settings.report));
    let (r1, _) = score(&chat, "all/text+granite+static").expect("chat row");
    within("chat all", r1, 0.97);
    let (r1, _) = score(&settings, "all/text+granite+static").expect("settings row");
    within("settings all", r1, 0.81);
    let (_, r5) = score(&chat, "vi-nodiac/text+granite+static").expect("nodiac row");
    within("chat vi-nodiac R@5", r5, 1.0);
}
