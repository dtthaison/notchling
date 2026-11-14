use notchling_engine::versions::{mark, Mark, Method, Tool, ToolState, Version, WorkerTools};
use notchling_local_api::ops;
use notchling_locale as t;
use serde_json::{json, Value};

use crate::acting::Request;
use crate::actions::Actions;
use crate::machines::versions_feed::mac_side;
use crate::machines::versions_model::mark_text;
use crate::switches::{Switch, Switches};
use notchling_engine::pages::clock::local_offset_secs;

pub use super::client_check::Extras;
use super::client_check::{
    check_line, go_back, latest_line, soak_line, update_button, CheckLine, GoBack, UpdateButton,
};

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
    pub latest: Option<String>,
    pub update: Option<UpdateButton>,
    pub check: Option<CheckLine>,
    pub soak: Option<(String, bool)>,
    pub go_back: Option<GoBack>,
    pub busy: bool,
}

impl ClientPart {
    pub fn not_running() -> ClientPart {
        ClientPart {
            found: t::settings::client_not_running().to_owned(),
            workers: Vec::new(),
            latest: None,
            update: None,
            check: None,
            soak: None,
            go_back: None,
            busy: false,
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
    client_part_with(tool, get, workers, home, Extras::default())
}

pub fn client_part_with(
    tool: Tool,
    get: &Value,
    workers: &Value,
    home: &str,
    extras: Extras,
) -> ClientPart {
    if get.get("tools").and_then(Value::as_array).is_none() {
        return ClientPart::not_running();
    }
    let row = row_of(get, tool);
    let found = row.map_or(t::settings::client_not_found().to_owned(), |row| {
        found_line(row, home)
    });
    ClientPart {
        found,
        workers: worker_lines(tool, get, workers),
        latest: row.and_then(latest_line),
        update: row.and_then(|r| update_button(tool, r, extras.install_on)),
        check: row.map(|r| check_line(tool, r, extras.offset_secs)),
        soak: row.and_then(soak_line),
        go_back: row.and_then(|r| go_back(tool, r)),
        busy: row.is_some_and(super::client_check::busy),
    }
}

pub fn load(actions: &Actions, tool: Tool, home: &str) -> ClientPart {
    let get = actions.read(&Request {
        op: ops::VERSIONS_GET,
        args: json!({}),
    });
    let Ok(mut get) = get else {
        return ClientPart::not_running();
    };
    let workers = actions
        .read(&Request {
            op: ops::VERSIONS_WORKERS,
            args: json!({}),
        })
        .unwrap_or(Value::Null);
    let latest = actions.read(&Request {
        op: ops::VERSIONS_LATEST,
        args: json!({"tool": tool.name()}),
    });
    if let (Ok(latest), Some(rows)) = (latest, get.get_mut("tools").and_then(Value::as_array_mut)) {
        for row in rows.iter_mut() {
            if row.get("tool").and_then(Value::as_str) == Some(tool.name()) {
                row["latest"] = latest.get("latest").cloned().unwrap_or(Value::Null);
            }
        }
    }
    let install_on = actions
        .read(&Request {
            op: ops::SETTINGS_SWITCHES,
            args: json!({}),
        })
        .map(|reply| Switches::from_reply(&reply).is_on(Switch::Install))
        .unwrap_or(false);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0.0, |d| d.as_secs_f64());
    client_part_with(
        tool,
        &get,
        &workers,
        home,
        Extras {
            install_on,
            offset_secs: local_offset_secs(now),
        },
    )
}
