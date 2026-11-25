use super::chunk::{page_chunks, ChunkText};
use super::fts::{FtsDoc, SearchDb};
use super::index::Refresh;
use super::{snippet, IndexedSession, PageEntry, SessionMeta, DEFAULT_RADIUS};
use crate::pages::PageLibrary;
use std::collections::{HashMap, HashSet};

pub const CORPUS_PAGES: &str = "pages";
const MAX_FILE_BYTES: u64 = 4_000_000;
const VERSION: &str = "version";
const BATCH: usize = 25;

#[derive(Clone, Debug, PartialEq)]
pub struct PageContentHit {
    pub key: String,
    pub score: f64,
}

pub fn page_key(entry: &PageEntry) -> String {
    format!("page:{}/{}", entry.folder, entry.file)
}

fn is_mime_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '.' | '+' | '-')
}

fn data_url_end(rest: &str) -> Option<usize> {
    let after = rest.get(..5).filter(|p| p.eq_ignore_ascii_case("data:"))?;
    let tail = &rest[after.len()..];
    let kind = tail.find(|c: char| !is_mime_char(c))?;
    let tail = &tail[kind..];
    let sub = tail.strip_prefix('/')?;
    let name = sub.find(|c: char| !is_mime_char(c))?;
    let sub = &sub[name..];
    sub.starts_with([';', ','])
        .then(|| rest.len() - sub.len() + sub.find(is_url_stop).unwrap_or(sub.len()))
}

fn is_url_stop(c: char) -> bool {
    c.is_whitespace() || matches!(c, '"' | '\'' | '(' | ')' | '<' | '>')
}

fn without_noise(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut i = 0;
    let mut prev_word = false;
    while i < html.len() {
        let rest = &html[i..];
        if !prev_word {
            if let Some(end) = data_url_end(rest) {
                out.push(' ');
                i += end;
                prev_word = false;
                continue;
            }
        }
        if rest.starts_with("<!") && !rest.starts_with("<!--") {
            i += rest.find('>').map_or(rest.len(), |n| n + 1);
            prev_word = false;
            continue;
        }
        let Some(c) = rest.chars().next() else {
            break;
        };
        out.push(c);
        prev_word = c.is_alphanumeric();
        i += c.len_utf8();
    }
    out
}

pub fn page_chunks_of(html: &str) -> Vec<ChunkText> {
    page_chunks(&without_noise(html))
}

pub fn page_text(html: &str) -> String {
    page_chunks_of(html)
        .into_iter()
        .map(|c| c.text)
        .collect::<Vec<_>>()
        .join("\n\n")
}

fn read_page(library: &PageLibrary, entry: &PageEntry) -> Option<String> {
    let path = library.local_path(entry)?;
    let meta = std::fs::metadata(&path).ok()?;
    if !meta.is_file() || meta.len() > MAX_FILE_BYTES {
        return None;
    }
    let bytes = std::fs::read(&path).ok()?;
    Some(page_text(&String::from_utf8_lossy(&bytes)))
}

pub fn refresh(db: &SearchDb, library: &PageLibrary) -> Refresh {
    let known: HashMap<String, (f64, Option<String>)> = db
        .corpus_stamps(CORPUS_PAGES)
        .into_iter()
        .map(|s| {
            let version = s
                .facets
                .iter()
                .find(|(k, _)| k == VERSION)
                .map(|(_, v)| v.clone());
            (s.key, (s.at, version))
        })
        .collect();
    let mut stats = Refresh::default();
    let mut live: HashSet<String> = HashSet::new();
    let mut batch: Vec<FtsDoc> = Vec::new();
    for entry in library.entries() {
        let key = page_key(&entry);
        if known.get(&key) == Some(&(entry.updated, Some(entry.version.clone()))) {
            live.insert(key);
            stats.reused += 1;
            continue;
        }
        let Some(body) = read_page(library, &entry) else {
            continue;
        };
        live.insert(key.clone());
        stats.read += 1;
        batch.push(FtsDoc {
            key,
            title: entry.title.clone(),
            folder: entry.project.clone(),
            body,
            at: entry.updated,
            facets: vec![(VERSION.into(), entry.version.clone())],
        });
        if batch.len() >= BATCH {
            let _ = db.put_docs(CORPUS_PAGES, &std::mem::take(&mut batch));
        }
    }
    let _ = db.put_docs(CORPUS_PAGES, &batch);
    let gone: Vec<String> = known.into_keys().filter(|k| !live.contains(k)).collect();
    stats.dropped = gone.len();
    let _ = db.remove_corpus_docs(CORPUS_PAGES, &gone);
    stats
}

pub fn search<W: AsRef<str>>(
    db: &SearchDb,
    words: &[W],
    now: f64,
    limit: usize,
) -> Vec<PageContentHit> {
    db.rank_corpus(CORPUS_PAGES, words, now, limit)
        .into_iter()
        .map(|(key, score)| PageContentHit { key, score })
        .collect()
}

pub fn snippet_of<W: AsRef<str>>(db: &SearchDb, entry: &PageEntry, words: &[W]) -> Option<String> {
    let body = db.doc_body(CORPUS_PAGES, &page_key(entry))?;
    let meta = SessionMeta {
        key: page_key(entry),
        tool_key: String::new(),
        session_id: String::new(),
        title: entry.title.clone(),
        folder: entry.project.clone(),
        path: String::new(),
        config_dir: None,
        modified: entry.updated,
        size: 0,
        parent: None,
        account: None,
        started: None,
    };
    snippet(&IndexedSession::new(meta, body), words, DEFAULT_RADIUS)
}
