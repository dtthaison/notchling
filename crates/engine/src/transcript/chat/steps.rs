//! Tool steps as the phone shows them: a name, one line of input, and for Edit/MultiEdit/Write the
//! changed lines; plus the text helpers the message view caps its output with.

use crate::transcript::text::{between, first_line, prefix_chars};
use crate::transcript::util::str_of;
use serde_json::{Map, Value};

const MAX_SUMMARY: usize = 140;
const MAX_OUTPUT: usize = 3000;
const MAX_DIFF_ROWS: usize = 80;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffRowKind {
    Context,
    Added,
    Removed,
}

/// One line of a diff.
#[derive(Debug, Clone, PartialEq)]
pub struct DiffRow {
    pub kind: DiffRowKind,
    pub text: String,
}

impl DiffRow {
    /// "+added", "-removed", " context": one string per row on the wire.
    pub fn wire(&self) -> String {
        let sign = match self.kind {
            DiffRowKind::Context => ' ',
            DiffRowKind::Added => '+',
            DiffRowKind::Removed => '-',
        };
        format!("{sign}{}", self.text)
    }
}

/// A call as the phone shows it.
#[derive(Debug, Clone, PartialEq)]
pub struct ToolStep {
    pub id: String,
    pub name: String,
    pub summary: String,
    pub output: Option<String>,
    pub path: Option<String>,
    pub diff: Vec<DiffRow>,
}

impl ToolStep {
    fn new(id: &str, name: &str, summary: String) -> ToolStep {
        ToolStep {
            id: id.into(),
            name: name.into(),
            summary,
            output: None,
            path: None,
            diff: Vec::new(),
        }
    }
}

pub fn cap_output(text: &str) -> String {
    let trimmed = text.trim();
    if trimmed.chars().count() > MAX_OUTPUT {
        format!("{}…", prefix_chars(trimmed, MAX_OUTPUT - 1))
    } else {
        trimmed.to_string()
    }
}

/// The text of a tool result: a plain string, or the text blocks of a list.
pub fn result_text(content: Option<&Value>) -> String {
    match content {
        Some(Value::String(s)) => cap_output(s),
        Some(Value::Array(blocks)) => cap_output(
            &blocks
                .iter()
                .filter(|b| str_of(b, "type") == Some("text"))
                .filter_map(|b| str_of(b, "text"))
                .collect::<Vec<_>>()
                .join("\n"),
        ),
        _ => String::new(),
    }
}

/// The text between `<tag>` and `</tag>`, if the text has them.
pub fn tagged(text: &str, tag: &str) -> Option<String> {
    between(text, &format!("<{tag}>"), &format!("</{tag}>"), true)
}

fn short_path(path: &str) -> String {
    let parts: Vec<&str> = path.split('/').filter(|p| !p.is_empty()).collect();
    parts[parts.len().saturating_sub(2)..].join("/")
}

fn summary_line(text: &str) -> String {
    let line = first_line(text);
    if line.chars().count() > MAX_SUMMARY {
        format!("{}…", prefix_chars(line, MAX_SUMMARY - 1))
    } else {
        line.to_string()
    }
}

fn rows(text: &str, kind: DiffRowKind, limit: usize) -> Vec<DiffRow> {
    text.split('\n')
        .take(limit)
        .map(|l| DiffRow {
            kind,
            text: l.to_string(),
        })
        .collect()
}

pub fn step(id: &str, name: &str, input: &Map<String, Value>) -> ToolStep {
    let get = |k: &str| input.get(k).and_then(Value::as_str);
    let file = get("file_path")
        .or_else(|| get("notebook_path"))
        .or_else(|| get("path"));
    let named = |file: Option<&str>| file.map_or_else(|| name.to_string(), short_path);
    match name {
        "Edit" | "MultiEdit" | "NotebookEdit" => {
            let mut diff: Vec<DiffRow> = Vec::new();
            let mut edits: Vec<&Map<String, Value>> = input
                .get("edits")
                .and_then(Value::as_array)
                .map(|a| a.iter().filter_map(Value::as_object).collect())
                .unwrap_or_default();
            if edits.is_empty() {
                edits = vec![input];
            }
            for edit in edits {
                let text = |k: &str| edit.get(k).and_then(Value::as_str);
                let old = text("old_string").unwrap_or("");
                let new = text("new_string")
                    .or_else(|| text("new_source"))
                    .unwrap_or("");
                diff.extend(rows(old, DiffRowKind::Removed, 30));
                diff.extend(rows(new, DiffRowKind::Added, 30));
                if diff.len() >= MAX_DIFF_ROWS {
                    diff.truncate(MAX_DIFF_ROWS);
                    break;
                }
            }
            let mut s = ToolStep::new(id, name, named(file));
            s.path = file.map(String::from);
            s.diff = diff;
            s
        }
        "Write" => {
            let mut s = ToolStep::new(id, name, named(file));
            s.path = file.map(String::from);
            s.diff = rows(get("content").unwrap_or(""), DiffRowKind::Added, 40);
            s
        }
        "Bash" => ToolStep::new(id, name, summary_line(get("command").unwrap_or(""))),
        "Read" => {
            let mut s = ToolStep::new(id, name, named(file));
            s.path = file.map(String::from);
            s
        }
        "Grep" => {
            let pattern = get("pattern").unwrap_or("");
            let in_path = get("path")
                .map(|p| format!(" in {}", short_path(p)))
                .unwrap_or_default();
            ToolStep::new(id, name, summary_line(&format!("{pattern}{in_path}")))
        }
        "Glob" => ToolStep::new(id, name, summary_line(get("pattern").unwrap_or(""))),
        _ => {
            // The first text input, by key order so the summary is the same every read.
            let mut keys: Vec<&String> = input.keys().collect();
            keys.sort();
            let text = keys.iter().find_map(|k| input[*k].as_str()).unwrap_or("");
            ToolStep::new(
                id,
                name,
                if text.is_empty() {
                    name.to_string()
                } else {
                    summary_line(text)
                },
            )
        }
    }
}
