use super::agy_pick;
use super::options::Tool as RunTool;
use super::role_models::{catalog_name, codex_hints, hints_of, resolve, Live};
use super::role_view;
use crate::catalog::roles::Roles;
use crate::catalog::{Catalog, LiveModel, Tool};
use crate::tools::codex::models as codex_models;
use serde_json::{json, Map, Value};
use std::path::Path;

pub const PRICE_HELP: &str = "Notchling never invents a price. To add one, put it in the override file (Settings > Skills & Models > Override), per tool: {\"codex\": {\"prices\": [{\"id\": \"<model>\", \"input\": <USD per 1M tokens>, \"output\": <USD per 1M>, \"cache_read\": <USD per 1M>}]}}. Prices are read when the daemon starts.";

const KEPT: [&str; 9] = [
    "slug",
    "display_name",
    "description",
    "default_reasoning_level",
    "supported_reasoning_levels",
    "visibility",
    "priority",
    "context_window",
    "upgrade",
];

pub fn agy_live(listing: &str, before: &[LiveModel]) -> Vec<LiveModel> {
    agy_pick::entries(listing)
        .into_iter()
        .map(|e| {
            let whole = e.levels.len() == 1;
            let id = if whole {
                e.whole.clone()
            } else {
                e.model.clone()
            };
            let efforts = if whole { Vec::new() } else { e.levels.clone() };
            let default_effort = before
                .iter()
                .find(|b| b.id == id)
                .and_then(|b| b.default_effort.clone())
                .filter(|d| efforts.contains(d));
            LiveModel {
                name: e.name.clone().or_else(|| Some(id.clone())),
                id,
                efforts,
                default_effort,
                ..LiveModel::default()
            }
        })
        .collect()
}

pub fn codex_live(raw: &[u8]) -> Vec<LiveModel> {
    codex_models::parse(raw)
        .into_iter()
        .map(|m| LiveModel {
            id: m.slug,
            name: Some(m.name),
            context_window: m.context_window,
            efforts: m.efforts,
            default_effort: m.default_effort,
        })
        .collect()
}

pub fn codex_snapshot(raw: &[u8]) -> Option<String> {
    let root = serde_json::from_slice::<Value>(raw).ok()?;
    let models: Vec<Value> = root
        .get("models")?
        .as_array()?
        .iter()
        .filter(|m| m.get("slug").and_then(Value::as_str).is_some())
        .map(|m| {
            let mut kept = Map::new();
            for key in KEPT {
                if let Some(v) = m.get(key) {
                    kept.insert(key.to_string(), v.clone());
                }
            }
            Value::Object(kept)
        })
        .collect();
    if models.is_empty() {
        return None;
    }
    Some(json!({ "models": models }).to_string())
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct EffortChange {
    pub model: String,
    pub added: Vec<String>,
    pub removed: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Diff {
    pub added: Vec<String>,
    pub removed: Vec<String>,
    pub efforts: Vec<EffortChange>,
}

impl Diff {
    pub fn is_empty(&self) -> bool {
        self.added.is_empty() && self.removed.is_empty() && self.efforts.is_empty()
    }
}

pub fn diff(before: &[LiveModel], after: &[LiveModel]) -> Diff {
    let mut out = Diff::default();
    for model in after {
        match before.iter().find(|b| b.id == model.id) {
            None => out.added.push(model.id.clone()),
            Some(old) => {
                let added: Vec<String> = model
                    .efforts
                    .iter()
                    .filter(|e| !old.efforts.contains(e))
                    .cloned()
                    .collect();
                let removed: Vec<String> = old
                    .efforts
                    .iter()
                    .filter(|e| !model.efforts.contains(e))
                    .cloned()
                    .collect();
                if !added.is_empty() || !removed.is_empty() {
                    out.efforts.push(EffortChange {
                        model: model.id.clone(),
                        added,
                        removed,
                    });
                }
            }
        }
    }
    for old in before {
        if !after.iter().any(|m| m.id == old.id) {
            out.removed.push(old.id.clone());
        }
    }
    out
}

pub fn unpriced(catalog: &Catalog, tool: Tool, live: &[LiveModel]) -> Vec<String> {
    catalog
        .models(tool, live)
        .into_iter()
        .filter(|m| m.price.is_none())
        .map(|m| m.id)
        .collect()
}

pub fn suggestions(roles: &Roles, codex: &Live, agy: &Live) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut add = |line: String| {
        if !out.contains(&line) {
            out.push(line);
        }
    };
    let built_in = Roles::built_in();
    for view in role_view::views(roles, &built_in, codex, agy) {
        for choice in &view.choices {
            if choice.model.as_deref() != Some(choice.marked.as_str()) {
                let line = match (&choice.model, &choice.note) {
                    (Some(m), Some(note)) => format!(
                        "{}: {} {} at {}: {note}; consider marking {m}",
                        view.role, choice.tool, choice.marked, choice.effort
                    ),
                    (None, Some(note)) => {
                        format!("{}: {} {}: {note}", view.role, choice.tool, choice.marked)
                    }
                    _ => format!(
                        "{}: {} {} is gone and nothing in its family is listed",
                        view.role, choice.tool, choice.marked
                    ),
                };
                add(line);
            } else if let Some(note) = &choice.note {
                add(format!(
                    "{}: {} {} at {}: {note}",
                    view.role, choice.tool, choice.marked, choice.effort
                ));
            }
            if choice.tool == "antigravity"
                && choice.checked
                && !agy_pick::offers(&choice.marked, &choice.effort, &agy.efforts)
            {
                add(format!(
                    "{}: antigravity {} no longer offers effort {}",
                    view.role, choice.marked, choice.effort
                ));
            }
        }
    }
    for tool in [RunTool::Codex, RunTool::Agy] {
        let name = catalog_name(tool);
        let live = if tool == RunTool::Codex { codex } else { agy };
        if live.models.is_empty() {
            continue;
        }
        for pick in &roles.tool(name).picks {
            for ladder in &pick.then {
                for step in &ladder.steps {
                    let (step_tool, step_live) = match step.tool.as_str() {
                        "codex" => (RunTool::Codex, codex),
                        _ => (RunTool::Agy, agy),
                    };
                    if step_live.models.is_empty() {
                        continue;
                    }
                    let (model, note) = resolve(
                        &step.model,
                        &step.effort,
                        &ladder.role,
                        roles.tool(catalog_name(step_tool)),
                        step_live,
                    );
                    if model.as_deref() != Some(step.model.as_str()) || note.is_some() {
                        add(format!(
                            "{}: fallback {} {}: {}",
                            ladder.role,
                            catalog_name(step_tool),
                            step.model,
                            note.unwrap_or_else(|| "is gone".into())
                        ));
                    }
                }
            }
        }
    }
    out
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ToolReport {
    pub tool: String,
    pub source: String,
    pub read: bool,
    pub note: Option<String>,
    pub total: usize,
    pub diff: Diff,
    pub unpriced: Vec<String>,
    pub written: bool,
}

impl ToolReport {
    pub fn title(&self) -> &str {
        match self.tool.as_str() {
            "claude" => "Claude Code",
            "codex" => "Codex",
            _ => "Antigravity",
        }
    }

    fn to_json(&self) -> Value {
        json!({
            "tool": self.tool,
            "source": self.source,
            "read": self.read,
            "note": self.note,
            "total": self.total,
            "added": self.diff.added,
            "removed": self.diff.removed,
            "efforts": self.diff.efforts.iter().map(|c| json!({
                "model": c.model, "added": c.added, "removed": c.removed,
            })).collect::<Vec<_>>(),
            "unpriced": self.unpriced,
            "written": self.written,
        })
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Report {
    pub applied: bool,
    pub tools: Vec<ToolReport>,
    pub suggestions: Vec<String>,
    pub skills: Vec<String>,
}

impl Report {
    pub fn unpriced_count(&self) -> usize {
        self.tools.iter().map(|t| t.unpriced.len()).sum()
    }

    pub fn changed(&self) -> bool {
        self.tools.iter().any(|t| !t.diff.is_empty())
    }

    pub fn to_json(&self) -> Value {
        json!({
            "applied": self.applied,
            "changed": self.changed(),
            "tools": self.tools.iter().map(ToolReport::to_json).collect::<Vec<_>>(),
            "suggestions": self.suggestions,
            "skills": self.skills,
            "priceHelp": PRICE_HELP,
            "text": self.render(),
        })
    }

    pub fn render(&self) -> String {
        let mut lines: Vec<String> = Vec::new();
        for t in &self.tools {
            let state = if t.read {
                format!("{} models", t.total)
            } else {
                "not read".to_string()
            };
            lines.push(format!("{} ({}): {state}", t.title(), t.source));
            if let Some(note) = &t.note {
                lines.push(format!("  note: {note}"));
            }
            if t.read && t.diff.is_empty() {
                lines.push("  no change".into());
            }
            if !t.diff.added.is_empty() {
                lines.push(format!("  new: {}", t.diff.added.join(", ")));
            }
            if !t.diff.removed.is_empty() {
                lines.push(format!("  gone: {}", t.diff.removed.join(", ")));
            }
            for c in &t.diff.efforts {
                let mut parts: Vec<String> = c.added.iter().map(|e| format!("+{e}")).collect();
                parts.extend(c.removed.iter().map(|e| format!("-{e}")));
                lines.push(format!("  efforts {}: {}", c.model, parts.join(" ")));
            }
            if !t.unpriced.is_empty() {
                lines.push(format!("  no price: {}", t.unpriced.join(", ")));
            }
        }
        if self.unpriced_count() > 0 {
            lines.push(format!("Prices: {PRICE_HELP}"));
        }
        if self.suggestions.is_empty() {
            lines.push("Role picks: all still current.".into());
        } else {
            lines.push("Role picks to review (nothing was changed):".into());
            lines.extend(self.suggestions.iter().map(|s| format!("  {s}")));
        }
        if !self.skills.is_empty() {
            lines.push("Skills:".into());
            lines.extend(self.skills.iter().map(|s| format!("  {s}")));
        }
        if !self.applied {
            lines.push("This was a check: nothing was written. `notchling models update` writes the lists.".into());
        }
        lines.join("\n")
    }
}

pub fn codex_live_state(codex_home: &Path, raw: Option<&[u8]>) -> Live {
    match raw {
        Some(raw) => Live {
            models: codex_models::parse(raw)
                .into_iter()
                .map(|m| m.slug)
                .collect(),
            hints: hints_of(raw),
            efforts: Vec::new(),
        },
        None => Live {
            models: codex_models::load(codex_home)
                .into_iter()
                .map(|m| m.slug)
                .collect(),
            hints: codex_hints(codex_home),
            efforts: Vec::new(),
        },
    }
}

pub fn agy_live_state(listing: Option<&str>, cached: &[LiveModel]) -> Live {
    match listing {
        Some(listing) => {
            let listed = agy_pick::models(listing);
            Live {
                models: listed.iter().map(|(m, _)| m.clone()).collect(),
                hints: Vec::new(),
                efforts: listed,
            }
        }
        None => Live {
            models: cached.iter().map(|m| m.id.clone()).collect(),
            hints: Vec::new(),
            efforts: cached
                .iter()
                .map(|m| (m.id.clone(), m.efforts.clone()))
                .collect(),
        },
    }
}

#[cfg(test)]
#[path = "models_update_tests.rs"]
mod tests;
