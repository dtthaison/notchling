use super::locate::project_folder;
use super::model::Timestamp;
use super::tail::read_lines;
use super::util::{date_field, str_of};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, SystemTime};

const TAIL_BYTES: u64 = 65_536;
const MAX_CHAIN: usize = 8;
const MAX_FOLDERS: usize = 64;
const HOT: Duration = Duration::from_secs(600);

#[derive(Clone, Debug, PartialEq)]
struct Link {
    to: String,
    at: Timestamp,
}

#[derive(Clone)]
struct Entry {
    size: u64,
    modified: SystemTime,
    links: Vec<Link>,
}

struct Before {
    path: String,
    id: String,
    at: Timestamp,
}

struct Folder {
    modified: SystemTime,
    files: HashMap<String, Entry>,
    before: HashMap<String, Vec<Before>>,
    reads: u64,
}

#[derive(Default)]
struct Cache {
    folders: HashMap<String, (u64, Arc<Folder>)>,
    clock: u64,
}

fn cache() -> &'static Mutex<Cache> {
    static CACHE: OnceLock<Mutex<Cache>> = OnceLock::new();
    CACHE.get_or_init(Mutex::default)
}

fn stem(path: &str) -> Option<&str> {
    Path::new(path).file_stem()?.to_str()
}

fn link(line: &str, own_id: &str) -> Option<Link> {
    let obj: Value = serde_json::from_str(line).ok()?;
    if str_of(&obj, "type")? != "continued-in" {
        return None;
    }
    if str_of(&obj, "sessionId").is_some_and(|from| from != own_id) {
        return None;
    }
    let to = str_of(&obj, "continuedInSessionId").filter(|to| !to.is_empty() && *to != own_id)?;
    Some(Link {
        to: to.to_string(),
        at: date_field(&obj).unwrap_or(0.0),
    })
}

fn read_links(path: &str) -> Vec<Link> {
    let Some(own_id) = stem(path) else {
        return Vec::new();
    };
    let Ok(lines) = read_lines(path, TAIL_BYTES) else {
        return Vec::new();
    };
    lines
        .iter()
        .filter(|line| line.contains("continued-in"))
        .filter_map(|line| link(line, own_id))
        .collect()
}

fn entry_of(path: &str, size: u64, modified: SystemTime) -> Entry {
    Entry {
        size,
        modified,
        links: if size == 0 {
            Vec::new()
        } else {
            read_links(path)
        },
    }
}

fn index(files: &HashMap<String, Entry>) -> HashMap<String, Vec<Before>> {
    let mut before: HashMap<String, Vec<Before>> = HashMap::new();
    for (path, entry) in files {
        let Some(id) = stem(path) else { continue };
        for link in &entry.links {
            before.entry(link.to.clone()).or_default().push(Before {
                path: path.clone(),
                id: id.to_string(),
                at: link.at,
            });
        }
    }
    before
}

fn folder_of(files: HashMap<String, Entry>, modified: SystemTime, reads: u64) -> Arc<Folder> {
    Arc::new(Folder {
        modified,
        before: index(&files),
        files,
        reads,
    })
}

fn stat(path: &str) -> Option<(u64, SystemTime)> {
    let meta = fs::metadata(path).ok()?;
    meta.is_file().then_some(())?;
    Some((meta.len(), meta.modified().ok()?))
}

fn relist(dir: &Path, modified: SystemTime, old: Option<&Folder>) -> Arc<Folder> {
    let mut files = HashMap::new();
    let mut reads = old.map_or(0, |o| o.reads);
    let listing = fs::read_dir(dir).into_iter().flatten().flatten();
    for item in listing {
        let path = item.path();
        if path.extension().is_none_or(|x| x != "jsonl") {
            continue;
        }
        let Some(name) = path.to_str() else { continue };
        let Some((size, mtime)) = item
            .metadata()
            .ok()
            .filter(|m| m.is_file())
            .and_then(|m| Some((m.len(), m.modified().ok()?)))
        else {
            continue;
        };
        let kept = old
            .and_then(|o| o.files.get(name))
            .filter(|e| e.size == size && e.modified == mtime);
        let entry = match kept {
            Some(entry) => entry.clone(),
            None => {
                reads += u64::from(size > 0);
                entry_of(name, size, mtime)
            }
        };
        files.insert(name.to_string(), entry);
    }
    folder_of(files, modified, reads)
}

fn touched(old: &Arc<Folder>, now: SystemTime) -> Option<Arc<Folder>> {
    let mut reread = Vec::new();
    let mut gone = Vec::new();
    for (path, entry) in &old.files {
        let hot = now
            .duration_since(entry.modified)
            .map_or(true, |age| age < HOT);
        if !hot {
            continue;
        }
        match stat(path) {
            Some((size, mtime)) if size == entry.size && mtime == entry.modified => {}
            Some((size, mtime)) => reread.push((path, size, mtime)),
            None => gone.push(path),
        }
    }
    if reread.is_empty() && gone.is_empty() {
        return None;
    }
    let mut files = old.files.clone();
    for path in gone {
        files.remove(path);
    }
    let reads = old.reads + reread.iter().filter(|(_, size, _)| *size > 0).count() as u64;
    for (path, size, mtime) in reread {
        files.insert(path.clone(), entry_of(path, size, mtime));
    }
    Some(folder_of(files, old.modified, reads))
}

fn folder(dir: &Path) -> Option<Arc<Folder>> {
    let key = dir.to_string_lossy().into_owned();
    let modified = fs::metadata(dir).ok()?.modified().ok()?;
    let known = {
        let mut cache = cache().lock().unwrap_or_else(|e| e.into_inner());
        cache.clock += 1;
        let clock = cache.clock;
        cache.folders.get_mut(&key).map(|(used, folder)| {
            *used = clock;
            folder.clone()
        })
    };
    let fresh = match &known {
        Some(old) if old.modified == modified => match touched(old, SystemTime::now()) {
            Some(fresh) => fresh,
            None => return known,
        },
        _ => relist(dir, modified, known.as_deref()),
    };
    let mut cache = cache().lock().unwrap_or_else(|e| e.into_inner());
    if cache.folders.len() >= MAX_FOLDERS && !cache.folders.contains_key(&key) {
        let oldest = cache
            .folders
            .iter()
            .min_by_key(|(_, (used, _))| *used)
            .map(|(k, _)| k.clone());
        if let Some(oldest) = oldest {
            cache.folders.remove(&oldest);
        }
    }
    let clock = cache.clock;
    cache.folders.insert(key, (clock, fresh.clone()));
    Some(fresh)
}

pub fn earlier(path: &str) -> Vec<String> {
    let Some(dir) = Path::new(path).parent() else {
        return Vec::new();
    };
    let Some(folder) = folder(dir) else {
        return Vec::new();
    };
    let Some(first) = stem(path) else {
        return Vec::new();
    };
    let mut seen: HashSet<String> = HashSet::from([first.to_string()]);
    let mut current = first.to_string();
    let mut chain = Vec::new();
    while chain.len() < MAX_CHAIN {
        let next = folder
            .before
            .get(&current)
            .into_iter()
            .flatten()
            .filter(|b| !seen.contains(&b.id))
            .max_by(|a, b| a.at.total_cmp(&b.at).then_with(|| a.path.cmp(&b.path)));
        let Some(next) = next else { break };
        seen.insert(next.id.clone());
        current.clone_from(&next.id);
        chain.push(next.path.clone());
    }
    chain.reverse();
    chain
}

pub fn find_earlier(projects_dir: &str, session_id: &str, cwd: &str) -> Option<String> {
    let mut cwds = vec![cwd.to_string()];
    if let Some(rest) = cwd.strip_prefix("/private") {
        cwds.push(rest.to_string());
    } else if ["/tmp", "/var", "/etc"]
        .iter()
        .any(|p| cwd == *p || cwd.starts_with(&format!("{p}/")))
    {
        cwds.push(format!("/private{cwd}"));
    }
    cwds.iter().find_map(|c| {
        let guess = Path::new(projects_dir)
            .join(project_folder(c))
            .join(format!("{session_id}.jsonl"));
        earlier(&guess.to_string_lossy()).pop()
    })
}

#[cfg(test)]
pub(crate) fn tail_reads(dir: &Path) -> u64 {
    cache()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .folders
        .get(&dir.to_string_lossy().into_owned())
        .map_or(0, |(_, f)| f.reads)
}

#[cfg(test)]
#[path = "continued_tests.rs"]
mod tests;
