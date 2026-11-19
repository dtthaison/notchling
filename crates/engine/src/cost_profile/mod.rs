pub mod action;
pub mod apply;
pub mod check;
pub mod detect;
pub mod diff;
pub mod import;
pub mod keys;
pub mod manifest;
pub mod model;
pub mod plan;
pub mod plan_other;
pub mod presets;
pub mod probe;
pub mod pstack;
pub mod render;
pub mod resolve;
pub mod service;
pub mod undo;

pub use action::{
    Action, Env, Kind, Options, Plan, State, CONSENTS, CONSENT_CODEX_AGENTS, CONSENT_CODEX_CONFIG,
    CONSENT_ORCHESTRATOR, CONSENT_ORCHESTRATOR_NEW, CONSENT_PSTACK, CONSENT_RULES,
};
pub use apply::{apply, ActionResult, Applied};
pub use check::{check, resolve as resolve_drift, Choice, Row};
pub use manifest::Manifest;
pub use model::{Cell, Habits, Profile, Role, Stored, APP_KEY};
pub use plan::plan;
pub use resolve::{resolve, Available, Offer, Resolved};
pub use service::Service;
pub use undo::{undo, Outcome, UndoResult};

#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_setup;
#[cfg(test)]
mod tests_undo;
