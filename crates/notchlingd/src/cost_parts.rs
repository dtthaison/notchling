use super::{bad, CostProfileOps};
use notchling_engine::catalog::Tool;
use notchling_engine::cost_profile::diff::counts;
use notchling_engine::cost_profile::model::{tool_key, HABIT_KEYS};
use notchling_engine::cost_profile::{
    ActionResult, Only, Options, Outcome, Plan, Profile, Service, UndoResult,
};
use notchling_local_api::ApiError;
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;

pub const PROFILE: &str = "profile";

pub(crate) fn stored_hash(profile: &Profile) -> String {
    let stored = profile.to_stored();
    let text = json!({
        "preset": stored.preset,
        "picks": stored.picks,
        "tools": stored.tools,
        "app": stored.app,
    })
    .to_string();
    notchling_engine::install::text_hash(&text)
}

pub(crate) fn profile_hash(service: &Service) -> String {
    stored_hash(&service.profile())
}

fn facts(profile: &Profile) -> BTreeMap<String, Value> {
    let mut out = BTreeMap::new();
    out.insert("preset".to_string(), json!(profile.preset));
    for tool in Tool::ALL.iter().copied() {
        out.insert(
            format!("tool.{}", tool_key(tool)),
            json!(profile.is_on(tool)),
        );
    }
    for ((role, tool), cell) in &profile.picks {
        out.insert(
            format!("pick.{}.{}", role.name(), tool_key(*tool)),
            json!(cell.to_string()),
        );
    }
    let habits = profile.habits();
    for key in HABIT_KEYS {
        out.insert(format!("habit.{key}"), json!(habits.get(key)));
    }
    out
}

pub(crate) fn changed_fields(before: &Profile, after: &Profile) -> Vec<Value> {
    let (a, b) = (facts(before), facts(after));
    let mut keys: Vec<&String> = a.keys().chain(b.keys()).collect();
    keys.sort();
    keys.dedup();
    keys.into_iter()
        .filter(|k| a.get(*k) != b.get(*k))
        .map(|k| {
            json!({
                "field": k,
                "before": a.get(k).cloned().unwrap_or(Value::Null),
                "after": b.get(k).cloned().unwrap_or(Value::Null),
            })
        })
        .collect()
}

pub(crate) fn apply_states(only: &Only, results: &[ActionResult]) -> Vec<Value> {
    only.keys()
        .map(|id| {
            let found = results.iter().find(|r| &r.id == id);
            let state = match found {
                Some(r) if r.stale => "stale",
                Some(r) if r.done => "written",
                Some(_) => "failed",
                None => "unchanged",
            };
            json!({"id": id, "state": state, "error": found.and_then(|r| r.error.clone()).filter(|_| state == "failed")})
        })
        .collect()
}

pub(crate) fn undo_states(results: &[UndoResult]) -> Vec<Value> {
    results
        .iter()
        .map(|r| {
            let state = match (r.outcome, r.detail.as_deref()) {
                (Outcome::Conflict, Some("stale")) => "stale",
                (Outcome::Restored | Outcome::Removed | Outcome::Gone, _) => "written",
                _ => "failed",
            };
            json!({"id": r.id, "state": state, "error": r.detail.clone().filter(|_| state == "failed")})
        })
        .collect()
}

impl CostProfileOps {
    pub(crate) fn options_of(args: &Value) -> Result<Options, ApiError> {
        Self::consent(args)
    }

    pub(crate) fn apply_parts(&self, opts: &Options) -> Result<Vec<Value>, ApiError> {
        let plan = self.service.plan(opts).map_err(super::failed)?;
        Ok(parts_of(&plan))
    }
}

pub(crate) fn parts_of(plan: &Plan) -> Vec<Value> {
    plan.actions
        .iter()
        .filter(|a| a.kind.writes())
        .map(|a| {
            let (added, removed) = counts(
                a.before.as_deref().unwrap_or(""),
                a.after.as_deref().unwrap_or(""),
            );
            json!({
                "id": a.id,
                "path": a.path.to_string_lossy(),
                "tool": tool_key(a.tool),
                "change": a.kind.as_str(),
                "needs": a.needs,
                "added": added,
                "removed": removed,
                "before": a.before.as_deref().map(notchling_engine::install::text_hash),
                "after": a.after.as_deref().map(notchling_engine::install::text_hash),
                "notes": a.notes,
            })
        })
        .collect()
}

impl CostProfileOps {
    pub(crate) fn undo_part_list(&self, tool: Tool) -> Result<Vec<Value>, ApiError> {
        let list = self.service.undo_parts(tool).map_err(super::failed)?;
        Ok(list
            .iter()
            .map(|p| {
                json!({
                    "id": p.id,
                    "path": p.path.to_string_lossy(),
                    "tool": tool_key(tool),
                    "change": if p.removes { "remove" } else { "restore" },
                    "needs": Value::Null,
                    "added": 0,
                    "removed": 0,
                    "before": p.now,
                    "after": Value::Null,
                    "notes": [],
                })
            })
            .collect())
    }

    pub(crate) fn tool_of(args: &Value) -> Result<Tool, ApiError> {
        Self::tool_arg(args)
    }

    pub(crate) fn profile_part(&self, args: &Value) -> Result<Value, ApiError> {
        let Some(object) = args.as_object().filter(|o| !o.is_empty()) else {
            return Err(bad("Say what to change in the profile."));
        };
        let before = self.service.profile();
        let after = self.edited(&Value::Object(object.clone()))?;
        let fields = changed_fields(&before, &after);
        if fields.is_empty() {
            return Err(bad("That changes nothing in the profile.").with_reason("nothing_to_do"));
        }
        let mut part = Map::new();
        part.insert("id".into(), json!(PROFILE));
        part.insert(
            "path".into(),
            json!(self.service.settings_file().to_string_lossy()),
        );
        part.insert("tool".into(), json!("notchling"));
        part.insert("change".into(), json!("update"));
        part.insert("before".into(), json!(stored_hash(&before)));
        part.insert("after".into(), json!(stored_hash(&after)));
        part.insert("fields".into(), json!(fields));
        Ok(Value::Object(part))
    }
}
