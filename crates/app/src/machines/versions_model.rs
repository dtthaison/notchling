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
