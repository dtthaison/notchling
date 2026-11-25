mod qwen3;
mod run;

use candle_core::{DType, Device, Tensor};
use candle_nn::VarBuilder;
use notchling_embed::convert;
use notchling_embed::eval::{self, BadLine, Query};
use notchling_embed::granite::Granite;
use notchling_embed::scratch::Scratch;
use notchling_embed::tokens::Tokens;
use notchling_engine::search::embed::{Embedder, StaticEmbedder};
use run::{Corpus, Model};
use std::cell::RefCell;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::rc::Rc;

const USAGE: &str = "usage: eval (--set FILE | --queries FILE --data-root DIR) [--models DIR] [--extra-model NAME DIR] [--find WORDS] [--sweep-floors] [--verbose] [--json]";

const F2LLM_PROMPT: &str =
    "Instruct: Given a question, retrieve passages that can help answer the question.\nQuery: ";
const F2LLM_MAX_TOKENS: usize = 1024;

const GRANITE_DIR: &str = "ibm-granite__granite-embedding-97m-multilingual-r2";
const STATIC_DIR: &str = "sentence-transformers__static-similarity-mrl-multilingual-v1";

#[derive(Default)]
struct Args {
    set: Option<PathBuf>,
    queries: Option<PathBuf>,
    data_root: Option<PathBuf>,
    models: Option<PathBuf>,
    find: Option<String>,
    extra: Vec<(String, PathBuf)>,
    sweep: bool,
    verbose: bool,
    json: bool,
}

fn parse(args: &[String]) -> Option<Args> {
    let mut out = Args::default();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--set" => out.set = Some(PathBuf::from(it.next()?)),
            "--queries" => out.queries = Some(PathBuf::from(it.next()?)),
            "--data-root" => out.data_root = Some(PathBuf::from(it.next()?)),
            "--models" => out.models = Some(PathBuf::from(it.next()?)),
            "--find" => out.find = Some(it.next()?.clone()),
            "--extra-model" => {
                let name = it.next()?.clone();
                out.extra.push((name, PathBuf::from(it.next()?)));
            }
            "--sweep-floors" => out.sweep = true,
            "--verbose" => out.verbose = true,
            "--json" => out.json = true,
            _ => return None,
        }
    }
    let queries_mode = out.queries.is_some() && out.data_root.is_some() && out.set.is_none();
    let set_mode = out.set.is_some() && out.queries.is_none() && out.data_root.is_none();
    let find_mode = out.find.is_some() && out.data_root.is_some();
    (queries_mode || set_mode || find_mode).then_some(out)
}

fn models_dir(args: &Args) -> Result<PathBuf, String> {
    args.models
        .clone()
        .or_else(|| std::env::var_os("NOTCHLING_EMBED_MODELS").map(PathBuf::from))
        .ok_or_else(|| "give --models or set NOTCHLING_EMBED_MODELS".to_string())
}

fn granite(models: &Path) -> Result<Model, String> {
    let g = Rc::new(Granite::load(&models.join(GRANITE_DIR))?);
    let q = Rc::clone(&g);
    Ok(Model {
        name: "granite".to_string(),
        queries: Box::new(move |t| q.embed(t)),
        docs: Box::new(move |t| g.embed(t)),
    })
}

fn static_table(models: &Path, scratch: &Scratch) -> Result<PathBuf, String> {
    if let Some(p) = std::env::var_os("NOTCHLING_EMBED_STATIC_TABLE") {
        return Ok(PathBuf::from(p));
    }
    let beside = models.join("static-512i8.bin");
    if beside.is_file() {
        return Ok(beside);
    }
    let out = scratch.join("static-512i8.bin");
    convert::convert(&models.join(STATIC_DIR), &out)?;
    Ok(out)
}

fn static_present(models: &Path) -> bool {
    std::env::var_os("NOTCHLING_EMBED_STATIC_TABLE").is_some()
        || models.join("static-512i8.bin").is_file()
        || models.join(STATIC_DIR).is_dir()
}

fn static_model(models: &Path, scratch: &Scratch) -> Result<Model, String> {
    let table = static_table(models, scratch)?;
    let tokenizer = models
        .join(STATIC_DIR)
        .join("0_StaticEmbedding/tokenizer.json");
    let s = Rc::new(StaticEmbedder::open(&table, &tokenizer)?);
    let q = Rc::clone(&s);
    Ok(Model {
        name: "static".to_string(),
        queries: Box::new(move |t| q.embed(t).map_err(|e| format!("{e:?}"))),
        docs: Box::new(move |t| s.embed(t).map_err(|e| format!("{e:?}"))),
    })
}

struct F2llm {
    model: RefCell<qwen3::Model>,
    tokens: Tokens,
}

impl F2llm {
    fn open(dir: &Path) -> Result<F2llm, String> {
        let text = std::fs::read_to_string(dir.join("config.json"))
            .map_err(|e| format!("{}: {e}", dir.display()))?;
        let config: qwen3::Config = serde_json::from_str(&text).map_err(|e| e.to_string())?;
        let file = dir.join("model.safetensors");
        let vb = unsafe { VarBuilder::from_mmaped_safetensors(&[&file], DType::F32, &Device::Cpu) }
            .map_err(|e| e.to_string())?;
        let model = qwen3::Model::new(&config, vb).map_err(|e| e.to_string())?;
        let tokens = Tokens::from_tokenizer_json(&dir.join("tokenizer.json"))?;
        Ok(F2llm {
            model: RefCell::new(model),
            tokens,
        })
    }

    fn one(&self, text: &str) -> candle_core::Result<Vec<f32>> {
        let ids = self.tokens.encode(text, F2LLM_MAX_TOKENS);
        let n = ids.len().max(1);
        let input = Tensor::new(ids.as_slice(), &Device::Cpu)?.unsqueeze(0)?;
        let mut model = self.model.borrow_mut();
        model.clear_kv_cache();
        let hidden = model.forward(&input, 0)?;
        let last = hidden.narrow(1, n - 1, 1)?.squeeze(1)?.squeeze(0)?;
        let mut v: Vec<f32> = last.to_dtype(DType::F32)?.to_vec1()?;
        let norm = v.iter().map(|x| x * x).sum::<f32>().sqrt().max(1e-12);
        v.iter_mut().for_each(|x| *x /= norm);
        Ok(v)
    }

    fn embed(&self, texts: &[&str], prefix: &str) -> Result<Vec<Vec<f32>>, String> {
        texts
            .iter()
            .map(|t| self.one(&format!("{prefix}{t}")).map_err(|e| e.to_string()))
            .collect()
    }
}

fn f2llm(dir: &Path) -> Result<Model, String> {
    let m = Rc::new(F2llm::open(dir)?);
    let q = Rc::clone(&m);
    Ok(Model {
        name: "f2llm".to_string(),
        queries: Box::new(move |t| q.embed(t, F2LLM_PROMPT)),
        docs: Box::new(move |t| m.embed(t, "")),
    })
}

fn load_queries(path: &Path, corpus: &Corpus) -> Result<(Vec<Query>, Vec<BadLine>), String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let keys: HashSet<&str> = corpus.docs.iter().map(|d| d.key.as_str()).collect();
    Ok(eval::parse_queries(&text, &|id| keys.contains(id)))
}

fn top5(ids: &[String]) -> String {
    ids.iter().take(5).cloned().collect::<Vec<_>>().join(" ")
}

fn go(args: Args) -> Result<(), String> {
    let (corpus, set_queries) = match (&args.set, &args.data_root) {
        (Some(set), _) => {
            let (c, q) = run::research_set(set)?;
            (c, Some(q))
        }
        (None, Some(root)) => (run::data_root(root)?, None),
        _ => return Err(USAGE.to_string()),
    };
    eprintln!("{} docs", corpus.docs.len());
    if let Some(words) = &args.find {
        for line in run::find(&corpus, words)? {
            println!("{line}");
        }
        return Ok(());
    }
    let mut bad = Vec::new();
    let queries = match (set_queries, &args.queries) {
        (Some(q), _) => q,
        (None, Some(path)) => {
            let (q, b) = load_queries(path, &corpus)?;
            bad = b;
            q
        }
        _ => return Err(USAGE.to_string()),
    };
    for b in &bad {
        eprintln!("queries line {}: {}", b.line, b.why);
    }
    if queries.is_empty() {
        return Err("no query to score".to_string());
    }
    eprintln!("{} queries", queries.len());
    let models_path = models_dir(&args)?;
    let scratch = Scratch::new("nl-eval")?;
    let mut models = vec![granite(&models_path)?];
    let mut combos = vec![vec![0]];
    if static_present(&models_path) {
        models.push(static_model(&models_path, &scratch)?);
        combos.push(vec![0, 1]);
    } else {
        eprintln!(
            "static: no {STATIC_DIR} or static-512i8.bin in {}, scoring granite alone",
            models_path.display()
        );
    }
    for (name, dir) in &args.extra {
        if name != "f2llm" {
            return Err(format!("no extra model called {name}; known: f2llm"));
        }
        models.push(f2llm(dir)?);
        combos.push(vec![models.len() - 1]);
    }
    let e = run::evaluate(&corpus, queries, &models, &combos)?;
    let sweeps: Vec<_> = if args.sweep {
        (0..models.len())
            .map(|m| (models[m].name.clone(), run::floors(&e, m)))
            .collect()
    } else {
        Vec::new()
    };
    if args.json {
        let out = serde_json::json!({
            "report": e.report,
            "floors": sweeps.iter().map(|(n, r)| serde_json::json!({"model": n, "rows": r})).collect::<Vec<_>>(),
            "skipped": bad.iter().map(|b| serde_json::json!({"line": b.line, "why": b.why})).collect::<Vec<_>>(),
        });
        println!("{out}");
        return Ok(());
    }
    print!("{}", eval::render(&e.report));
    for (name, rows) in &sweeps {
        println!("\nfloors for {name}: R@5 and the share of meaning-only hits below the floor");
        for r in rows {
            println!(
                "  {:.2}  R@5 {:.3}  meaning-only {}  below {:.3}",
                r.floor, r.r5, r.meaning_only, r.lost
            );
        }
    }
    if args.verbose {
        for (i, q) in e.queries.iter().enumerate() {
            println!("\nquery {} (want {})", i + 1, q.want.join(" "));
            for (name, rankings) in &e.systems {
                println!("  {name}: {}", top5(&rankings[i]));
            }
        }
    }
    Ok(())
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(args) = parse(&args) else {
        eprintln!("{USAGE}");
        return ExitCode::from(2);
    };
    match go(args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("eval: {e}");
            ExitCode::from(1)
        }
    }
}
