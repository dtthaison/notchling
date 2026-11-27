use super::lanes::{config_dirs, Roots};
use crate::search::{claude_line, first_line_title, SessionRole, TranscriptLine, MAX_LINE_BYTES};
use std::cmp::Reverse;
use std::collections::{HashMap, HashSet};
use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;
use std::time::UNIX_EPOCH;

pub const PAGE: usize = 30;
pub const HEAD_BYTES: u64 = 128 * 1024;
pub const TAIL_BYTES: u64 = 32 * 1024;
const CACHE_MAX: usize = 4_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Ended,
    Past,
}

impl Kind {
    fn rank(self) -> u8 {
        match self {
            Kind::Ended => 0,
            Kind::Past => 1,
        }
    }

    fn mark(self) -> char {
        match self {
            Kind::Ended => 'e',
            Kind::Past => 'p',
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Item {
    pub kind: Kind,
    pub id: String,
    pub title: String,
    pub folder: String,
    pub config_dir: Option<String>,
    pub path: Option<String>,
    pub modified_ms: i64,
}

type Key = (u8, Reverse<i64>, String);

impl Item {
    fn key(&self) -> Key {
        (self.kind.rank(), Reverse(self.modified_ms), self.id.clone())
    }

    pub fn cursor(&self) -> String {
        format!("{}{}:{}", self.kind.mark(), self.modified_ms, self.id)
    }
}

fn parse_cursor(text: &str) -> Option<Key> {
    let mut chars = text.chars();
    let rank = match chars.next()? {
        'e' => 0,
        'p' => 1,
        _ => return None,
    };
    let (ms, id) = chars.as_str().split_once(':')?;
    Some((rank, Reverse(ms.parse().ok()?), id.to_string()))
}

pub fn valid_cursor(text: &str) -> bool {
    parse_cursor(text).is_some()
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Source {
    pub projects: PathBuf,
    pub config_dir: Option<String>,
}

pub fn sources(roots: &Roots) -> Vec<Source> {
    config_dirs(roots)
        .into_iter()
        .map(|c| Source {
            projects: c.dir.join("projects"),
            config_dir: c.lane.map(|_| c.dir.to_string_lossy().into_owned()),
        })
        .collect()
}

#[derive(Clone, Debug)]
struct Entry {
    id: String,
    path: PathBuf,
    config_dir: Option<String>,
    modified_ms: i64,
    size: u64,
}

fn real_inside(path: &Path, root: &Path) -> Option<PathBuf> {
    fs::canonicalize(path)
        .ok()
        .filter(|real| real.starts_with(root))
}

fn scan(sources: &[Source], since_ms: Option<i64>) -> Vec<Entry> {
    let mut seen: HashSet<PathBuf> = HashSet::new();
    let mut by_id: HashMap<String, Entry> = HashMap::new();
    for source in sources {
        let Ok(root) = fs::canonicalize(&source.projects) else {
            continue;
        };
        let Ok(folders) = fs::read_dir(&root) else {
            continue;
        };
        for folder in folders.flatten() {
            let Ok(kind) = folder.file_type() else {
                continue;
            };
            let folder_real = if kind.is_dir() {
                folder.path()
            } else if kind.is_symlink() {
                let Some(real) = real_inside(&folder.path(), &root) else {
                    continue;
                };
                real
            } else {
                continue;
            };
            let Ok(files) = fs::read_dir(&folder_real) else {
                continue;
            };
            for file in files.flatten() {
                let name = file.file_name().to_string_lossy().into_owned();
                let Some(id) = name.strip_suffix(".jsonl") else {
                    continue;
                };
                if id.starts_with("agent-") {
                    continue;
                }
                let Ok(kind) = file.file_type() else {
                    continue;
                };
                let path = if kind.is_file() {
                    folder_real.join(&name)
                } else if kind.is_symlink() {
                    let Some(real) = real_inside(&file.path(), &root) else {
                        continue;
                    };
                    real
                } else {
                    continue;
                };
                let Ok(meta) = fs::metadata(&path) else {
                    continue;
                };
                if !meta.is_file() || meta.len() == 0 {
                    continue;
                }
                let Some(modified_ms) = meta
                    .modified()
                    .ok()
                    .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                    .and_then(|d| i64::try_from(d.as_millis()).ok())
                else {
                    continue;
                };
                if since_ms.is_some_and(|since| modified_ms < since) {
                    continue;
                }
                if !path.starts_with(&root) || !seen.insert(path.clone()) {
                    continue;
                }
                if by_id.get(id).is_some_and(|e| e.modified_ms >= modified_ms) {
                    continue;
                }
                by_id.insert(
                    id.to_string(),
                    Entry {
                        id: id.to_string(),
                        path,
                        config_dir: source.config_dir.clone(),
                        modified_ms,
                        size: meta.len(),
                    },
                );
            }
        }
    }
    let mut out: Vec<Entry> = by_id.into_values().collect();
    out.sort_by(|a, b| {
        b.modified_ms
            .cmp(&a.modified_ms)
            .then_with(|| a.id.cmp(&b.id))
    });
    out
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Head {
    title: String,
    folder: String,
}

fn open_plain(path: &Path) -> Option<File> {
    let mut options = fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW);
    }
    options.open(path).ok()
}

fn window(path: &Path, size: u64) -> Option<Vec<u8>> {
    let mut file = open_plain(path)?;
    if size <= HEAD_BYTES + TAIL_BYTES {
        let mut all = Vec::new();
        file.take(size).read_to_end(&mut all).ok()?;
        return Some(all);
    }
    let mut head = Vec::new();
    (&mut file).take(HEAD_BYTES).read_to_end(&mut head).ok()?;
    head.truncate(head.iter().rposition(|&b| b == b'\n').map_or(0, |i| i + 1));
    file.seek(SeekFrom::Start(size - TAIL_BYTES)).ok()?;
    let mut tail = Vec::new();
    file.take(TAIL_BYTES).read_to_end(&mut tail).ok()?;
    if let Some(cut) = tail.iter().position(|&b| b == b'\n') {
        head.extend_from_slice(&tail[cut + 1..]);
    }
    Some(head)
}

fn summary_of(line: &[u8]) -> Option<String> {
    let marker = br#""type":"summary""#;
    if !line.windows(marker.len()).any(|w| w == marker) {
        return None;
    }
    let value: serde_json::Value = serde_json::from_slice(line).ok()?;
    let text = value.get("summary")?.as_str()?.trim();
    (!text.is_empty()).then(|| text.to_string())
}

fn head_of(data: &[u8]) -> Option<Head> {
    let (mut custom, mut ai, mut summary, mut prompt, mut folder) = (None, None, None, None, None);
    let mut messages = 0usize;
    for line in data.split(|&b| b == b'\n') {
        if line.is_empty() || line.len() > MAX_LINE_BYTES {
            continue;
        }
        if let Some(text) = summary_of(line) {
            summary = Some(text);
            continue;
        }
        match claude_line(line) {
            Some(TranscriptLine::CustomTitle(t)) => custom = Some(t),
            Some(TranscriptLine::AiTitle(t)) => ai = Some(t),
            Some(TranscriptLine::Message { role, text, cwd }) => {
                messages += 1;
                if folder.is_none() {
                    folder = cwd.filter(|c| !c.is_empty());
                }
                if role == SessionRole::User && prompt.is_none() {
                    prompt = Some(text);
                }
            }
            None => {}
        }
    }
    if messages == 0 {
        return None;
    }
    let folder = folder.unwrap_or_default();
    let title = custom
        .or(ai)
        .or(summary)
        .or(prompt)
        .map(|t| first_line_title(t.trim()))
        .filter(|t| !t.is_empty())
        .or_else(|| {
            Path::new(&folder)
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
        })
        .unwrap_or_default();
    Some(Head { title, folder })
}

type Cached = (i64, u64, Option<Head>);

#[derive(Default)]
pub struct History {
    heads: Mutex<HashMap<PathBuf, Cached>>,
    reads: AtomicUsize,
}

pub struct Query<'a> {
    pub sources: &'a [Source],
    pub ended: Vec<Item>,
    pub skip: &'a HashSet<String>,
    pub since_ms: Option<i64>,
    pub keep: &'a dyn Fn(&str, i64) -> bool,
    pub after: Option<&'a str>,
    pub limit: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Page {
    pub items: Vec<Item>,
    pub next: Option<String>,
}

impl History {
    pub fn new() -> History {
        History::default()
    }

    pub fn head_reads(&self) -> usize {
        self.reads.load(Ordering::Relaxed)
    }

    fn head(&self, entry: &Entry) -> Option<Head> {
        let heads = self.heads.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((modified, size, head)) = heads.get(&entry.path) {
            if *modified == entry.modified_ms && *size == entry.size {
                return head.clone();
            }
        }
        drop(heads);
        self.reads.fetch_add(1, Ordering::Relaxed);
        let head = window(&entry.path, entry.size).and_then(|data| head_of(&data));
        let mut heads = self.heads.lock().unwrap_or_else(|e| e.into_inner());
        if heads.len() >= CACHE_MAX {
            heads.clear();
        }
        heads.insert(
            entry.path.clone(),
            (entry.modified_ms, entry.size, head.clone()),
        );
        head
    }

    pub fn page(&self, q: &Query) -> Page {
        let after = q.after.and_then(parse_cursor);
        let mut items: Vec<Item> = q
            .ended
            .iter()
            .filter(|i| (q.keep)(&i.folder, i.modified_ms))
            .filter(|i| q.since_ms.is_none_or(|s| i.modified_ms >= s))
            .filter(|i| after.as_ref().is_none_or(|k| i.key() > *k))
            .cloned()
            .collect();
        items.sort_by_key(Item::key);
        let want = q.limit + 1;
        if items.len() < want {
            for entry in scan(q.sources, q.since_ms) {
                if q.skip.contains(&entry.id) {
                    continue;
                }
                let key = (
                    Kind::Past.rank(),
                    Reverse(entry.modified_ms),
                    entry.id.clone(),
                );
                if after.as_ref().is_some_and(|k| key <= *k) {
                    continue;
                }
                let Some(head) = self.head(&entry) else {
                    continue;
                };
                if !(q.keep)(&head.folder, entry.modified_ms) {
                    continue;
                }
                items.push(Item {
                    kind: Kind::Past,
                    id: entry.id,
                    title: head.title,
                    folder: head.folder,
                    config_dir: entry.config_dir,
                    path: Some(entry.path.to_string_lossy().into_owned()),
                    modified_ms: entry.modified_ms,
                });
                if items.len() >= want {
                    break;
                }
            }
        }
        let next = (items.len() > q.limit).then(|| {
            items.truncate(q.limit);
            items.last().map(Item::cursor)
        });
        Page {
            items,
            next: next.flatten(),
        }
    }
}

#[cfg(test)]
#[path = "history_tests.rs"]
mod tests;
