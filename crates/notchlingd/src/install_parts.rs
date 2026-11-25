use super::{Direction, InstallOps, Plan, Request};
use notchling_engine::cost_profile::diff::counts;
use notchling_engine::install::{self, Action, Only, SkillResult, State, Want};
use notchling_local_api::{ops, ApiError};
use serde_json::{json, Value};
use std::path::Path;

pub const HOOKS: &str = "hooks";
pub const EXPECT_AFTER: &str = "expectAfter";
pub const KEEP_GUARD: &str = "keepGuard";

pub struct AgentPlan {
    pub parts: Vec<Value>,
    pub removes: bool,
    pub blocked: Option<String>,
}

pub(crate) fn state(id: &str, state: &str, error: Option<String>) -> Value {
    json!({"id": id, "state": state, "error": error})
}

pub(crate) fn skill_state(r: &SkillResult) -> Value {
    let id = install::part_id(r.plan.tool, &r.plan.name);
    match (&r.error, r.plan.action) {
        (Some(e), _) if e == install::STALE => state(&id, "stale", None),
        (Some(e), _) => state(&id, "failed", Some(e.clone())),
        (None, Action::Write | Action::Update | Action::Remove) => state(&id, "written", None),
        (None, _) => state(&id, "unchanged", None),
    }
}

pub(crate) fn only_of(args: &Value) -> Result<Option<Only>, ApiError> {
    let bad = || ApiError::bad_request("parts is {part id: hash of the file it was planned from}");
    match args.get("parts") {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Object(map)) => map
            .iter()
            .map(|(id, hash)| match hash {
                Value::Null => Ok((id.clone(), None)),
                Value::String(h) => Ok((id.clone(), Some(h.clone()))),
                _ => Err(bad()),
            })
            .collect::<Result<Only, ApiError>>()
            .map(Some),
        Some(_) => Err(bad()),
    }
}

pub(crate) fn after_of(args: &Value) -> Result<Option<Only>, ApiError> {
    let bad =
        || ApiError::bad_request("expectAfter is {part id: hash of the file it was shown as}");
    match args.get(EXPECT_AFTER) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Object(map)) => map
            .iter()
            .map(|(id, hash)| match hash {
                Value::Null => Ok((id.clone(), None)),
                Value::String(h) => Ok((id.clone(), Some(h.clone()))),
                _ => Err(bad()),
            })
            .collect::<Result<Only, ApiError>>()
            .map(Some),
        Some(_) => Err(bad()),
    }
}

pub(crate) fn keep_guard(args: &Value) -> bool {
    args.get(KEEP_GUARD).and_then(Value::as_bool) == Some(true)
}

pub(crate) fn drifted(
    only: Option<Only>,
    shown: Option<&Only>,
    fresh: &[Value],
) -> (Option<Only>, Vec<String>) {
    let Some(mut only) = only else {
        return (None, Vec::new());
    };
    let Some(shown) = shown else {
        return (Some(only), Vec::new());
    };
    let mut moved = Vec::new();
    for id in only.keys().cloned().collect::<Vec<_>>() {
        let now = fresh.iter().find(|p| p["id"] == json!(id));
        let now = now.map(|p| p["after"].as_str().map(str::to_string));
        if shown.get(&id) != now.as_ref() {
            only.remove(&id);
            moved.push(id);
        }
    }
    (Some(only), moved)
}

fn part(
    id: &str,
    path: &Path,
    tool: &str,
    before: Option<&str>,
    after: Option<&str>,
    notes: &[String],
) -> Value {
    let (added, removed) = counts(before.unwrap_or(""), after.unwrap_or(""));
    let change = match (before, after) {
        (None, _) => "create",
        (_, None) => "remove",
        _ => "update",
    };
    json!({
        "id": id,
        "path": path.to_string_lossy(),
        "tool": tool,
        "change": change,
        "added": added,
        "removed": removed,
        "before": before.map(install::text_hash),
        "after": after.map(install::text_hash),
        "notes": notes,
    })
}

impl InstallOps {
    pub(super) fn parts_of(&self, plan: &Plan) -> Vec<Value> {
        let mut out = Vec::new();
        if let Ok(file) = &plan.hooks {
            if file.hooks.changed {
                let before = install::load(&self.setup.settings)
                    .ok()
                    .filter(|l| l.state != State::Missing)
                    .map(|l| l.text);
                out.push(part(
                    HOOKS,
                    &self.setup.settings,
                    "claude",
                    before.as_deref(),
                    Some(&file.hooks.text),
                    &file.hooks.changes,
                ));
            }
        }
        for skill in &plan.skills {
            if !matches!(
                skill.action,
                Action::Write | Action::Update | Action::Remove
            ) {
                continue;
            }
            let Some(spec) = self.setup.skills.iter().find(|s| s.name == skill.name) else {
                continue;
            };
            let (before, after) = install::skill_texts(spec, skill);
            let after = after.filter(|_| skill.action != Action::Remove);
            let note = format!("{} {}", skill.action.as_str(), skill.name);
            out.push(part(
                &install::part_id(skill.tool, &skill.name),
                &skill.dir.join("SKILL.md"),
                install::skills::tool_name(skill.tool),
                before.as_deref(),
                after.as_deref(),
                &[note],
            ));
        }
        out
    }

    pub(crate) fn agent_plan(&self, what: &Value) -> Result<AgentPlan, ApiError> {
        let args = json!({"what": what});
        let mut request: Request = self.request(ops::INSTALL_APPLY, &args)?;
        let mut removes = false;
        if request.guard == Want::Remove {
            let probe = Request {
                direction: Direction::Install,
                prompt: Want::Leave,
                pages: Want::Leave,
                guard: Want::Remove,
                skills: Vec::new(),
                with_swift_hooks: true,
            };
            removes = self
                .plan(&probe)?
                .hooks
                .as_ref()
                .is_ok_and(|file| file.hooks.changed);
            request.guard = Want::Leave;
        }
        let plan = self.plan(&request)?;
        if let Err(e) = &plan.hooks {
            return Err(ApiError::new(
                notchling_local_api::ErrorCode::Failed,
                e.to_string(),
            ));
        }
        Ok(AgentPlan {
            parts: self.parts_of(&plan),
            removes,
            blocked: plan.blocked.clone(),
        })
    }
}
