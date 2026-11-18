use super::update::CODEX_INSTALL_URL;
use super::worker_update::strict;
use super::{Tool, ToolState, Version};
use crate::lanes::quote;

pub const INSTALLABLE: [Tool; 2] = [Tool::Codex, Tool::Agy];

pub const AGY_INSTALL_URL: &str = "https://antigravity.google/cli/install.sh";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InstallPlan {
    pub script: String,
    pub target: Option<Version>,
    pub note: Option<String>,
}

fn codex_script(target: Option<&Version>) -> String {
    let release = target.map_or(String::new(), |v| {
        format!(" --release {}", quote(v.as_str()))
    });
    let package = format!("@openai/codex@{}", target.map_or("latest", Version::as_str));
    format!(
        "if command -v curl >/dev/null 2>&1; then s=$(curl -fsSL {CODEX_INSTALL_URL}) && printf '%s\\n' \"$s\" | sh -s --{release}; elif command -v npm >/dev/null 2>&1; then npm install -g {}; else echo 'curl and npm are both missing' >&2; exit 1; fi",
        quote(&package)
    )
}

fn agy_script() -> String {
    format!(
        "if command -v curl >/dev/null 2>&1; then s=$(curl -fsSL {AGY_INSTALL_URL}) && printf '%s\\n' \"$s\" | bash -s --; else echo 'curl is missing' >&2; exit 1; fi"
    )
}

pub fn install_plan(
    machine: &str,
    tool: Tool,
    state: &ToolState,
    mac: Option<&Version>,
) -> Result<InstallPlan, String> {
    if !INSTALLABLE.contains(&tool) {
        return Err(format!(
            "Notchling doesn't install {} this way",
            tool.title()
        ));
    }
    if matches!(state, ToolState::Installed { .. }) {
        return Err(format!(
            "{} is already installed on {machine}",
            tool.title()
        ));
    }
    match tool {
        Tool::Codex => match mac {
            Some(v) if strict(v).is_none() => Err(format!(
                "{v} isn't a version Notchling installs. Install Codex on {machine} yourself, then check again."
            )),
            Some(v) => Ok(InstallPlan {
                script: codex_script(Some(v)),
                target: Some(v.clone()),
                note: None,
            }),
            None => Ok(InstallPlan {
                script: codex_script(None),
                target: None,
                note: Some(format!(
                    "This Mac has no checked Codex version, so Notchling installs the newest on {machine}."
                )),
            }),
        },
        _ => Ok(InstallPlan {
            script: agy_script(),
            target: None,
            note: Some(match mac {
                Some(v) => format!(
                    "Antigravity's installer takes no exact version, so Notchling installs the newest on {machine}. It may not be {v}."
                ),
                None => format!(
                    "Antigravity's installer takes no exact version, so Notchling installs the newest on {machine}."
                ),
            }),
        }),
    }
}

pub fn verdict(
    machine: &str,
    tool: Tool,
    target: Option<&Version>,
    after: Option<&ToolState>,
) -> Result<String, String> {
    let name = tool.title();
    match (after, target) {
        (Some(ToolState::Installed { version, .. }), Some(want)) => match version {
            Some(found) if found == want => Ok(format!("{name} {want} is installed on {machine}")),
            Some(found) => Err(format!("{name} on {machine} reports {found}, not {want}")),
            None => Err(format!(
                "{name} is on {machine}, but its version couldn't be read; it should be {want}"
            )),
        },
        (Some(ToolState::Installed { version, .. }), None) => Ok(match version {
            Some(found) => format!("{name} {found} is installed on {machine}"),
            None => format!("{name} is installed on {machine}"),
        }),
        _ => Err(format!(
            "{name} still isn't found on {machine} after the install"
        )),
    }
}
