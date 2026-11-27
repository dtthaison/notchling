use crate::chat::file_panel::open_regular;
use serde_json::Value;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

pub const PAGE_LINES: usize = 200;
pub const PAGE_BYTES: u64 = 2 * 1024 * 1024;
pub const MAX_LINE: usize = 256 * 1024;
pub const SCAN_CAP: u64 = 8 * 1024 * 1024;
const PREVIEW_CHARS: usize = 120;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Summary {
    pub tag: String,
    pub preview: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    pub number: usize,
    pub raw: String,
    pub cut: bool,
    pub summary: Summary,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Jsonl {
    pub rows: Vec<Row>,
    pub start: u64,
}

impl Jsonl {
    pub fn more(&self) -> bool {
        self.start > 0
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Page {
    pub start: u64,
    pub lines: Vec<String>,
    pub capped: bool,
}

fn flat(text: &str) -> String {
    let joined = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if joined.chars().count() > PREVIEW_CHARS {
        let cut: String = joined.chars().take(PREVIEW_CHARS).collect();
        format!("{cut}…")
    } else {
        joined
    }
}

fn text_of(value: &Value) -> Option<String> {
    match value {
        Value::String(s) if !s.trim().is_empty() => Some(s.clone()),
        Value::Array(items) => items.iter().find_map(text_of),
        Value::Object(map) => ["text", "content", "thinking", "name", "command"]
            .iter()
            .find_map(|k| map.get(*k).and_then(text_of)),
        _ => None,
    }
}

fn tag_of(value: &Value) -> String {
    let word = |v: Option<&Value>| v.and_then(Value::as_str).map(str::to_owned);
    word(value.get("type"))
        .or_else(|| word(value.get("role")))
        .or_else(|| word(value.pointer("/message/role")))
        .or_else(|| word(value.get("event")))
        .or_else(|| word(value.get("kind")))
        .unwrap_or_default()
}

pub fn summarize(line: &str) -> Summary {
    let trimmed = line.trim();
    let Ok(value) = serde_json::from_str::<Value>(trimmed) else {
        return Summary {
            tag: "invalid".to_owned(),
            preview: flat(trimmed),
        };
    };
    let preview = value
        .as_object()
        .and_then(|map| {
            [
                "message", "content", "text", "summary", "msg", "prompt", "name", "command",
            ]
            .iter()
            .find_map(|k| map.get(*k).and_then(text_of))
        })
        .map(|t| flat(&t))
        .unwrap_or_else(|| flat(trimmed));
    Summary {
        tag: tag_of(&value),
        preview,
    }
}

fn long_line(file: &mut std::fs::File, before: u64, end: u64) -> Option<Page> {
    let mut start = 0;
    let mut at = before;
    let floor = before.saturating_sub(SCAN_CAP);
    let mut capped = floor > 0;
    let mut chunk = vec![0u8; 64 * 1024];
    while at > floor {
        let from = at.saturating_sub(chunk.len() as u64).max(floor);
        file.seek(SeekFrom::Start(from)).ok()?;
        let got = &mut chunk[..(at - from) as usize];
        file.read_exact(got).ok()?;
        if let Some(i) = got.iter().rposition(|b| *b == b'\n') {
            start = from + i as u64 + 1;
            capped = false;
            break;
        }
        at = from;
    }
    if capped {
        start = floor;
    }
    file.seek(SeekFrom::Start(start)).ok()?;
    let mut line = Vec::new();
    file.take((end - start).min(MAX_LINE as u64 + 1))
        .read_to_end(&mut line)
        .ok()?;
    Some(Page {
        start,
        lines: vec![String::from_utf8_lossy(&line).into_owned()],
        capped,
    })
}

pub fn tail(path: &Path, end: u64, max_lines: usize, max_bytes: u64) -> Option<Page> {
    let mut file = open_regular(path)?;
    let end = end.min(file.metadata().ok()?.len());
    let begin = end.saturating_sub(max_bytes);
    let from = begin.saturating_sub(1);
    file.seek(SeekFrom::Start(from)).ok()?;
    let mut buf = Vec::with_capacity((end - from) as usize);
    (&mut file).take(end - from).read_to_end(&mut buf).ok()?;
    let mut content: &[u8] = &buf;
    if content.last() == Some(&b'\n') {
        content = &content[..content.len() - 1];
    }
    let mut start = from;
    if begin > 0 {
        if buf.first() == Some(&b'\n') {
            content = &content[1.min(content.len())..];
            start = from + 1;
        } else {
            match content.iter().position(|b| *b == b'\n') {
                Some(i) => {
                    content = &content[i + 1..];
                    start = from + i as u64 + 1;
                }
                None => return long_line(&mut file, from, end),
            }
        }
    }
    let mut lines: Vec<&[u8]> = content.split(|b| *b == b'\n').collect();
    if content.is_empty() {
        lines.clear();
    }
    if lines.len() > max_lines {
        let drop = lines.len() - max_lines;
        start += lines[..drop]
            .iter()
            .map(|l| l.len() as u64 + 1)
            .sum::<u64>();
        lines.drain(..drop);
    }
    Some(Page {
        start,
        lines: lines
            .into_iter()
            .map(|l| String::from_utf8_lossy(l.strip_suffix(b"\r").unwrap_or(l)).into_owned())
            .collect(),
        capped: false,
    })
}

pub fn lines_before(path: &Path, offset: u64) -> usize {
    let Some(file) = open_regular(path) else {
        return 0;
    };
    let mut reader = file.take(offset);
    let mut chunk = vec![0u8; 64 * 1024];
    let mut count = 0;
    loop {
        match reader.read(&mut chunk) {
            Ok(0) | Err(_) => return count,
            Ok(n) => count += chunk[..n].iter().filter(|b| **b == b'\n').count(),
        }
    }
}

fn prefix_tag(line: &str) -> String {
    ["type", "role", "event", "kind"]
        .iter()
        .find_map(|key| {
            let needle = format!("\"{key}\":\"");
            let mut end = line.len().min(4096);
            while !line.is_char_boundary(end) {
                end -= 1;
            }
            let head = &line[..end];
            let from = head.find(&needle)? + needle.len();
            let word: String = head[from..].chars().take_while(|c| *c != '"').collect();
            (!word.is_empty()).then_some(word)
        })
        .unwrap_or_default()
}

fn rows_of(page: &Page, first: usize) -> Vec<Row> {
    page.lines
        .iter()
        .enumerate()
        .filter(|(_, line)| !line.trim().is_empty())
        .map(|(i, line)| {
            let cut = line.len() > MAX_LINE;
            let summary = match summarize(line) {
                Summary { tag, preview } if cut && tag == "invalid" => Summary {
                    tag: prefix_tag(line),
                    preview,
                },
                other => other,
            };
            let raw = if cut {
                let mut end = MAX_LINE;
                while !line.is_char_boundary(end) {
                    end -= 1;
                }
                line[..end].to_owned()
            } else {
                line.clone()
            };
            Row {
                number: first + i,
                raw,
                cut,
                summary,
            }
        })
        .collect()
}

fn older(path: &Path, mut rows: Vec<Row>, mut start: u64) -> Option<Jsonl> {
    loop {
        let page = tail(path, start, PAGE_LINES, PAGE_BYTES)?;
        let first = if page.capped {
            0
        } else {
            lines_before(path, page.start) + 1
        };
        let mut added = rows_of(&page, first);
        let stepped = !added.is_empty();
        added.extend(rows);
        rows = added;
        let moved = page.start < start;
        start = if page.capped { 0 } else { page.start };
        if stepped || start == 0 || !moved {
            return Some(Jsonl { rows, start });
        }
    }
}

pub fn open(path: &Path) -> Option<Jsonl> {
    let len = open_regular(path)?.metadata().ok()?.len();
    older(path, Vec::new(), len)
}

pub fn earlier(path: &Path, shown: &Jsonl) -> Option<Jsonl> {
    older(path, shown.rows.clone(), shown.start)
}
