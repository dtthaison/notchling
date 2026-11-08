//! Codex tool calls as Chat steps: shell commands, patches, anything else by name.

use crate::transcript::model::*;
use crate::transcript::text::last_path_component;
use serde_json::{Map, Value};

/// A Codex tool call as a Chat step.
pub fn call(id: &str, name: &str, raw: &str, action: Option<&Value>) -> ToolCall {
    let args: Option<Map<String, Value>> =
        serde_json::from_str::<Value>(raw)
            .ok()
            .and_then(|v| match v {
                Value::Object(m) => Some(m),
                _ => None,
            });
    let arg_str = |key: &str| {
        args.as_ref()
            .and_then(|a| a.get(key))
            .and_then(Value::as_str)
    };
    match name {
        "shell" | "exec_command" | "local_shell" | "container.exec" | "shell_command" => {
            let mut command = arg_str("cmd")
                .or_else(|| arg_str("command"))
                .map(String::from);
            if command.is_none() {
                let value = args
                    .as_ref()
                    .and_then(|a| a.get("command"))
                    .or_else(|| action.and_then(|a| a.get("command")));
                let parts: Option<Vec<&str>> = value
                    .and_then(Value::as_array)
                    .and_then(|a| a.iter().map(Value::as_str).collect());
                if let Some(parts) = parts {
                    // ["bash", "-lc", "the command"]: the script is what matters.
                    command = Some(if parts.len() == 3 && parts[1] == "-lc" {
                        parts[2].to_string()
                    } else {
                        parts.join(" ")
                    });
                }
            }
            let text = command.unwrap_or_else(|| raw.to_string());
            let title = text
                .split(['\n', '\r'])
                .find(|l| !l.is_empty())
                .unwrap_or("Command")
                .to_string();
            ToolCall::new(id, "Bash", ToolKind::Shell, title, text)
        }
        "apply_patch" => {
            let patch = arg_str("input").unwrap_or(raw);
            let files = patch_files(patch);
            let title = if files.is_empty() {
                "Edit".to_string()
            } else {
                format!(
                    "Edit {}",
                    files
                        .iter()
                        .map(|f| last_path_component(f))
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            };
            let mut call = ToolCall::new(id, "Edit", ToolKind::Edit, title, files.join("\n"));
            call.diff = diff(patch);
            call
        }
        "view_image" => ToolCall::new(
            id,
            "Read",
            ToolKind::Read,
            format!("View {}", arg_str("path").unwrap_or("image")),
            raw.to_string(),
        ),
        "web_search" => ToolCall::new(
            id,
            "WebSearch",
            ToolKind::Web,
            arg_str("query").unwrap_or("Web search").to_string(),
            raw.to_string(),
        ),
        "update_plan" => ToolCall::new(
            id,
            "TodoWrite",
            ToolKind::Task,
            "Update plan".into(),
            raw.to_string(),
        ),
        _ => ToolCall::new(id, name, ToolKind::Other, name.to_string(), raw.to_string()),
    }
}

/// The files an `apply_patch` touches (`*** Update File: path`, Add, Delete).
pub fn patch_files(patch: &str) -> Vec<String> {
    patch
        .split(['\n', '\r'])
        .filter_map(|line| {
            ["*** Update File: ", "*** Add File: ", "*** Delete File: "]
                .iter()
                .find_map(|prefix| line.strip_prefix(prefix))
                .map(|rest| rest.trim().to_string())
        })
        .collect()
}

pub fn diff(patch: &str) -> Vec<DiffLine> {
    patch
        .split('\n')
        .filter_map(|line| {
            if line.starts_with("***") {
                return None;
            }
            let rest = |n: usize| line[n..].to_string();
            if line.starts_with("@@") {
                Some(DiffLine {
                    kind: DiffKind::Hunk,
                    text: line.to_string(),
                    number: None,
                })
            } else if line.starts_with('+') {
                Some(DiffLine {
                    kind: DiffKind::Add,
                    text: rest(1),
                    number: None,
                })
            } else if line.starts_with('-') {
                Some(DiffLine {
                    kind: DiffKind::Remove,
                    text: rest(1),
                    number: None,
                })
            } else if line.starts_with(' ') {
                Some(DiffLine {
                    kind: DiffKind::Context,
                    text: rest(1),
                    number: None,
                })
            } else {
                None
            }
        })
        .collect()
}

/// A tool output as text. Exec output can be JSON `{"output": "...", "metadata": {...}}`.
pub fn output_text(value: &Value) -> String {
    match value {
        Value::String(s) => {
            if let Ok(Value::Object(obj)) = serde_json::from_str::<Value>(s) {
                if let Some(out) = obj.get("output").and_then(Value::as_str) {
                    return out.to_string();
                }
            }
            s.clone()
        }
        Value::Array(parts) => parts
            .iter()
            .filter_map(|p| p.get("text").and_then(Value::as_str))
            .collect::<Vec<_>>()
            .join("\n"),
        Value::Object(obj) => obj
            .get("content")
            .and_then(Value::as_str)
            .or_else(|| obj.get("output").and_then(Value::as_str))
            .unwrap_or("")
            .to_string(),
        _ => String::new(),
    }
}

/// The exit code a shell output reports ("Process exited with code 1", "exit code: 2"), if any.
pub fn exit_code(output: &str) -> Option<u64> {
    for (i, _) in output.char_indices() {
        let rest = &output[i..];
        let after = if let Some(r) = rest.strip_prefix("Process exited with code") {
            r
        } else if let Some(r) = rest.strip_prefix("exit code") {
            r.strip_prefix(':').unwrap_or(r)
        } else {
            continue;
        };
        let digits: String = after
            .trim_start()
            .chars()
            .take_while(char::is_ascii_digit)
            .collect();
        if !digits.is_empty() {
            return digits.parse().ok();
        }
    }
    None
}
