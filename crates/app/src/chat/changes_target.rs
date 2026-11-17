use std::path::Path;
use std::sync::Arc;

use serde_json::{json, Value};

use super::branch::{self, BranchModel, ChangedFile, BRANCHES_OP, FILE_OP, STAT_OP};
use super::html_preview;
use crate::acting::Request;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Target {
    Own,
    Worktree(String),
    Ref(String),
}

fn aim(args: &mut Value, target: &Target) {
    match target {
        Target::Own => {}
        Target::Worktree(path) => args["worktree"] = json!(path),
        Target::Ref(name) => args["ref"] = json!(name),
    }
}

pub fn stat_request(cwd: &str, target: &Target, base: Option<&str>) -> Request {
    let mut args = json!({ "cwd": cwd });
    aim(&mut args, target);
    match base {
        Some(base) => args["base"] = json!(base),
        None => args["pick"] = json!("nearest"),
    }
    Request { op: STAT_OP, args }
}

pub fn file_request(
    cwd: &str,
    target: &Target,
    base: &str,
    path: &str,
    from: Option<&str>,
) -> Request {
    let mut args = json!({ "cwd": cwd, "base": base, "path": path });
    aim(&mut args, target);
    if let Some(from) = from {
        args["from"] = json!(from);
    }
    Request { op: FILE_OP, args }
}

pub fn branches_request(cwd: &str, target: &Target) -> Request {
    let mut args = json!({ "cwd": cwd });
    if let Target::Worktree(path) = target {
        args["worktree"] = json!(path);
    }
    Request {
        op: BRANCHES_OP,
        args,
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TargetState {
    pub target: Target,
    pub branch: Option<Arc<BranchModel>>,
    pub base_choice: Option<String>,
    pub refused: Option<String>,
}

impl TargetState {
    pub fn new(target: Target) -> TargetState {
        TargetState {
            target,
            branch: None,
            base_choice: None,
            refused: None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Applied {
    Ignored,
    Same,
    Changed,
    Retry,
}

pub fn shown_branch<'a>(
    own: Option<&'a Arc<BranchModel>>,
    other: Option<&'a TargetState>,
) -> Option<&'a Arc<BranchModel>> {
    match other {
        Some(state) => state.branch.as_ref(),
        None => own,
    }
}

pub fn shown_target(other: Option<&TargetState>) -> Target {
    other.map_or(Target::Own, |state| state.target.clone())
}

pub fn is_ref(other: Option<&TargetState>) -> bool {
    other.is_some_and(|state| matches!(state.target, Target::Ref(_)))
}

pub fn asking(other: Option<&TargetState>) -> Option<(Target, Option<String>)> {
    other
        .filter(|state| state.refused.is_none())
        .map(|state| (state.target.clone(), state.base_choice.clone()))
}

pub fn choose_base(state: &mut TargetState, base: String) -> bool {
    if state.base_choice.as_deref() == Some(base.as_str()) {
        return false;
    }
    state.base_choice = Some(base);
    true
}

pub fn apply_stat(
    other: &mut Option<TargetState>,
    asked: &Target,
    base: Option<&str>,
    result: Result<Value, String>,
) -> Applied {
    let Some(state) = other
        .as_mut()
        .filter(|s| &s.target == asked && s.refused.is_none())
    else {
        return Applied::Ignored;
    };
    if base != state.base_choice.as_deref() {
        return Applied::Retry;
    }
    match result {
        Ok(reply) => {
            let wanted = branch::parse_stat(&reply).map(Arc::new);
            if state.branch == wanted {
                Applied::Same
            } else {
                state.branch = wanted;
                Applied::Changed
            }
        }
        Err(_) if base.is_some() => {
            state.base_choice = None;
            Applied::Retry
        }
        Err(why) => {
            state.branch = None;
            state.refused = Some(why);
            Applied::Changed
        }
    }
}

pub fn on_close(other: &mut Option<TargetState>, from_branches: &mut bool) -> bool {
    *other = None;
    std::mem::take(from_branches)
}

pub fn on_folder_change(other: &mut Option<TargetState>, from_branches: &mut bool) {
    *other = None;
    *from_branches = false;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiffVia {
    pub target: Target,
    pub base: String,
    pub from: Option<String>,
}

impl DiffVia {
    pub fn request(&self, cwd: &str, path: &str) -> Request {
        file_request(cwd, &self.target, &self.base, path, self.from.as_deref())
    }
}

pub fn offers_page(target: &Target, file: &ChangedFile) -> bool {
    *target == Target::Own
        && file.uncommitted
        && file.is_new()
        && html_preview::is_html(Path::new(&file.path))
}

#[cfg(test)]
#[path = "changes_target_tests.rs"]
mod tests;
