use super::event::{plain_id, Event};
use super::rules::{Action, Rule, Would};
use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

pub const DIR: &str = "spool";
pub const EXT: &str = "ev";
pub const MAX_FILES: usize = 2_000;
pub const MAX_BATCH: usize = 500;
pub const MAX_READ: u64 = 64 * 1024;
pub const STALE_TMP_SECS: u64 = 3_600;
pub const TOOLS: [&str; 4] = ["Bash", "Read", "Agent", "other"];

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Logged {
    pub ts: f64,
    pub session: String,
    pub agent: String,
    pub tool: String,
    pub rule: String,
    pub action: String,
}

pub fn logged(now: f64, event: &Event, would: &Would) -> Logged {
    Logged {
        ts: now,
        session: event.session.clone(),
        agent: event.agent.clone().unwrap_or_default(),
        tool: event.logged_tool().to_string(),
        rule: would.rule.name().to_string(),
        action: would.action.name().to_string(),
    }
}

fn valid(row: &Logged) -> bool {
    row.ts.is_finite()
        && plain_id(&row.session)
        && (row.agent.is_empty() || plain_id(&row.agent))
        && TOOLS.contains(&row.tool.as_str())
        && Rule::parse(&row.rule).is_some()
        && Action::parse(&row.action).is_some()
}

#[cfg(unix)]
fn create_new(path: &Path) -> std::io::Result<std::fs::File> {
    use std::os::unix::fs::OpenOptionsExt;
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)
}

#[cfg(not(unix))]
fn create_new(path: &Path) -> std::io::Result<std::fs::File> {
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
}

fn unique_name() -> String {
    static N: AtomicU64 = AtomicU64::new(0);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!(
        "{nanos:024}-{}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::Relaxed)
    )
}

pub fn append(dir: &Path, now: f64, event: &Event, woulds: &[Would]) -> bool {
    if woulds.is_empty() {
        return true;
    }
    let mut text = String::new();
    for would in woulds {
        if let Ok(line) = serde_json::to_string(&logged(now, event, would)) {
            text.push_str(&line);
            text.push('\n');
        }
    }
    let folder = dir.join(DIR);
    if std::fs::create_dir_all(&folder).is_err() {
        return false;
    }
    let backlog = std::fs::read_dir(&folder)
        .map(|entries| entries.take(MAX_FILES).count())
        .unwrap_or(0);
    if backlog >= MAX_FILES {
        return false;
    }
    let name = unique_name();
    let tmp = folder.join(format!("{name}.tmp"));
    let done = folder.join(format!("{name}.{EXT}"));
    let written = create_new(&tmp)
        .and_then(|mut file| file.write_all(text.as_bytes()))
        .and_then(|_| std::fs::rename(&tmp, &done))
        .is_ok();
    if !written {
        let _ = std::fs::remove_file(&tmp);
    }
    written
}

#[derive(Debug)]
pub struct Batch {
    pub id: String,
    pub rows: Vec<Logged>,
    file: PathBuf,
}

#[derive(Debug, Default)]
pub struct Taken {
    pub batches: Vec<Batch>,
}

impl Taken {
    pub fn rows(&self) -> usize {
        self.batches.iter().map(|b| b.rows.len()).sum()
    }

    pub fn done(self) {
        for batch in self.batches {
            let _ = std::fs::remove_file(batch.file);
        }
    }
}

fn read_rows(path: &Path) -> Vec<Logged> {
    use std::io::Read;
    let Ok(file) = std::fs::File::open(path) else {
        return Vec::new();
    };
    let mut bytes = Vec::new();
    if file.take(MAX_READ).read_to_end(&mut bytes).is_err() {
        return Vec::new();
    }
    String::from_utf8_lossy(&bytes)
        .lines()
        .filter_map(|l| serde_json::from_str::<Logged>(l).ok())
        .filter(valid)
        .collect()
}

fn sweep_tmp(folder: &Path) {
    let Ok(entries) = std::fs::read_dir(folder) else {
        return;
    };
    let limit = std::time::Duration::from_secs(STALE_TMP_SECS);
    for entry in entries.flatten() {
        let path = entry.path();
        let stale = path.extension().is_some_and(|e| e == "tmp")
            && entry
                .metadata()
                .and_then(|m| m.modified())
                .ok()
                .and_then(|t| t.elapsed().ok())
                .is_some_and(|age| age > limit);
        if stale {
            let _ = std::fs::remove_file(path);
        }
    }
}

pub fn take(dir: &Path) -> Option<Taken> {
    let folder = dir.join(DIR);
    let entries = std::fs::read_dir(&folder).ok()?;
    let mut names: Vec<String> = entries
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x == EXT))
        .filter_map(|e| e.file_name().into_string().ok())
        .collect();
    if names.is_empty() {
        sweep_tmp(&folder);
        return None;
    }
    names.sort();
    names.truncate(MAX_BATCH);
    let batches = names
        .into_iter()
        .map(|name| {
            let file = folder.join(&name);
            Batch {
                rows: read_rows(&file),
                id: name,
                file,
            }
        })
        .collect();
    Some(Taken { batches })
}
