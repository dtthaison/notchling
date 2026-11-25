use crate::transcript::chat;
use crate::transcript::tail::MAX_LINE_BYTES;
use crate::transcript::text::{last_path_component, prefix_chars};
use crate::transcript::util::{date_field, int_of, parse_object, str_of};
use notchling_protocol::generated as wire;
use regex::Regex;
use serde_json::{Map, Value};
use std::collections::{BTreeMap, HashMap};
use std::sync::OnceLock;

pub const TAIL_BYTES: u64 = 8_000_000;
const DISTANT_PAST: f64 = -62_135_769_600.0;
const EDIT_TOOLS: [&str; 4] = ["Edit", "Write", "MultiEdit", "NotebookEdit"];

#[derive(Debug, Clone, PartialEq)]
pub struct MermaidBlock {
    pub source: String,
    pub origin: String,
    pub time: Option<f64>,
    pub message_id: String,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct TranscriptFacts {
    pub turns: Vec<wire::FlowTurn>,
    pub file_edits: BTreeMap<String, i64>,
    pub mermaid: Vec<MermaidBlock>,
    pub usage: Vec<wire::UsagePoint>,
    pub tool_counts: BTreeMap<String, i64>,
    pub local_urls: Vec<String>,
    pub device_hints: Vec<String>,
    pub newest_activity: Option<f64>,
}

impl TranscriptFacts {
    pub fn merge(&mut self, other: TranscriptFacts) {
        for (path, count) in other.file_edits {
            *self.file_edits.entry(path).or_default() += count;
        }
        self.mermaid.extend(other.mermaid);
        self.usage.extend(other.usage);
        for (tool, count) in other.tool_counts {
            *self.tool_counts.entry(tool).or_default() += count;
        }
        self.local_urls.extend(other.local_urls);
        for hint in other.device_hints {
            if !self.device_hints.contains(&hint) {
                self.device_hints.push(hint);
            }
        }
        if let Some(date) = other.newest_activity {
            self.newest_activity = Some(self.newest_activity.map_or(date, |own| own.max(date)));
        }
    }
}

fn url_pattern() -> Option<&'static Regex> {
    static PATTERN: OnceLock<Option<Regex>> = OnceLock::new();
    PATTERN
        .get_or_init(|| {
            Regex::new(r#"https?://(?:localhost|127\.0\.0\.1):[0-9]{2,5}[^\s"'<>)\]\\`,]*"#).ok()
        })
        .as_ref()
}

fn device_pattern() -> Option<&'static Regex> {
    static PATTERN: OnceLock<Option<Regex>> = OnceLock::new();
    PATTERN
        .get_or_init(|| {
            Regex::new(
                r"(?:iPhone|iPad)(?: [0-9]{1,2}[a-z]?)?(?: (?:Pro Max|Pro|Plus|mini|Air|e))?",
            )
            .ok()
        })
        .as_ref()
}

fn mermaid_pattern() -> Option<&'static Regex> {
    static PATTERN: OnceLock<Option<Regex>> = OnceLock::new();
    PATTERN
        .get_or_init(|| Regex::new(r"(?s)```mermaid[ \t]*\n(.*?)```").ok())
        .as_ref()
}

pub fn analyze_file(path: &str, is_main: bool, origin: &str, tail_bytes: u64) -> TranscriptFacts {
    match chat::tail(path, tail_bytes) {
        Some(lines) => analyze_lines(&lines, is_main, origin, path),
        None => TranscriptFacts::default(),
    }
}

pub fn analyze_lines(lines: &[String], is_main: bool, origin: &str, path: &str) -> TranscriptFacts {
    let mut facts = TranscriptFacts::default();
    let mut turns: Vec<wire::FlowTurn> = Vec::new();
    let mut usage_by_id: HashMap<String, wire::UsagePoint> = HashMap::new();
    let mut usage_order: Vec<String> = Vec::new();

    for (index, line) in lines.iter().enumerate() {
        if line.len() > MAX_LINE_BYTES {
            continue;
        }
        let Some(map) = parse_object(line.as_bytes()) else {
            continue;
        };
        let object = Value::Object(map);
        let Some(kind) = str_of(&object, "type").filter(|k| *k == "user" || *k == "assistant")
        else {
            continue;
        };
        let Some(message) = object.get("message").filter(|m| m.is_object()) else {
            continue;
        };
        if is_main
            && (object.get("isSidechain").and_then(Value::as_bool) == Some(true)
                || object.get("isMeta").and_then(Value::as_bool) == Some(true))
        {
            continue;
        }
        let time = date_field(&object);
        if let Some(time) = time {
            facts.newest_activity = Some(facts.newest_activity.map_or(time, |t| t.max(time)));
        }
        let blocks: Vec<&Value> = message
            .get("content")
            .and_then(Value::as_array)
            .map(|b| b.iter().filter(|b| b.is_object()).collect())
            .unwrap_or_default();

        if kind == "user" {
            user_line(&mut facts, &mut turns, message, &blocks, is_main);
            continue;
        }

        let message_id = str_of(message, "id")
            .map(String::from)
            .or_else(|| str_of(&object, "uuid").map(String::from))
            .unwrap_or_else(|| format!("{path}:{index}"));
        if let Some(usage) = message.get("usage").filter(|u| u.is_object()) {
            if !usage_by_id.contains_key(&message_id) {
                usage_order.push(message_id.clone());
            }
            let count = |key: &str| usage.get(key).and_then(int_of).unwrap_or(0);
            usage_by_id.insert(
                message_id.clone(),
                wire::UsagePoint {
                    time: time.unwrap_or(DISTANT_PAST) * 1000.0,
                    input: count("input_tokens"),
                    output: count("output_tokens"),
                    cache_creation: count("cache_creation_input_tokens"),
                    cache_read: count("cache_read_input_tokens"),
                },
            );
        }
        for block in blocks {
            match str_of(block, "type") {
                Some("tool_use") => tool_use(&mut facts, &mut turns, block, time, is_main),
                Some("text") => {
                    let Some(body) = str_of(block, "text") else {
                        continue;
                    };
                    collect_urls(body, &mut facts);
                    for source in mermaid_sources(body) {
                        facts.mermaid.push(MermaidBlock {
                            source,
                            origin: origin.to_string(),
                            time,
                            message_id: message_id.clone(),
                        });
                    }
                }
                _ => {}
            }
        }
    }
    facts.turns = turns;
    facts.usage = usage_order
        .iter()
        .filter_map(|id| usage_by_id.remove(id))
        .collect();
    facts
}

fn user_line(
    facts: &mut TranscriptFacts,
    turns: &mut Vec<wire::FlowTurn>,
    message: &Value,
    blocks: &[&Value],
    is_main: bool,
) {
    let prompt = match message.get("content").and_then(Value::as_str) {
        Some(plain) => plain.to_string(),
        None => blocks
            .iter()
            .filter(|b| str_of(b, "type") == Some("text"))
            .filter_map(|b| str_of(b, "text"))
            .collect::<Vec<_>>()
            .join("\n"),
    };
    collect_urls(&prompt, facts);
    for block in blocks
        .iter()
        .filter(|b| str_of(b, "type") == Some("tool_result"))
    {
        match block.get("content") {
            Some(Value::String(output)) => collect_urls(output, facts),
            Some(Value::Array(parts)) => {
                for part in parts {
                    if let Some(output) = str_of(part, "text") {
                        collect_urls(output, facts);
                    }
                }
            }
            _ => {}
        }
    }
    let cleaned = prompt.trim();
    if is_main
        && !cleaned.is_empty()
        && !cleaned.starts_with('<')
        && !cleaned.starts_with("[Request interrupted")
    {
        turns.push(wire::FlowTurn {
            index: turns.len() as i64 + 1,
            prompt: prefix_chars(cleaned, 140).to_string(),
            calls: Vec::new(),
        });
    }
}

fn tool_use(
    facts: &mut TranscriptFacts,
    turns: &mut Vec<wire::FlowTurn>,
    block: &Value,
    time: Option<f64>,
    is_main: bool,
) {
    let name = str_of(block, "name").unwrap_or("");
    let empty = Map::new();
    let input = block
        .get("input")
        .and_then(Value::as_object)
        .unwrap_or(&empty);
    *facts.tool_counts.entry(name.to_string()).or_default() += 1;
    let call = flow_call(name, input, time);
    if EDIT_TOOLS.contains(&name) {
        let file = input
            .get("file_path")
            .and_then(Value::as_str)
            .or_else(|| input.get("notebook_path").and_then(Value::as_str));
        if let Some(file) = file {
            *facts.file_edits.entry(file.to_string()).or_default() += 1;
        }
    }
    if is_main {
        if turns.is_empty() {
            turns.push(wire::FlowTurn {
                index: 1,
                prompt: String::new(),
                calls: Vec::new(),
            });
        }
        if let Some(last) = turns.last_mut() {
            last.calls.push(call);
        }
    }
    let command = input.get("command").and_then(Value::as_str);
    if let (true, Some(command)) = (name == "Bash", command) {
        if command.contains("simctl") || command.contains("adb") || command.contains("emulator") {
            collect_devices(command, facts);
        }
    }
}

pub fn mermaid_sources(text: &str) -> Vec<String> {
    if !text.contains("```mermaid") {
        return Vec::new();
    }
    let Some(pattern) = mermaid_pattern() else {
        return Vec::new();
    };
    pattern
        .captures_iter(text)
        .filter_map(|c| c.get(1))
        .map(|m| m.as_str().trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

pub fn flow_call(name: &str, input: &Map<String, Value>, time: Option<f64>) -> wire::FlowCall {
    let text = |key: &str| input.get(key).and_then(Value::as_str);
    let base = |key: &str| last_path_component(text(key).unwrap_or("")).to_string();
    let short = |value: &str| prefix_chars(value, 60).to_string();
    let mut subagent = false;
    let target = match name {
        "Read" | "Edit" | "Write" | "MultiEdit" => base("file_path"),
        "NotebookEdit" => base("notebook_path"),
        "Bash" => short(&text("command").unwrap_or("").replace('\n', " ")),
        "Agent" | "Task" => {
            subagent = true;
            short(
                text("description")
                    .or_else(|| text("subagent_type"))
                    .unwrap_or("subagent"),
            )
        }
        "Grep" | "Glob" => short(text("pattern").unwrap_or("")),
        "WebFetch" => short(text("url").unwrap_or("")),
        "WebSearch" => short(text("query").unwrap_or("")),
        _ => String::new(),
    };
    wire::FlowCall {
        tool: name.to_string(),
        target,
        subagent,
        edit: EDIT_TOOLS.contains(&name),
        time: time.map(|t| t * 1000.0),
    }
}

fn collect_urls(text: &str, facts: &mut TranscriptFacts) {
    if !text.contains("://") {
        return;
    }
    let Some(pattern) = url_pattern() else {
        return;
    };
    for found in pattern.find_iter(text) {
        let url = found.as_str().trim_end_matches(['.', ',', ';', ':']);
        facts.local_urls.push(url.to_string());
    }
}

fn collect_devices(text: &str, facts: &mut TranscriptFacts) {
    let Some(pattern) = device_pattern() else {
        return;
    };
    for found in pattern.find_iter(text) {
        let name = found.as_str().to_string();
        if !facts.device_hints.contains(&name) {
            facts.device_hints.push(name);
        }
    }
}
