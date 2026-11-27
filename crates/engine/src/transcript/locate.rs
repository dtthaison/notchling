use super::model::Timestamp;
use super::util::{file_times, modified_secs, now_secs};
use std::collections::HashMap;
use std::fs::File;
use std::io::Read;
use std::path::Path;
use std::sync::{Mutex, OnceLock};
use std::time::SystemTime;

pub fn project_folder(cwd: &str) -> String {
    cwd.chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect()
}

pub fn project_folders(cwd: &str) -> Vec<String> {
    let mut cwds = vec![cwd.to_string()];
    if let Some(rest) = cwd.strip_prefix("/private") {
        cwds.push(rest.to_string());
    } else if ["/tmp", "/var", "/etc"]
        .iter()
        .any(|p| cwd == *p || cwd.starts_with(&format!("{p}/")))
    {
        cwds.push(format!("/private{cwd}"));
    }
    cwds.iter().map(|c| project_folder(c)).collect()
}

pub fn find_transcript(projects_dir: &str, session_id: &str, cwd: &str) -> Option<String> {
    let file = format!("{session_id}.jsonl");
    for folder in project_folders(cwd) {
        let guess = Path::new(projects_dir).join(folder).join(&file);
        if guess.is_file() {
            return Some(guess.to_string_lossy().into_owned());
        }
    }
    let mut dirs: Vec<_> = std::fs::read_dir(projects_dir)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .collect();
    dirs.sort();
    dirs.into_iter()
        .map(|d| d.join(&file))
        .find(|p| p.is_file())
        .map(|p| p.to_string_lossy().into_owned())
}

pub fn find_in_folders(projects_dir: &str, ids: &[String], cwds: &[&str]) -> Option<String> {
    let folders: Vec<String> = cwds
        .iter()
        .filter(|c| !c.is_empty())
        .flat_map(|c| project_folders(c))
        .collect();
    let mut best: Option<(String, Timestamp)> = None;
    for id in ids.iter().filter(|id| !id.is_empty()) {
        for folder in &folders {
            let guess = Path::new(projects_dir)
                .join(folder)
                .join(format!("{id}.jsonl"))
                .to_string_lossy()
                .into_owned();
            let Some(date) = modified_secs(&guess) else {
                continue;
            };
            if best.as_ref().is_none_or(|(_, d)| *d < date) {
                best = Some((guess, date));
            }
        }
    }
    best.map(|(path, _)| path)
}

pub fn job_transcript(
    link_scan_path: Option<&str>,
    session_id: Option<&str>,
    worktree_path: Option<&str>,
    modified: impl Fn(&str) -> Option<Timestamp>,
) -> Option<String> {
    let folders: Vec<&str> = worktree_path
        .filter(|w| !w.is_empty())
        .into_iter()
        .collect();
    newest_of(link_scan_path?, &[session_id], &folders, false, modified)
}

pub struct JobPaths<'a> {
    pub link_scan_path: Option<&'a str>,
    pub session_id: Option<&'a str>,
    pub resume_session_id: Option<&'a str>,
    pub worktree_path: Option<&'a str>,
    pub cwd: Option<&'a str>,
}

pub fn job_transcript_live(
    job: &JobPaths,
    modified: impl Fn(&str) -> Option<Timestamp>,
) -> Option<String> {
    let folders: Vec<&str> = [job.worktree_path, job.cwd]
        .into_iter()
        .flatten()
        .filter(|f| !f.is_empty())
        .collect();
    newest_of(
        job.link_scan_path?,
        &[job.resume_session_id, job.session_id],
        &folders,
        true,
        modified,
    )
}

fn newest_of(
    link: &str,
    ids: &[Option<&str>],
    cwds: &[&str],
    live: bool,
    modified: impl Fn(&str) -> Option<Timestamp>,
) -> Option<String> {
    let mut candidates = Vec::new();
    if !live {
        candidates.push(link.to_string());
    }
    if !cwds.is_empty() || live {
        let projects = Path::new(link)
            .parent()
            .and_then(Path::parent)
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default();
        let stem = Path::new(link)
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned());
        let mut named: Vec<String> = Vec::new();
        for id in ids.iter().flatten().map(|id| id.to_string()).chain(stem) {
            if !id.is_empty() && !named.contains(&id) {
                named.push(id);
            }
        }
        let mut folders: Vec<String> = Vec::new();
        if live {
            if let Some(own) = Path::new(link).parent().and_then(Path::file_name) {
                folders.push(own.to_string_lossy().into_owned());
            }
        }
        for folder in cwds.iter().flat_map(|cwd| project_folders(cwd)) {
            if !folders.contains(&folder) {
                folders.push(folder);
            }
        }
        for id in &named {
            for folder in &folders {
                let guess = format!("{projects}/{folder}/{id}.jsonl");
                if !candidates.contains(&guess) {
                    candidates.push(guess);
                }
            }
        }
    }
    if live && !candidates.iter().any(|c| c == link) {
        candidates.push(link.to_string());
    }
    let mut best: Option<(String, Timestamp)> = None;
    for path in candidates {
        let Some(date) = modified(&path) else {
            continue;
        };
        if best.as_ref().is_none_or(|(_, d)| *d < date) {
            best = Some((path, date));
        }
    }
    Some(best.map_or_else(|| link.to_string(), |b| b.0))
}

pub fn job_transcript_on_disk(
    link_scan_path: Option<&str>,
    session_id: Option<&str>,
    worktree_path: Option<&str>,
) -> Option<String> {
    job_transcript(link_scan_path, session_id, worktree_path, modified_secs)
}

type SuccessorCache = HashMap<String, (SystemTime, Option<String>)>;

fn successor_cache() -> &'static Mutex<SuccessorCache> {
    static CACHE: OnceLock<Mutex<SuccessorCache>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

const HEAD_WINDOW: usize = 16_384;
const HEAD_PENDING_FOR: f64 = 60.0;
const MAX_HOPS: usize = 32;

struct Search {
    found: Option<String>,
    settled: bool,
}

enum Head {
    Clear,
    Not,
    Pending,
}

fn remembered(key: String, dir: &str, search: impl FnOnce() -> Search) -> Option<String> {
    let Some(dir_modified) = std::fs::metadata(dir).ok().and_then(|m| m.modified().ok()) else {
        return search().found;
    };
    if let Some((stamp, result)) = successor_cache()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(&key)
    {
        if *stamp == dir_modified {
            return result.clone();
        }
    }
    let result = search();
    if result.settled {
        let mut cache = successor_cache().lock().unwrap_or_else(|e| e.into_inner());
        if cache.len() > 500 {
            cache.clear();
        }
        cache.insert(key, (dir_modified, result.found.clone()));
    }
    result.found
}

pub fn successor(path: &str, name: &str) -> Option<String> {
    let dir = Path::new(path).parent()?.to_string_lossy().into_owned();
    remembered(format!("{path}\n{name}"), &dir, || {
        search_successor(path, name)
    })
}

pub fn predecessor(path: &str, name: &str) -> Option<String> {
    let dir = Path::new(path).parent()?.to_string_lossy().into_owned();
    remembered(format!("before\n{path}\n{name}"), &dir, || {
        search_predecessor(path, name)
    })
}

fn search_predecessor(path: &str, name: &str) -> Search {
    let none = |settled| Search {
        found: None,
        settled,
    };
    if name.is_empty() {
        return none(true);
    }
    let Ok(quoted) = serde_json::to_string(name) else {
        return none(true);
    };
    let marker = format!("\"agentName\":{quoted}");
    match head_state(Path::new(path), &marker) {
        Head::Clear => {}
        Head::Not => return none(true),
        Head::Pending => return none(false),
    }
    let Some((_, created)) = file_times(path) else {
        return none(true);
    };
    let Some(Ok(entries)) = Path::new(path).parent().map(std::fs::read_dir) else {
        return none(true);
    };
    let mut candidates: Vec<(Timestamp, String)> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "jsonl"))
        .map(|p| p.to_string_lossy().into_owned())
        .filter(|p| p != path)
        .filter_map(|p| file_times(&p).map(|(_, c)| (c, p)))
        .filter(|(c, _)| *c < created)
        .collect();
    candidates.sort_by(|a, b| b.0.total_cmp(&a.0).then_with(|| b.1.cmp(&a.1)));
    Search {
        found: candidates
            .into_iter()
            .map(|c| c.1)
            .find(|p| head_contains(Path::new(p), &marker)),
        settled: true,
    }
}

fn head_of(path: &Path) -> Option<(String, bool)> {
    let file = File::open(path).ok()?;
    let mut head = Vec::new();
    file.take(HEAD_WINDOW as u64).read_to_end(&mut head).ok()?;
    let full = head.len() >= HEAD_WINDOW;
    Some((String::from_utf8_lossy(&head).into_owned(), full))
}

pub fn begins_with_clear(path: &str) -> bool {
    head_of(Path::new(path)).is_some_and(|(head, _)| {
        head.contains("<command-name>/clear</command-name>")
            || head.contains(r#""commandRun":{"command":"clear""#)
            || head.contains("SessionStart:clear")
    })
}

fn head_contains(path: &Path, marker: &str) -> bool {
    head_of(path).is_some_and(|(head, _)| head.contains(marker))
}

fn head_state(path: &Path, marker: &str) -> Head {
    let Some((head, full)) = head_of(path) else {
        return Head::Not;
    };
    if head.contains(marker) && head.contains("SessionStart:clear") {
        Head::Clear
    } else if full || !young(path) {
        Head::Not
    } else {
        Head::Pending
    }
}

fn young(path: &Path) -> bool {
    file_times(&path.to_string_lossy())
        .is_some_and(|(_, created)| now_secs() - created < HEAD_PENDING_FOR)
}

fn search_successor(path: &str, name: &str) -> Search {
    let none = Search {
        found: None,
        settled: true,
    };
    if name.is_empty() {
        return none;
    }
    let Ok(quoted) = serde_json::to_string(name) else {
        return none;
    };
    let marker = format!("\"agentName\":{quoted}");
    let Some(dir) = Path::new(path).parent().map(Path::to_path_buf) else {
        return none;
    };

    let mut current = path.to_string();
    let mut visited = vec![path.to_string()];
    let mut found = None;
    let mut settled = true;
    let mut ended = false;
    for _ in 0..MAX_HOPS {
        let Some(since) = modified_secs(&current).map(|m| m - 10.0) else {
            ended = true;
            break;
        };
        let Ok(entries) = std::fs::read_dir(&dir) else {
            ended = true;
            break;
        };
        let mut candidates: Vec<(Timestamp, String)> = entries
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|x| x == "jsonl"))
            .map(|p| p.to_string_lossy().into_owned())
            .filter(|p| !visited.contains(p))
            .filter_map(|p| file_times(&p).map(|(_, created)| (created, p)))
            .filter(|(created, _)| *created > since)
            .collect();
        candidates.sort_by(|a, b| a.0.total_cmp(&b.0).then_with(|| a.1.cmp(&b.1)));
        let mut next = None;
        for (_, candidate) in candidates {
            match head_state(Path::new(&candidate), &marker) {
                Head::Clear => {
                    next = Some(candidate);
                    break;
                }
                Head::Pending => settled = false,
                Head::Not => {}
            }
        }
        let Some(next) = next else {
            ended = true;
            break;
        };
        visited.push(next.clone());
        found = Some(next.clone());
        current = next;
    }
    Search {
        found: found.filter(|_| ended),
        settled,
    }
}
