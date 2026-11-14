pub mod check;
mod identity;
pub mod mac;
pub mod mark;
mod method;
pub mod mismatch;
mod parse;
pub mod record;
pub mod reference;
pub mod soak;
mod version;
pub mod worker;

#[cfg(test)]
mod check_tests;
#[cfg(test)]
mod identity_tests;
#[cfg(test)]
mod mac_tests;
#[cfg(test)]
mod method_tests;
#[cfg(test)]
mod mismatch_tests;
#[cfg(test)]
mod parse_tests;
#[cfg(test)]
mod record_tests;
#[cfg(test)]
mod reference_tests;
#[cfg(test)]
mod soak_tests;
#[cfg(test)]
mod version_tests;
#[cfg(test)]
mod worker_tests;

pub use identity::{Identity, IdentityCache};
pub use mark::{mark, Mark};
pub use method::{can_pin, Method};
pub use mismatch::{mismatch, mismatch_between, Mismatch};
pub use parse::cli_output;
pub use version::Version;
pub use worker::{parse_probe, ToolState, WorkerTools};

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Tool {
    #[serde(rename = "claude")]
    Claude,
    #[serde(rename = "codex")]
    Codex,
    #[serde(rename = "antigravity", alias = "agy")]
    Agy,
    #[serde(rename = "notchling")]
    Notchling,
}

impl Tool {
    pub const ALL: [Tool; 4] = [Tool::Claude, Tool::Codex, Tool::Agy, Tool::Notchling];

    pub fn title(self) -> &'static str {
        match self {
            Tool::Claude => "Claude Code",
            Tool::Codex => "Codex",
            Tool::Agy => "Antigravity",
            Tool::Notchling => "Notchling",
        }
    }

    pub fn command(self) -> &'static str {
        match self {
            Tool::Claude => "claude",
            Tool::Codex => "codex",
            Tool::Agy => "agy",
            Tool::Notchling => "notchling",
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Tool::Claude => "claude",
            Tool::Codex => "codex",
            Tool::Agy => "antigravity",
            Tool::Notchling => "notchling",
        }
    }

    pub fn from_name(name: &str) -> Option<Tool> {
        match name {
            "claude" => Some(Tool::Claude),
            "codex" => Some(Tool::Codex),
            "antigravity" | "agy" => Some(Tool::Agy),
            "notchling" => Some(Tool::Notchling),
            _ => None,
        }
    }
}
