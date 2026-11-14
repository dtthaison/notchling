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

pub fn find_transcript(projects_dir: &str, session_id: &str, cwd: &str) -> Option<String> {
    let file = format!("{session_id}.jsonl");
    let mut cwds = vec![cwd.to_string()];
    if let Some(rest) = cwd.strip_prefix("/private") {
        cwds.push(rest.to_string());
    } else if ["/tmp", "/var", "/etc"]
        .iter()
        .any(|p| cwd == *p || cwd.starts_with(&format!("{p}/")))
    {
        cwds.push(format!("/private{cwd}"));
    }
    for c in cwds {
        let guess = Path::new(projects_dir).join(project_folder(&c)).join(&file);
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

pub fn job_transcript(
    link_scan_path: Option<&str>,
    session_id: Option<&str>,
    worktree_path: Option<&str>,
    modified: impl Fn(&str) -> Option<Timestamp>,
) -> Option<String> {
    let link = link_scan_path?;
    let mut candidates = vec![link.to_string()];
    if let (Some(session), Some(worktree)) = (
        session_id.filter(|s| !s.is_empty()),
        worktree_path.filter(|w| !w.is_empty()),
    ) {
        let projects = Path::new(link)
            .parent()
            .and_then(Path::parent)
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default();
        candidates.push(format!(
            "{projects}/{}/{session}.jsonl",
            project_folder(worktree)
        ));
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
    let mut found = None;
    let mut settled = true;
    for _ in 0..4 {
        let Some(since) = modified_secs(&current).map(|m| m - 10.0) else {
            break;
        };
        let Ok(entries) = std::fs::read_dir(&dir) else {
            break;
        };
        let mut candidates: Vec<(Timestamp, String)> = entries
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|x| x == "jsonl"))
            .map(|p| p.to_string_lossy().into_owned())
            .filter(|p| *p != current && p != path)
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
        let Some(next) = next else { break };
        found = Some(next.clone());
        current = next;
    }
    Search { found, settled }
}
