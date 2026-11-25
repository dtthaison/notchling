use super::options::Tool;
use super::role_models::{catalog_name, resolve, Live};
use crate::catalog::roles::{Pick, Roles};
use serde_json::{json, Value};

pub const KNOWN_ROLES: [&str; 4] = ["review", "review-final", "review-secrets", "spec-check"];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    BuiltIn,
    Override,
    Live,
}

impl Source {
    pub fn as_str(self) -> &'static str {
        match self {
            Source::BuiltIn => "built_in",
            Source::Override => "override",
            Source::Live => "live",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Choice {
    pub tool: &'static str,
    pub marked: String,
    pub model: Option<String>,
    pub effort: String,
    pub source: Source,
    pub note: Option<String>,
    pub advice: Option<String>,
    pub checked: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RoleView {
    pub role: String,
    pub choices: Vec<Choice>,
}

fn choice(
    role: &str,
    tool: Tool,
    pick: &Pick,
    roles: &Roles,
    built_in: &Roles,
    live: &Live,
) -> Choice {
    let name = catalog_name(tool);
    let checked = !live.models.is_empty();
    let (model, note) = if checked {
        resolve(&pick.model, &pick.effort, role, roles.tool(name), live)
    } else if roles.tool(name).excluded(&pick.model, &pick.effort, role) {
        (None, Some(format!("{} is not for {role}", pick.model)))
    } else {
        (Some(pick.model.clone()), None)
    };
    let users = !built_in.tool(name).picks.contains(pick);
    let source = if users {
        Source::Override
    } else if model.as_ref().is_some_and(|m| *m != pick.model) {
        Source::Live
    } else {
        Source::BuiltIn
    };
    Choice {
        tool: name,
        marked: pick.model.clone(),
        model,
        effort: pick.effort.clone(),
        source,
        note,
        advice: Some(pick.note.clone()).filter(|n| !n.is_empty()),
        checked,
    }
}

pub fn views(roles: &Roles, built_in: &Roles, codex: &Live, agy: &Live) -> Vec<RoleView> {
    let mut names: Vec<String> = KNOWN_ROLES.iter().map(|r| (*r).to_owned()).collect();
    for extra in roles.all_roles() {
        if !names.contains(&extra) {
            names.push(extra);
        }
    }
    names
        .into_iter()
        .map(|role| {
            let mut choices = Vec::new();
            for (tool, live) in [(Tool::Codex, codex), (Tool::Agy, agy)] {
                for pick in roles.tool(catalog_name(tool)).picks_for(&role) {
                    choices.push(choice(&role, tool, pick, roles, built_in, live));
                }
            }
            RoleView { role, choices }
        })
        .collect()
}

pub fn to_json(views: &[RoleView]) -> Value {
    views
        .iter()
        .map(|v| {
            json!({
                "role": v.role,
                "choices": v.choices.iter().map(|c| json!({
                    "tool": c.tool,
                    "marked": c.marked,
                    "model": c.model,
                    "effort": c.effort,
                    "source": c.source.as_str(),
                    "note": c.note,
                    "advice": c.advice,
                    "checked": c.checked,
                })).collect::<Vec<_>>(),
            })
        })
        .collect()
}

#[cfg(test)]
#[path = "role_view_tests.rs"]
mod tests;
