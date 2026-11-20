use notchling_local_api::ops;
use serde_json::{json, Value};

use crate::acting::Request;

fn call(op: &'static str, args: Value) -> Request {
    Request { op, args }
}

pub fn profile() -> Request {
    call(ops::COST_PROFILE_GET, json!({}))
}

pub fn preset(name: &str) -> Request {
    call(
        ops::COST_PROFILE_SET,
        json!({ "preset": name, "reset": true }),
    )
}

pub fn pick(role: &str, tool: &str, text: &str) -> Request {
    call(
        ops::COST_PROFILE_SET,
        json!({ "picks": { format!("{role}.{tool}"): text } }),
    )
}

pub fn clear_pick(role: &str, tool: &str) -> Request {
    call(
        ops::COST_PROFILE_SET,
        json!({ "picks": { format!("{role}.{tool}"): Value::Null } }),
    )
}

pub fn plan(consent: &[String]) -> Request {
    call(ops::COST_SETUP_PLAN, json!({ "consent": consent }))
}

pub fn apply(consent: &[String]) -> Request {
    call(ops::COST_SETUP_APPLY, json!({ "consent": consent }))
}

pub fn undo(tool: &str) -> Request {
    call(ops::COST_SETUP_UNDO, json!({ "tool": tool }))
}

pub fn check() -> Request {
    call(ops::COST_SETUP_CHECK, json!({}))
}

pub fn resolve(id: &str, choice: &str) -> Request {
    call(
        ops::COST_SETUP_CHECK,
        json!({ "resolve": { "id": id, "choice": choice } }),
    )
}

pub fn catalog(tool: &str) -> Request {
    call(ops::CATALOG_GET, json!({ "tool": tool }))
}

pub fn versions() -> Request {
    call(ops::VERSIONS_GET, json!({}))
}
