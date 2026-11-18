pub mod agentapi;
pub mod bundle;
pub mod config_file;
pub mod deliver;
pub mod hook_io;
pub mod hooks_file;
pub mod ids;
pub mod jobs;
pub mod plugin;
pub mod steps;

use std::path::{Path, PathBuf};

pub const PLUGIN: &str = "notchling";
pub const SIDECAR_FOLDER: &str = "sidecar";
pub const SIDECAR_ID: &str = "notchling/sidecar";
pub const HOOK_NAME: &str = "notchling";
pub const PRE_TOOL: &str = "pre-tool";
pub const PRE_INVOCATION: &str = "pre-invocation";
pub const TOOL_MATCHER: &str =
    "run_command|write_to_file|replace_file_content|multi_replace_file_content|read_url_content";
pub const TOOL_TIMEOUT_SECS: u64 = 120;
pub const INVOCATION_TIMEOUT_SECS: u64 = 5;
pub const ASK_WAIT_SECS: u64 = 100;

#[derive(Clone, Debug)]
pub struct Roots {
    pub config: PathBuf,
    pub data: PathBuf,
    pub apps: Vec<PathBuf>,
}

impl Roots {
    pub fn under(home: &Path) -> Roots {
        Roots {
            config: home.join(".gemini/config"),
            data: home.join(".gemini/antigravity"),
            apps: vec![
                PathBuf::from("/Applications/Antigravity.app"),
                home.join("Applications/Antigravity.app"),
            ],
        }
    }

    pub fn config_json(&self) -> PathBuf {
        self.config.join("config.json")
    }

    pub fn hooks_json(&self) -> PathBuf {
        self.config.join("hooks.json")
    }

    pub fn plugin_dir(&self) -> PathBuf {
        self.config.join("plugins").join(PLUGIN)
    }

    pub fn conversations(&self) -> PathBuf {
        self.data.join("conversations")
    }

    pub fn installed(&self) -> bool {
        self.apps.iter().any(|app| app.is_dir())
    }
}

#[cfg(test)]
mod agentapi_tests;
#[cfg(test)]
mod bundle_tests;
#[cfg(test)]
mod config_file_tests;
#[cfg(test)]
mod deliver_tests;
#[cfg(test)]
mod hook_io_tests;
#[cfg(test)]
mod hooks_file_tests;
#[cfg(test)]
mod ids_tests;
#[cfg(test)]
mod jobs_tests;
#[cfg(test)]
mod plugin_tests;
#[cfg(test)]
mod steps_tests;
