use serde_json::{json, Value};

use super::branch::{BRANCHES_OP, FILE_OP, STAT_OP};
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

#[cfg(test)]
#[path = "changes_target_tests.rs"]
mod tests;
