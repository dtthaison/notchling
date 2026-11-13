use crate::transcript::model::*;
use crate::transcript::text::{last_path_component, prefix_chars};
use crate::transcript::util::{int_of, str_of};
use serde_json::{Map, Value};

fn str_in(input: &Map<String, Value>, key: &str) -> String {
    input
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string()
}

fn file_in(input: &Map<String, Value>, key: &str) -> String {
    last_path_component(&str_in(input, key)).to_string()
}

fn url_host(url: &str) -> Option<String> {
    let rest = url.split_once("://")?.1;
    let authority = rest.split(['/', '?', '#']).next()?;
    let host_port = authority.rsplit('@').next()?;
    let host = host_port.split(':').next()?;
    if host.is_empty() || url.contains(' ') {
        None
    } else {
        Some(host.to_string())
    }
}

pub fn make_call(id: &str, name: &str, input: &Map<String, Value>) -> ToolCall {
    let s = |k: &str| str_in(input, k);
    let call =
        |kind: ToolKind, title: String, text: String| ToolCall::new(id, name, kind, title, text);
    match name {
        "Bash" => {
            let command = s("command");
            let d = s("description");
            let title = if d.is_empty() {
                format!("Ran {}", prefix_chars(&command, 60))
            } else {
                d
            };
            call(ToolKind::Shell, title, command)
        }
        "Read" => call(
            ToolKind::Read,
            format!("Read {}", file_in(input, "file_path")),
            s("file_path"),
        ),
        "Edit" | "MultiEdit" => call(
            ToolKind::Edit,
            format!("Edited {}", file_in(input, "file_path")),
            s("file_path"),
        ),
        "NotebookEdit" => call(
            ToolKind::Edit,
            format!("Edited {}", file_in(input, "notebook_path")),
            s("notebook_path"),
        ),
        "Write" => call(
            ToolKind::Write,
            format!("Wrote {}", file_in(input, "file_path")),
            s("file_path"),
        ),
        "Grep" => {
            let text = [s("pattern"), s("path")]
                .into_iter()
                .filter(|p| !p.is_empty())
                .collect::<Vec<_>>()
                .join(" in ");
            call(
                ToolKind::Search,
                format!("Searched for {}", s("pattern")),
                text,
            )
        }
        "Glob" => call(
            ToolKind::Search,
            format!("Found files matching {}", s("pattern")),
            s("pattern"),
        ),
        "WebSearch" => call(
            ToolKind::Web,
            format!("Searched the web for \u{201c}{}\u{201d}", s("query")),
            s("query"),
        ),
        "WebFetch" => {
            let url = s("url");
            let host = url_host(&url).unwrap_or_else(|| url.clone());
            call(ToolKind::Web, format!("Read {host}"), url)
        }
        "Agent" | "Task" => {
            let d = s("description");
            call(
                ToolKind::Agent,
                format!(
                    "Agent: {}",
                    if d.is_empty() {
                        "subagent".to_string()
                    } else {
                        d
                    }
                ),
                s("prompt"),
            )
        }
        "Skill" => call(
            ToolKind::Skill,
            format!("Used the {} skill", s("skill")),
            s("args"),
        ),
        "TaskCreate" => call(
            ToolKind::Task,
            format!("Added a task: {}", s("subject")),
            s("description"),
        ),
        "TaskUpdate" => {
            let status = s("status");
            let title = if status.is_empty() {
                format!("Updated task #{}", s("taskId"))
            } else {
                format!("Task #{} {}", s("taskId"), status.replace('_', " "))
            };
            call(ToolKind::Task, title, String::new())
        }
        "TodoWrite" | "TaskList" | "TaskGet" => call(
            ToolKind::Task,
            "Updated the task list".into(),
            String::new(),
        ),
        "AskUserQuestion" => {
            let q = input
                .get("questions")
                .and_then(Value::as_array)
                .and_then(|a| a.first())
                .and_then(|q| str_of(q, "question"))
                .unwrap_or("");
            call(ToolKind::Ask, format!("Asked you: {q}"), String::new())
        }
        _ => {
            let mut title = format!("Used {name}");
            if name.starts_with("mcp__") {
                let parts: Vec<&str> = name.split('_').filter(|p| !p.is_empty()).collect();
                title = format!("Used {}", parts.last().copied().unwrap_or(""));
                if parts.len() > 2 {
                    title += &format!(" ({})", parts[1]);
                }
            } else if let Some(action) = input.get("action").and_then(Value::as_str) {
                title = format!("{name}: {action}");
                if let Some(path) = input.get("file_path") {
                    title += &format!(" {}", last_path_component(path.as_str().unwrap_or("")));
                }
            }
            call(ToolKind::Other, title, pretty(input))
        }
    }
}

pub fn result_text(content: Option<&Value>) -> String {
    match content {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Array(blocks)) => blocks
            .iter()
            .filter_map(|b| match str_of(b, "type") {
                Some("text") => str_of(b, "text").map(String::from),
                Some("image") => Some("[Image]".to_string()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

pub fn clip(text: &str, result: Option<&Value>) -> Option<String> {
    let mut out = text.to_string();
    if let Some(stdout) = result.and_then(|r| str_of(r, "stdout")) {
        let stderr = result.and_then(|r| str_of(r, "stderr")).unwrap_or("");
        out = [stdout, stderr]
            .into_iter()
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join("\n");
    }
    let mut out = out.trim().to_string();
    if out.is_empty() {
        return None;
    }
    let lines: Vec<&str> = out.split('\n').collect();
    if lines.len() > 80 {
        out = format!(
            "{}\n… {} more lines",
            lines[..80].join("\n"),
            lines.len() - 80
        );
    }
    if out.chars().count() > 8000 {
        out = format!("{}\n…", prefix_chars(&out, 8000));
    }
    Some(out)
}

pub fn diff_lines(patch: &[Value]) -> Vec<DiffLine> {
    let mut out = Vec::new();
    for hunk in patch.iter().take(12) {
        let mut old = hunk.get("oldStart").and_then(int_of).unwrap_or(0);
        let mut new = hunk.get("newStart").and_then(int_of).unwrap_or(0);
        out.push(DiffLine {
            kind: DiffKind::Hunk,
            text: format!("@@ line {new}"),
            number: None,
        });
        let lines = hunk
            .get("lines")
            .and_then(Value::as_array)
            .map(Vec::as_slice)
            .unwrap_or(&[]);
        for line in lines.iter().take(120).filter_map(Value::as_str) {
            let mut chars = line.chars();
            let first = chars.next();
            let body = chars.as_str().to_string();
            match first {
                Some('+') => {
                    out.push(DiffLine {
                        kind: DiffKind::Add,
                        text: body,
                        number: Some(new),
                    });
                    new += 1;
                }
                Some('-') => {
                    out.push(DiffLine {
                        kind: DiffKind::Remove,
                        text: body,
                        number: Some(old),
                    });
                    old += 1;
                }
                _ => {
                    out.push(DiffLine {
                        kind: DiffKind::Context,
                        text: body,
                        number: Some(new),
                    });
                    old += 1;
                    new += 1;
                }
            }
        }
    }
    out
}

pub fn written_lines(content: &str) -> Vec<DiffLine> {
    content
        .split('\n')
        .take(60)
        .enumerate()
        .map(|(i, l)| DiffLine {
            kind: DiffKind::Add,
            text: l.to_string(),
            number: Some(i as i64 + 1),
        })
        .collect()
}

pub fn apply_task_result(name: &str, result: Option<&Value>, c: &mut Conversation) {
    let Some(result) = result else { return };
    if name == "TaskCreate" {
        if let Some(task) = result.get("task") {
            if let Some(id) = str_of(task, "id") {
                let subject = str_of(task, "subject").unwrap_or("").to_string();
                match c.tasks.iter_mut().find(|t| t.id == id) {
                    Some(existing) => existing.subject = subject,
                    None => c.tasks.push(TaskEntry {
                        id: id.to_string(),
                        subject,
                        status: TaskStatus::Pending,
                    }),
                }
            }
        }
    } else if name == "TaskUpdate" {
        let (Some(id), Some(to)) = (
            str_of(result, "taskId"),
            result.get("statusChange").and_then(|c| str_of(c, "to")),
        ) else {
            return;
        };
        if to == "deleted" {
            c.tasks.retain(|t| t.id != id);
        } else if let (Some(task), Some(status)) = (
            c.tasks.iter_mut().find(|t| t.id == id),
            TaskStatus::parse(to),
        ) {
            task.status = status;
        }
    }
}

pub fn pretty(input: &Map<String, Value>) -> String {
    let mut out = String::new();
    write_value(&Value::Object(input.clone()), 0, &mut out);
    if out.chars().count() > 4000 {
        format!("{}\n…", prefix_chars(&out, 4000))
    } else {
        out
    }
}

fn write_string(s: &str, out: &mut String) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{08}' => out.push_str("\\b"),
            '\u{0C}' => out.push_str("\\f"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04X}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
}

fn write_value(v: &Value, depth: usize, out: &mut String) {
    let pad = |d: usize| "  ".repeat(d);
    match v {
        Value::Object(map) if map.is_empty() => out.push_str(&format!("{{\n\n{}}}", pad(depth))),
        Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            out.push_str("{\n");
            for (i, k) in keys.iter().enumerate() {
                out.push_str(&pad(depth + 1));
                write_string(k, out);
                out.push_str(" : ");
                write_value(&map[*k], depth + 1, out);
                out.push_str(if i + 1 < keys.len() { ",\n" } else { "\n" });
            }
            out.push_str(&format!("{}}}", pad(depth)));
        }
        Value::Array(items) if items.is_empty() => out.push_str(&format!("[\n\n{}]", pad(depth))),
        Value::Array(items) => {
            out.push_str("[\n");
            for (i, item) in items.iter().enumerate() {
                out.push_str(&pad(depth + 1));
                write_value(item, depth + 1, out);
                out.push_str(if i + 1 < items.len() { ",\n" } else { "\n" });
            }
            out.push_str(&format!("{}]", pad(depth)));
        }
        Value::String(s) => write_string(s, out),
        other => out.push_str(&other.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn obj(v: Value) -> Map<String, Value> {
        v.as_object().cloned().unwrap_or_default()
    }

    #[test]
    fn pretty_matches_foundation() {
        let input = obj(
            json!({"b": [], "a": {"x": 1.5, "y": "é/\n\"q\"", "z": {}}, "n": 3, "t": true, "nul": null, "arr": [1, [2]]}),
        );
        let expected = "{\n  \"a\" : {\n    \"x\" : 1.5,\n    \"y\" : \"é/\\n\\\"q\\\"\",\n    \"z\" : {\n\n    }\n  },\n  \"arr\" : [\n    1,\n    [\n      2\n    ]\n  ],\n  \"b\" : [\n\n  ],\n  \"n\" : 3,\n  \"nul\" : null,\n  \"t\" : true\n}";
        assert_eq!(pretty(&input), expected);
    }

    #[test]
    fn titles_for_mcp_and_actions() {
        let call = make_call("i", "mcp__muse__muse_chat", &Map::new());
        assert_eq!(call.title, "Used chat (muse)");
        let call = make_call(
            "i",
            "Foo",
            &obj(json!({"action": "open", "file_path": "/a/b.txt"})),
        );
        assert_eq!(call.title, "Foo: open b.txt");
        let call = make_call(
            "i",
            "WebFetch",
            &obj(json!({"url": "https://example.com:8080/x?y"})),
        );
        assert_eq!(call.title, "Read example.com");
    }
}
