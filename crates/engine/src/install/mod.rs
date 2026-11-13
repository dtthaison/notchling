pub mod command;
mod edit;
pub mod file;
pub mod hooks;
mod jsonc;
mod render;
pub mod skills;

pub use command::{classify, hook_command, summary, Origin};
pub use file::{apply, load, plan, Applied, FileError, FilePlan, State};
pub use hooks::{
    detect, plan_hooks, Category, Found, HookPlan, Refused, Slot, Want, Wanted, SLOTS,
};
pub use skills::{apply_skills, plan_skills, Action, SkillPlan, SkillResult, SkillSpec, ToolRoots};

pub const PROMPT_HOOKS: &str = "prompt_hooks";
pub const PAGE_HOOKS: &str = "page_hooks";
pub const VAULT_SKILL: &str = "notchling-vault";

#[cfg(test)]
mod dup_tests;
#[cfg(test)]
mod edit_tests;
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
