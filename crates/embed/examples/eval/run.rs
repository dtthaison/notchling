use notchling_embed::eval::{self, FloorRow, Query, Report};
use notchling_engine::search::chunk::{session_chunks, turns};
use notchling_engine::search::fts::SearchDb;
use notchling_engine::search::query::fuse::{rrf, List, K};
use notchling_engine::search::query::shape::text_only;
use notchling_engine::search::query::words;
use notchling_engine::search::{
    claude_line, codex_id, codex_line, IndexedSession, SessionMeta, SessionTextBuilder,
    TranscriptLine,
};
use notchling_engine::transcript::codex as codex_transcript;
use std::collections::HashSet;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

pub const DEPTH: usize = 50;

pub struct Doc {
    pub key: String,
    pub title: String,
    pub folder: String,
    pub text: String,
    pub chunks: Vec<String>,
    pub modified: f64,
}

pub struct Corpus {
    pub docs: Vec<Doc>,
    pub now: f64,
}

type Embed = Box<dyn Fn(&[&str]) -> Result<Vec<Vec<f32>>, String>>;

pub struct Model {
    pub name: String,
    pub queries: Embed,
    pub docs: Embed,
}

pub type Scored = Vec<Vec<(String, f32)>>;

pub struct Evaluation {
    pub report: Report,
    pub queries: Vec<Query>,
    pub exact: Vec<bool>,
    pub text: Vec<Vec<String>>,
    pub dense: Vec<(String, Scored)>,
    pub systems: Vec<(String, Vec<Vec<String>>)>,
}

fn clock() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

pub fn research_set(path: &Path) -> Result<(Corpus, Vec<Query>), String> {
    let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let set: serde_json::Value =
        serde_json::from_slice(&bytes).map_err(|e| format!("{}: {e}", path.display()))?;
    let now = clock();
    let docs = set["docs"]
        .as_object()
        .ok_or("the set has no docs")?
        .iter()
        .map(|(key, text)| {
            let text = text.as_str().unwrap_or_default().to_string();
            Doc {
                key: key.clone(),
                title: String::new(),
                folder: String::new(),
                chunks: vec![text.clone()],
                text,
                modified: now,
            }
        })
        .collect();
    let queries = set["queries"]
        .as_array()
        .ok_or("the set has no queries")?
        .iter()
        .filter_map(|q| {
            Some(Query {
                q: q.get(0)?.as_str()?.to_string(),
                want: q
                    .get(1)?
                    .as_array()?
                    .iter()
                    .filter_map(|w| w.as_str().map(str::to_string))
                    .collect(),
                lang: q.get(2).and_then(|l| l.as_str()).map(str::to_string),
            })
        })
        .collect();
    Ok((Corpus { docs, now }, queries))
}

fn jsonl_in(dir: &Path, depth: usize, prefix: &str, out: &mut Vec<PathBuf>) {
    let Ok(read) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in read.flatten() {
        let path = entry.path();
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        if kind.is_dir() && depth > 0 {
            jsonl_in(&path, depth - 1, prefix, out);
        } else if kind.is_file() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.ends_with(".jsonl") && name.starts_with(prefix) {
                out.push(path);
            }
        }
    }
}

fn read_lines(
    path: &Path,
    from: u64,
    parse: fn(&[u8]) -> Option<TranscriptLine>,
) -> Vec<TranscriptLine> {
    use std::io::{Seek, SeekFrom};
    let Ok(mut file) = std::fs::File::open(path) else {
        return Vec::new();
    };
    if file.seek(SeekFrom::Start(from)).is_err() {
        return Vec::new();
    }
    BufReader::new(file)
        .split(b'\n')
        .map_while(Result::ok)
        .filter(|l| !l.is_empty())
        .filter_map(|l| parse(&l))
        .collect()
}

fn doc_from(
    key: String,
    path: &Path,
    lines: Vec<TranscriptLine>,
    folder: Option<String>,
    title: Option<String>,
) -> Option<Doc> {
    let mut builder = SessionTextBuilder::new();
    for line in &lines {
        builder.add(line.clone());
    }
    let text = builder.text();
    if text.is_empty() {
        return None;
    }
    let folder = folder.unwrap_or_else(|| builder.cwd().unwrap_or("").to_string());
    let title = title.unwrap_or_else(|| builder.title());
    let chunks: Vec<String> = session_chunks(&title, &folder, &turns(lines, true))
        .into_iter()
        .map(|c| c.text)
        .collect();
    if chunks.is_empty() {
        return None;
    }
    let modified = std::fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|m| m.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0);
    Some(Doc {
        key,
        title,
        folder,
        text,
        chunks,
        modified,
    })
}

pub fn data_root(root: &Path) -> Result<Corpus, String> {
    let claude = [root.join("claude/projects"), root.join("claude")]
        .into_iter()
        .find(|p| p.is_dir());
    let codex = [root.join("codex/sessions"), root.join("codex")]
        .into_iter()
        .find(|p| p.is_dir());
    if claude.is_none() && codex.is_none() {
        return Err(format!("{} has neither claude/ nor codex/", root.display()));
    }
    let mut docs = Vec::new();
    if let Some(dir) = claude {
        let mut files = Vec::new();
        jsonl_in(&dir, 1, "", &mut files);
        files.sort();
        for path in files {
            let Some(id) = path.file_stem().and_then(|s| s.to_str()) else {
                continue;
            };
            let lines = read_lines(&path, 0, claude_line);
            docs.extend(doc_from(format!("claude:{id}"), &path, lines, None, None));
        }
    }
    if let Some(dir) = codex {
        let mut files = Vec::new();
        jsonl_in(&dir, 4, "rollout-", &mut files);
        files.sort();
        for path in files {
            let Some(name) = path.file_name().and_then(|s| s.to_str()) else {
                continue;
            };
            let key = format!("codex:{}", codex_id(name));
            let text_path = path.to_string_lossy().into_owned();
            let Some(info) = codex_transcript::meta(&text_path) else {
                continue;
            };
            let from = codex_transcript::fork::own_start(&text_path);
            let lines = read_lines(&path, from, codex_line);
            docs.extend(doc_from(key, &path, lines, Some(info.cwd), info.title));
        }
    }
    let mut seen = HashSet::new();
    docs.retain(|d| seen.insert(d.key.clone()));
    Ok(Corpus { docs, now: clock() })
}

fn text_db(corpus: &Corpus) -> Result<SearchDb, String> {
    let db = SearchDb::memory()?;
    let sessions: Vec<IndexedSession> = corpus
        .docs
        .iter()
        .map(|d| {
            let meta = SessionMeta {
                key: d.key.clone(),
                tool_key: "claude".to_string(),
                session_id: d.key.clone(),
                title: d.title.clone(),
                folder: d.folder.clone(),
                path: String::new(),
                config_dir: None,
                modified: d.modified,
                size: 0,
                parent: None,
                account: None,
                started: None,
            };
            IndexedSession::new(meta, d.text.clone())
        })
        .collect();
    db.put(&sessions)?;
    Ok(db)
}

pub fn rank_text(corpus: &Corpus, queries: &[Query]) -> Result<Vec<Vec<String>>, String> {
    let db = text_db(corpus)?;
    Ok(queries
        .iter()
        .map(|q| {
            db.rank(&words(&q.q), corpus.now, DEPTH)
                .into_iter()
                .map(|(key, _)| key)
                .collect()
        })
        .collect())
}

pub fn find(corpus: &Corpus, phrase: &str) -> Result<Vec<String>, String> {
    let db = text_db(corpus)?;
    let by_key = |key: &str| corpus.docs.iter().find(|d| d.key == key);
    Ok(db
        .rank(&words(phrase), corpus.now, 10)
        .into_iter()
        .filter_map(|(key, _)| {
            let d = by_key(&key)?;
            Some(format!("{}  {}  {}", d.key, d.title, d.folder))
        })
        .collect())
}

fn dot(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

pub fn rank_dense(corpus: &Corpus, model: &Model, queries: &[Query]) -> Result<Scored, String> {
    let mut starts = Vec::with_capacity(corpus.docs.len());
    let mut texts: Vec<&str> = Vec::new();
    for d in &corpus.docs {
        starts.push(texts.len());
        texts.extend(d.chunks.iter().map(String::as_str));
    }
    starts.push(texts.len());
    eprintln!("{}: {} chunks", model.name, texts.len());
    let mut chunk_vectors = Vec::with_capacity(texts.len());
    for batch in texts.chunks(32) {
        chunk_vectors.extend((model.docs)(batch)?);
    }
    let qs: Vec<&str> = queries.iter().map(|q| q.q.as_str()).collect();
    let query_vectors = (model.queries)(&qs)?;
    Ok(query_vectors
        .iter()
        .map(|qv| {
            let mut scored: Vec<(String, f32)> = corpus
                .docs
                .iter()
                .enumerate()
                .map(|(i, d)| {
                    let best = chunk_vectors[starts[i]..starts[i + 1]]
                        .iter()
                        .map(|c| dot(qv, c))
                        .fold(f32::NEG_INFINITY, f32::max);
                    (d.key.clone(), best)
                })
                .collect();
            scored.sort_by(|a, b| b.1.total_cmp(&a.1));
            scored.truncate(DEPTH);
            scored
        })
        .collect())
}

pub fn fuse(lists: &[&[String]]) -> Vec<String> {
    let lists: Vec<List> = lists.iter().map(|ids| List { ids, weight: 1.0 }).collect();
    rrf(&lists, K).into_iter().map(|(id, _)| id).collect()
}

pub fn evaluate(
    corpus: &Corpus,
    queries: Vec<Query>,
    models: &[Model],
    combos: &[Vec<usize>],
) -> Result<Evaluation, String> {
    let exact: Vec<bool> = queries.iter().map(|q| text_only(&q.q)).collect();
    let text = rank_text(corpus, &queries)?;
    let mut dense = Vec::new();
    for model in models {
        dense.push((model.name.clone(), rank_dense(corpus, model, &queries)?));
    }
    let mut systems = vec![("text".to_string(), text.clone())];
    for (name, lists) in &dense {
        let rankings = lists
            .iter()
            .map(|l| l.iter().map(|(id, _)| id.clone()).collect())
            .collect();
        systems.push((name.clone(), rankings));
    }
    for combo in combos {
        let name = std::iter::once("text".to_string())
            .chain(combo.iter().map(|&m| models[m].name.clone()))
            .collect::<Vec<_>>()
            .join("+");
        let rankings = (0..queries.len())
            .map(|i| {
                let lists: Vec<Vec<String>> = combo
                    .iter()
                    .map(|&m| dense[m].1[i].iter().map(|(id, _)| id.clone()).collect())
                    .collect();
                eval::hybrid(exact[i], &text[i], &lists, &fuse)
            })
            .collect();
        systems.push((name, rankings));
    }
    let report = eval::report(&queries, &exact, &systems);
    Ok(Evaluation {
        report,
        queries,
        exact,
        text,
        dense,
        systems,
    })
}

pub fn floors(e: &Evaluation, model: usize) -> Vec<FloorRow> {
    let wants: Vec<Vec<String>> = e.queries.iter().map(|q| q.want.clone()).collect();
    eval::sweep(&wants, &e.text, &e.dense[model].1, &e.exact, &fuse)
}
