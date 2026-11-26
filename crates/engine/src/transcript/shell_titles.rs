use super::conversation::MAX_LINE;
use super::finished::WINDOW;
use super::glance::has;
use super::scan::{scan_lines, Line};
use super::shells;
use super::tail::RAW_BYTES;
use super::util::str_of;
use serde_json::Value;
use std::collections::{HashMap, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::time::UNIX_EPOCH;

const MAX_FILES: usize = 64;

const MAX_CALLS: usize = 64;

#[derive(Default)]
struct Scan {
    offset: u64,
    skip_first: bool,
    calls: VecDeque<(String, String)>,
    found: HashMap<String, String>,
}

#[derive(Default)]
pub struct ShellTitles {
    files: HashMap<PathBuf, Scan>,
}

impl ShellTitles {
    pub fn read(&mut self, dir: &Path, wanted: &[String], since_s: f64) -> HashMap<String, String> {
        let mut found = HashMap::new();
        if wanted.is_empty() {
            return found;
        }
        let mut files: Vec<(PathBuf, f64)> = fs::read_dir(dir)
            .into_iter()
            .flatten()
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|x| x == "jsonl"))
            .filter_map(|p| {
                let modified = fs::metadata(&p).ok()?.modified().ok()?;
                let secs = modified.duration_since(UNIX_EPOCH).ok()?.as_secs_f64();
                (secs >= since_s).then_some((p, secs))
            })
            .collect();
        files.sort_by(|a, b| b.1.total_cmp(&a.1));
        files.truncate(MAX_FILES);
        let live: Vec<&PathBuf> = files.iter().map(|(p, _)| p).collect();
        self.files.retain(|path, _| live.contains(&path));
        for (path, _) in &files {
            let scan = self.files.entry(path.clone()).or_default();
            scan.advance(path);
            for id in wanted {
                if let Some(description) = scan.found.get(id) {
                    found.insert(id.clone(), description.clone());
                }
            }
        }
        found
    }
}

impl Scan {
    fn advance(&mut self, path: &Path) {
        let Some(path) = path.to_str() else {
            return;
        };
        let Ok(size) = std::fs::metadata(path).map(|m| m.len()) else {
            return;
        };
        if size < self.offset {
            *self = Scan::default();
        }
        if size == self.offset {
            return;
        }
        let start = self.offset.max(size.saturating_sub(WINDOW));
        let skip_first = self.skip_first || start > self.offset;
        let mut lines = Vec::new();
        let Ok(scanned) = scan_lines(path, start, size, skip_first, MAX_LINE, |line| {
            if let Line::Whole(line) = line {
                if Self::wanted(line) {
                    lines.push(line.to_vec());
                }
            }
        }) else {
            return;
        };
        RAW_BYTES.fetch_add(scanned.span, Ordering::Relaxed);
        let consumed = start + scanned.span;
        if size - consumed > MAX_LINE as u64 {
            self.offset = size;
            self.skip_first = true;
        } else {
            self.offset = consumed;
            self.skip_first = skip_first && scanned.span == 0;
        }
        for line in lines {
            self.line(&line);
        }
    }

    fn wanted(line: &[u8]) -> bool {
        let call = has(line, b"\"tool_use\"") && has(line, b"\"description\"");
        let result = shells::launch_in(line) && has(line, b"\"tool_result\"");
        call || result
    }

    fn line(&mut self, line: &[u8]) {
        let Ok(value) = serde_json::from_slice::<Value>(line) else {
            return;
        };
        let Some(blocks) = value
            .get("message")
            .and_then(|m| m.get("content"))
            .and_then(Value::as_array)
        else {
            return;
        };
        for block in blocks {
            match str_of(block, "type") {
                Some("tool_use") if str_of(block, "name") == Some("Bash") => {
                    let id = str_of(block, "id").unwrap_or("");
                    let description = block
                        .get("input")
                        .and_then(|i| str_of(i, "description"))
                        .unwrap_or("");
                    if !id.is_empty() && !description.is_empty() {
                        if self.calls.len() == MAX_CALLS {
                            self.calls.pop_front();
                        }
                        self.calls
                            .push_back((id.to_string(), description.to_string()));
                    }
                }
                Some("tool_result") => {
                    let Some(id) = str_of(block, "tool_use_id") else {
                        continue;
                    };
                    let Some(at) = self.calls.iter().position(|(call, _)| call == id) else {
                        continue;
                    };
                    let Some((_, description)) = self.calls.remove(at) else {
                        continue;
                    };
                    let text = super::conversation::result_text(block.get("content"));
                    if let Some(launch) = shells::launch(&text) {
                        self.found.insert(launch.id, description);
                    }
                }
                _ => {}
            }
        }
    }
}
