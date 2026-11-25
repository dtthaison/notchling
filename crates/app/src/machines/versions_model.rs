use notchling_engine::machines::step::{CheckResult, StepId};
use notchling_engine::versions::{mark, Mark, Method, Tool, ToolState, Version, WorkerTools};

use super::versions_feed::{MacSide, MacVersionsError};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VersionRow {
    pub tool: Tool,
    pub text: String,
    pub mark: Mark,
    pub mark_text: &'static str,
}

pub fn mark_text(mark: Mark, method: Option<Method>) -> &'static str {
    match mark {
        Mark::Same => "Same as the Mac",
        Mark::Behind => "Behind the Mac",
        Mark::Ahead => "Ahead of the Mac",
        Mark::Held => "Held",
        Mark::CantPin if method == Some(Method::Brew) => "Homebrew can't install an exact version",
        Mark::CantPin => "Can't be set to an exact version here",
        Mark::NotInstalled => "Not installed",
        Mark::Unknown => "Can't tell",
    }
}

fn own_text(version: Option<&Version>, method: Method) -> String {
    let version = version.map_or("version unknown".to_owned(), |v| v.to_string());
    match method {
        Method::Unknown => version,
        known => format!("{version} · {}", known.title()),
    }
}

pub fn rows(
    mac: &Result<MacSide, MacVersionsError>,
    worker: &WorkerTools,
    held: &[Tool],
) -> Vec<VersionRow> {
    let compare = mac.as_ref().map(MacSide::compare_with).ok();
    let mut out = Vec::new();
    for tool in Tool::ALL {
        let Some(state) = worker.tools.get(&tool) else {
            continue;
        };
        let (text, mark_of, method) = match state {
            ToolState::NotInstalled => ("Not installed".to_owned(), Mark::NotInstalled, None),
            ToolState::Installed {
                version, method, ..
            } => {
                let mut text = own_text(version.as_ref(), *method);
                let Some(compare) = &compare else {
                    out.push(VersionRow {
                        tool,
                        text,
                        mark: Mark::Unknown,
                        mark_text: mark_text(Mark::Unknown, None),
                    });
                    continue;
                };
                let theirs = compare.get(&tool);
                if let Some(theirs) = theirs {
                    let checked = mac
                        .as_ref()
                        .ok()
                        .and_then(|m| m.tools.get(&tool))
                        .is_some_and(|t| t.usable);
                    if checked {
                        text.push_str(&format!(" · the Mac has {theirs}"));
                    } else {
                        text.push_str(&format!(" · the Mac has {theirs} (not checked yet)"));
                    }
                }
                (
                    text,
                    mark(tool, theirs, state, held.contains(&tool)),
                    Some(*method),
                )
            }
        };
        out.push(VersionRow {
            tool,
            text,
            mark: mark_of,
            mark_text: mark_text(mark_of, method),
        });
    }
    out
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlaceFlag {
    Off,
    On,
    CantRead,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlaceRow {
    pub name: String,
    pub detail: String,
    pub flag: PlaceFlag,
}

pub fn place_rows(result: &CheckResult) -> Vec<PlaceRow> {
    result
        .rows
        .iter()
        .map(|row| PlaceRow {
            name: row.name.clone(),
            detail: row.detail.clone(),
            flag: if row.ok {
                PlaceFlag::Off
            } else if row.name.starts_with("Can't read ") {
                PlaceFlag::CantRead
            } else {
                PlaceFlag::On
            },
        })
        .collect()
}

pub fn replace_label(id: &StepId) -> &'static str {
    if id.self_update_tool().is_some() {
        "Self-Update On…"
    } else {
        "Replace key…"
    }
}

pub fn remove_note(titles: &[String]) -> String {
    const PLAIN: &str = "The machine itself is not changed.";
    if titles.is_empty() {
        return PLAIN.to_owned();
    }
    let tools: Vec<&str> = titles
        .iter()
        .map(|t| t.strip_prefix("Self-Update On: ").unwrap_or(t))
        .collect();
    format!(
        "{PLAIN} Self-update is still off there for {}; use Self-Update On… on its card first to take that back.",
        tools.join(", ")
    )
}

pub fn replace_tip(id: &StepId) -> &'static str {
    if id.self_update_tool().is_some() {
        "Takes back only what Notchling set; a value you set yourself stays"
    } else {
        "Makes a new read-only deploy key, proves it works, then removes the old one"
    }
}

pub fn drift_lines(
    mac: &Result<MacSide, MacVersionsError>,
    worker: &WorkerTools,
) -> Vec<(Tool, String)> {
    let Ok(side) = mac else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for tool in Tool::ALL {
        let Some(ToolState::Installed {
            version: Some(found),
            ..
        }) = worker.tools.get(&tool)
        else {
            continue;
        };
        let Some(mac_tool) = side.tools.get(&tool).filter(|t| t.usable) else {
            continue;
        };
        let Some(expected) = &mac_tool.reference else {
            continue;
        };
        if found == expected {
            continue;
        }
        let mut line = format!("Drifted: {found}, the Mac {expected}");
        if found > expected {
            line.push_str(". Moved down only by its own click");
        }
        out.push((tool, line));
    }
    out
}

pub fn banner(mac: &Result<MacSide, MacVersionsError>) -> Option<&'static str> {
    match mac {
        Err(MacVersionsError::NotRunning) => Some("Notchling isn't running"),
        Err(MacVersionsError::Bad(_)) => Some("Notchling gave an answer Machines can't read"),
        Ok(side) => side
            .tools
            .values()
            .any(|t| t.reference.is_some() && !t.usable)
            .then_some("The Mac's check is out of date"),
    }
}
