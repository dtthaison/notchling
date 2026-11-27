pub mod command;
pub(crate) mod edit;
pub mod file;
pub mod hooks;
pub(crate) mod jsonc;
pub(crate) mod render;
pub mod skills;
pub mod toml_edit;

pub use command::{agy_hook_command, agy_hook_event, classify, hook_command, summary, Origin};
pub use file::{
    apply, apply_expecting, before_hash, load, plan, text_hash, Applied, FileError, FilePlan, State,
};
pub use hooks::{
    detect, plan_hooks, Category, Found, HookPlan, Refused, Slot, Want, Wanted, SLOTS,
};
pub use skills::{
    apply_skills, apply_skills_only, part_id, plan_skills, rendered, skill_texts, Action, Only,
    SkillPlan, SkillResult, SkillSpec, ToolRoots, STALE,
};

pub const PROMPT_HOOKS: &str = "prompt_hooks";
pub const PAGE_HOOKS: &str = "page_hooks";
pub const GUARD_HOOKS: &str = "guard_hooks";
pub const VAULT_SKILL: &str = "notchling-vault";

#[cfg(test)]
mod dup_tests;
#[cfg(test)]
mod edit_tests;
#[cfg(test)]
mod fence_slot_tests;
#[cfg(test)]
mod file_tests;
#[cfg(test)]
mod fuzz_tests;
#[cfg(test)]
mod hooks_tests;
#[cfg(test)]
mod jsonc_tests;
#[cfg(test)]
mod skills_tests;
#[cfg(test)]
mod testutil;
#[cfg(test)]
mod toml_edit_tests;
