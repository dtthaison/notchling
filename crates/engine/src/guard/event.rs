use serde_json::Value;

pub const MAX_ID: usize = 80;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Pre,
    SubagentStart,
    SubagentStop,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Event {
    pub kind: Kind,
    pub session: String,
    pub agent: Option<String>,
    pub tool: String,
    pub tool_use_id: Option<String>,
    pub cwd: Option<String>,
    pub input: Value,
}

pub fn plain_id(text: &str) -> bool {
    !text.is_empty()
        && text.len() <= MAX_ID
        && text
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

fn id_of(event: &Value, key: &str) -> Option<String> {
    event
        .get(key)
        .and_then(Value::as_str)
        .filter(|s| plain_id(s))
        .map(str::to_string)
}

pub fn parse(bytes: &[u8]) -> Option<Event> {
    let event: Value = serde_json::from_slice(bytes).ok()?;
    let kind = match event.get("hook_event_name").and_then(Value::as_str)? {
        "PreToolUse" => Kind::Pre,
        "SubagentStart" => Kind::SubagentStart,
        "SubagentStop" => Kind::SubagentStop,
        _ => return None,
    };
    Some(Event {
        kind,
        session: id_of(&event, "session_id")?,
        agent: id_of(&event, "agent_id"),
        tool: event
            .get("tool_name")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
        tool_use_id: id_of(&event, "tool_use_id"),
        cwd: event.get("cwd").and_then(Value::as_str).map(str::to_string),
        input: event.get("tool_input").cloned().unwrap_or(Value::Null),
    })
}

impl Event {
    pub fn text(&self, key: &str) -> Option<&str> {
        self.input.get(key).and_then(Value::as_str)
    }

    pub fn logged_tool(&self) -> &'static str {
        match self.tool.as_str() {
            "Bash" => "Bash",
            "Read" => "Read",
            "Agent" | "Task" => "Agent",
            _ => "other",
        }
    }

    pub fn spawns(&self) -> bool {
        matches!(self.tool.as_str(), "Agent" | "Task")
    }
}
