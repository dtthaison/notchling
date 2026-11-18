use super::ids::is_uuid;
use serde_json::{json, Map, Value};

pub const MAX_INPUT_CHARS: usize = 8_000;

#[derive(Clone, Debug, PartialEq)]
pub struct ToolAsk {
    pub conversation: String,
    pub tool: String,
    pub args: Value,
    pub cwd: String,
}

fn conversation(value: &Value) -> Option<String> {
    let id = value.get("conversationId")?.as_str()?;
    is_uuid(id).then(|| id.to_string())
}

pub fn parse_conversation(input: &[u8]) -> Option<String> {
    conversation(&serde_json::from_slice(input).ok()?)
}

pub fn parse_tool(input: &[u8]) -> Option<ToolAsk> {
    let value: Value = serde_json::from_slice(input).ok()?;
    let call = value.get("toolCall")?;
    let tool = call.get("name")?.as_str()?;
    if tool.is_empty()
        || tool.len() > 64
        || !tool
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        return None;
    }
    Some(ToolAsk {
        conversation: conversation(&value)?,
        tool: tool.to_string(),
        args: shrink(call.get("args").cloned().unwrap_or(Value::Null)),
        cwd: value
            .get("cwd")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .chars()
            .take(1000)
            .collect(),
    })
}

fn shrink(args: Value) -> Value {
    let text = args.to_string();
    if text.chars().count() <= MAX_INPUT_CHARS {
        return args;
    }
    match args {
        Value::Object(map) => {
            let mut out = Map::new();
            for (key, value) in map {
                let cut = match value {
                    Value::String(s) if s.chars().count() > 2_000 => {
                        Value::String(s.chars().take(2_000).collect::<String>() + "...")
                    }
                    other => other,
                };
                out.insert(key, cut);
            }
            let out = Value::Object(out);
            if out.to_string().chars().count() <= MAX_INPUT_CHARS {
                out
            } else {
                json!({"note": "The tool input was too long to show."})
            }
        }
        _ => json!({"note": "The tool input was too long to show."}),
    }
}

fn first_text<'a>(args: &'a Value, keys: &[&str]) -> Option<&'a str> {
    keys.iter()
        .find_map(|k| args.get(*k).and_then(Value::as_str))
        .filter(|s| !s.is_empty())
}

pub fn prompt_event(ask: &ToolAsk, row_id: &str) -> Value {
    let mut input = ask.args.as_object().cloned().unwrap_or_default();
    if let Some(command) = first_text(&ask.args, &["CommandLine", "commandLine"]) {
        input.insert("command".into(), json!(command));
    }
    if let Some(path) = first_text(
        &ask.args,
        &["TargetFile", "AbsolutePath", "FilePath", "Path"],
    ) {
        input.insert("file_path".into(), json!(path));
    }
    if let Some(url) = first_text(&ask.args, &["Url", "URL"]) {
        input.insert("url".into(), json!(url));
    }
    json!({
        "hook_event_name": "PermissionRequest",
        "session_id": row_id,
        "tool_name": ask.tool,
        "tool_input": input,
        "cwd": ask.cwd,
        "transcript_path": "",
    })
}

pub fn tool_output(answer: &str) -> Option<Value> {
    match answer {
        "allow" => Some(json!({"decision": "auto_approve"})),
        "deny" => Some(json!({
            "decision": "deny",
            "reason": "You said no in Notchling."
        })),
        _ => None,
    }
}

pub fn stop_output() -> Value {
    json!({
        "terminationBehavior": "terminate",
        "injectSteps": [{"errorMessage": {"userMessage": "Stopped from Notchling."}}],
    })
}
