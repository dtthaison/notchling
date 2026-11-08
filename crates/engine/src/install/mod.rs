//! E13: installing into other tools. The hook entries in Claude Code's `settings.json`, and the
//! `notchling-*` skills in the skills folders of Claude Code, Codex and Antigravity CLI.
//!
//! Built and switched off (decision 0011): the daemon's `install` switch is off at every start,
//! and until the maintainer turns it on the Swift app owns both. Nothing here runs on its own;
//! every path is a parameter, and the tests use temp folders only.
//!
//! | What | Where |
//! | --- | --- |
//! | Whose a hook command is, and ours spelled out | [`command`] |
//! | Planning the hook entries (add, replace, remove, detect) | [`hooks`] |
//! | The file: backup, atomic write, links, read-only | [`file`] |
//! | The skills folders | [`skills`] |
//! | Reading JSON with positions; splicing edits; rendering new text | `jsonc`, `edit`, `render` |
//!
//! # Rules
//!
//! - The user's file is theirs. A change touches only our own entries; every other byte stays as
//!   written: key order, spacing, comments, number spelling, unknown keys. The edit is checked
//!   against the same change made to the parsed data, and a difference is a refusal.
//! - A file that is broken, not an object, not UTF-8, too big, read-only or a dangling link is
//!   refused and left as it is. An empty or missing file is made into `{}` plus our entries.
//! - A backup is written before every change; the write is atomic and keeps the file's mode.
//! - The Swift app's `--hook` entries are reported and never changed or removed; retirement
//!   (decision 0011) rewrites them.
//! - A skill folder is ours only if it holds our mark file; others are never written or removed.
//! - Nothing from the file reaches a message except the shape of our own hook command.
//!
//! # Not yet
//!
//! - `notchling hook` still passes every Artifact event ([`crate::pages::hook`] is not wired
//!   into it: that would write the Swift app's pages folder), so the page entries this installs
//!   do nothing until the maintainer hands the page hook over.
//! - Retiring the Swift app's entries (rewriting their command to the new binary) is a later
//!   step; here they are only reported.

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

/// The item names the ops use for the two hook groups.
pub const PROMPT_HOOKS: &str = "prompt_hooks";
pub const PAGE_HOOKS: &str = "page_hooks";
/// The vault skill's folder name (its text comes from the vault).
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
