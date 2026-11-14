use super::{cli_output, Method, Tool, Version};
use crate::machines::shell::is_safe_machine_path;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

const MAX_LINES: usize = 64;
const MAX_LINK: usize = 512;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "camelCase")]
pub enum ToolState {
    NotInstalled,
    Installed {
        version: Option<Version>,
        method: Method,
        path: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkerTools {
    pub tools: BTreeMap<Tool, ToolState>,
    pub at_ms: i64,
}

#[derive(Default)]
struct Lines {
    path: Option<String>,
    link: Option<String>,
    version: Option<String>,
}

fn tool_of(command: &str) -> Option<Tool> {
    Tool::ALL.into_iter().find(|t| t.command() == command)
}

fn first<T>(slot: &mut Option<T>, value: T) {
    if slot.is_none() {
        *slot = Some(value);
    }
}

fn plain_link(link: &str) -> bool {
    link.len() <= MAX_LINK
        && link
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "._-/+@".contains(c))
}

pub fn parse_probe(text: &str, now_ms: i64) -> WorkerTools {
    let mut seen: BTreeMap<Tool, Lines> = BTreeMap::new();
    for line in text.lines().take(MAX_LINES) {
        let line = line.trim_end_matches('\r');
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let Some((command, field)) = key.split_once('.') else {
            continue;
        };
        let Some(tool) = tool_of(command) else {
            continue;
        };
        let entry = seen.entry(tool).or_default();
        match field {
            "path" => first(&mut entry.path, value.to_string()),
            "link" => first(&mut entry.link, value.to_string()),
            "version" => first(&mut entry.version, value.to_string()),
            _ => {}
        }
    }
    let mut tools = BTreeMap::new();
    for tool in Tool::ALL {
        let lines = seen.remove(&tool).unwrap_or_default();
        let state = match lines.path.as_deref() {
            None | Some("") => ToolState::NotInstalled,
            Some(path) => {
                let version = lines.version.as_deref().and_then(|v| cli_output(tool, v));
                if is_safe_machine_path(path) {
                    let link = lines.link.as_deref().filter(|l| plain_link(l));
                    ToolState::Installed {
                        version,
                        method: Method::from_path(tool, path, link),
                        path: path.to_string(),
                    }
                } else {
                    ToolState::Installed {
                        version,
                        method: Method::Unknown,
                        path: String::new(),
                    }
                }
            }
        };
        tools.insert(tool, state);
    }
    WorkerTools {
        tools,
        at_ms: now_ms,
    }
}
