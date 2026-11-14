use notchling_engine::versions::{mark, Mark, Method, Tool, ToolState, Version, WorkerTools};
use notchling_local_api::ops;
use notchling_locale as t;
use serde_json::{json, Value};

use crate::acting::Request;
use crate::actions::Actions;
use crate::machines::versions_feed::mac_side;
use crate::machines::versions_model::mark_text;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorkerLine {
    pub machine: String,
    pub text: String,
    pub mark: Mark,
    pub state: &'static str,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClientPart {
    pub found: String,
    pub workers: Vec<WorkerLine>,
}

impl ClientPart {
    pub fn not_running() -> ClientPart {
        ClientPart {
            found: t::settings::client_not_running().to_owned(),
            workers: Vec::new(),
        }
    }
}

fn shown_path(path: &str, home: &str) -> String {
    if home.len() > 1 {
        if let Some(rest) = path.strip_prefix(home) {
            if rest.starts_with('/') {
                return format!("~{rest}");
            }
        }
    }
    path.to_owned()
}

fn row_of(get: &Value, tool: Tool) -> Option<&Value> {
    get.get("tools")?
        .as_array()?
        .iter()
        .find(|row| row.get("tool").and_then(Value::as_str) == Some(tool.name()))
}

fn found_line(row: &Value, home: &str) -> String {
    let Some(path) = row.get("path").and_then(Value::as_str) else {
        return t::settings::client_not_found().to_owned();
    };
    let version = row
        .get("installed")
        .and_then(|i| i.get("version"))
        .and_then(Value::as_str)
        .and_then(Version::parse);
    let method = row
        .get("method")
        .cloned()
        .and_then(|m| serde_json::from_value::<Method>(m).ok())
        .filter(|m| *m != Method::Unknown);
    let path = shown_path(path, home);
    let version = version.map_or(t::settings::client_version_unknown().to_owned(), |v| {
        v.to_string()
    });
    match method {
        Some(method) => t::settings::client_found_by(&path, &version, method.title()),
        None => t::settings::client_found(&path, &version),
    }
}

fn worker_lines(tool: Tool, get: &Value, workers: &Value) -> Vec<WorkerLine> {
    let Some(map) = workers.get("workers").and_then(Value::as_object) else {
        return Vec::new();
    };
    let mac = mac_side(get).ok();
    let theirs = mac.as_ref().and_then(|m| m.compare_with().remove(&tool));
    let mut lines = Vec::new();
    for (machine, value) in map {
        let Ok(saved) = serde_json::from_value::<WorkerTools>(value.clone()) else {
            continue;
        };
        let Some(state) = saved.tools.get(&tool) else {
            continue;
        };
        let (text, mark_of) = match state {
            ToolState::NotInstalled => (
                t::settings::client_not_installed().to_owned(),
                Mark::NotInstalled,
            ),
            ToolState::Installed { version, .. } => {
                let shown = version
                    .as_ref()
                    .map_or(t::settings::client_version_unknown().to_owned(), |v| {
                        v.to_string()
                    });
                let mark_of = match &mac {
                    Some(_) => mark(tool, theirs.as_ref(), state, false),
                    None => Mark::Unknown,
                };
                (shown, mark_of)
            }
        };
        let method = match state {
            ToolState::Installed { method, .. } => Some(*method),
            ToolState::NotInstalled => None,
        };
        lines.push(WorkerLine {
            machine: machine.clone(),
            text,
            mark: mark_of,
            state: mark_text(mark_of, method),
        });
    }
    lines
}

pub fn client_part(tool: Tool, get: &Value, workers: &Value, home: &str) -> ClientPart {
    if get.get("tools").and_then(Value::as_array).is_none() {
        return ClientPart::not_running();
    }
    let found = row_of(get, tool).map_or(t::settings::client_not_found().to_owned(), |row| {
        found_line(row, home)
    });
    ClientPart {
        found,
        workers: worker_lines(tool, get, workers),
    }
}

pub fn load(actions: &Actions, tool: Tool, home: &str) -> ClientPart {
    let get = actions.read(&Request {
        op: ops::VERSIONS_GET,
        args: json!({}),
    });
    let Ok(get) = get else {
        return ClientPart::not_running();
    };
    let workers = actions
        .read(&Request {
            op: ops::VERSIONS_WORKERS,
            args: json!({}),
        })
        .unwrap_or(Value::Null);
    client_part(tool, &get, &workers, home)
}
