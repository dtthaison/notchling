//! Counts tool calls the way the terminal collapses them: "Read 2 files, searched for 1 pattern".

use super::text::{last_path_component, prefix_chars};
use serde_json::{Map, Value};
use std::collections::{HashMap, HashSet};

#[derive(Debug, Default, Clone)]
pub struct ToolTally {
    order: Vec<String>,
    counts: HashMap<String, usize>,
    files: HashMap<String, HashSet<String>>,
}

/// The last word of an `mcp__server__tool` name.
fn short_tool(tool: &str) -> String {
    tool.rsplit('_')
        .find(|p| !p.is_empty())
        .unwrap_or("")
        .to_string()
}

/// The terminal counts a shell command that only searches or reads as a search or a read.
fn bash_kind(command: &str) -> &'static str {
    let words: Vec<&str> = command
        .split(['&', '|', ';', '\n'])
        .filter_map(|part| part.split(' ').find(|w| !w.is_empty()))
        .filter(|w| *w != "cd")
        .collect();
    if !words.is_empty()
        && words
            .iter()
            .all(|w| ["grep", "rg", "find", "egrep"].contains(w))
    {
        return "search";
    }
    if !words.is_empty()
        && words
            .iter()
            .all(|w| ["cat", "head", "tail", "sed", "less", "wc"].contains(w))
    {
        return "read";
    }
    "bash"
}

impl ToolTally {
    pub fn is_empty(&self) -> bool {
        self.order.is_empty()
    }

    pub fn add(&mut self, tool: &str, input: &Map<String, Value>) {
        let key = match tool {
            "Read" => "read".to_string(),
            "Edit" | "MultiEdit" | "NotebookEdit" => "edit".to_string(),
            "Write" => "write".to_string(),
            "Grep" | "Glob" => "search".to_string(),
            "Bash" => {
                bash_kind(input.get("command").and_then(Value::as_str).unwrap_or("")).to_string()
            }
            "WebSearch" | "WebFetch" => "web".to_string(),
            "Agent" | "Task" => "agent".to_string(),
            "AskUserQuestion" => "ask".to_string(),
            "TodoWrite" | "TaskCreate" | "TaskUpdate" | "TaskList" | "TaskGet" => {
                "tasks".to_string()
            }
            other => {
                let short = if other.starts_with("mcp__") {
                    short_tool(other)
                } else {
                    other.to_string()
                };
                format!("use:{short}")
            }
        };
        if !self.counts.contains_key(&key) {
            self.order.push(key.clone());
        }
        *self.counts.entry(key.clone()).or_insert(0) += 1;
        let path = input
            .get("file_path")
            .and_then(Value::as_str)
            .or_else(|| input.get("notebook_path").and_then(Value::as_str));
        if let Some(path) = path {
            self.files.entry(key).or_default().insert(path.to_string());
        }
    }

    pub fn summary(&self) -> Option<String> {
        if self.order.is_empty() {
            return None;
        }
        let count = |key: &str, one: &str, many: &str| {
            let total = self
                .files
                .get(key)
                .map(HashSet::len)
                .unwrap_or_else(|| self.counts.get(key).copied().unwrap_or(0));
            format!("{total} {}", if total == 1 { one } else { many })
        };
        let parts: Vec<String> = self
            .order
            .iter()
            .map(|key| match key.as_str() {
                "read" => format!("read {}", count(key, "file", "files")),
                "edit" => format!("edited {}", count(key, "file", "files")),
                "write" => format!("wrote {}", count(key, "file", "files")),
                "search" => format!("searched for {}", count(key, "pattern", "patterns")),
                "bash" => format!("ran {}", count(key, "shell command", "shell commands")),
                "web" => "searched the web".to_string(),
                "agent" => format!("ran {}", count(key, "agent", "agents")),
                "ask" => "asked you".to_string(),
                "tasks" => "updated tasks".to_string(),
                other => format!("used {}", other.strip_prefix("use:").unwrap_or(other)),
            })
            .collect();
        let text = parts.join(", ");
        let mut chars = text.chars();
        Some(match chars.next() {
            Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
            None => text,
        })
    }
}

/// A short "what it's doing now" line, like "Editing main.swift".
pub fn describe(tool: &str, input: &Map<String, Value>) -> String {
    let file = |key: &str| {
        last_path_component(input.get(key).and_then(Value::as_str).unwrap_or("")).to_string()
    };
    let s = |key: &str| {
        input
            .get(key)
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string()
    };
    match tool {
        "Edit" | "MultiEdit" => format!("Editing {}", file("file_path")),
        "Write" => format!("Writing {}", file("file_path")),
        "Read" => format!("Reading {}", file("file_path")),
        "NotebookEdit" => format!("Editing {}", file("notebook_path")),
        "Bash" => {
            let d = s("description");
            if d.is_empty() {
                format!("Running {}", prefix_chars(&s("command"), 60))
            } else {
                d
            }
        }
        "Grep" | "Glob" => format!("Searching {}", s("pattern")),
        "WebFetch" | "WebSearch" => "Searching the web".to_string(),
        "Agent" | "Task" => {
            let d = input
                .get("description")
                .and_then(Value::as_str)
                .unwrap_or("a subagent");
            format!("Running {d}")
        }
        "AskUserQuestion" => "Asking you a question".to_string(),
        "TodoWrite" | "TaskCreate" | "TaskUpdate" => "Updating its task list".to_string(),
        other => {
            let short = if other.starts_with("mcp__") {
                short_tool(other)
            } else {
                other.to_string()
            };
            format!("Using {short}")
        }
    }
}
