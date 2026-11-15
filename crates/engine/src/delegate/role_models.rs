//! `delegate-models` with the catalog's roles: what each model is for, and `--for <role>`.

use super::agy_pick;
use super::deps::Deps;
use super::family::{self, Newest};
use super::options::Tool;
use crate::catalog::roles::{Roles, ToolRoles};
use crate::tools::codex::models;
use serde_json::{json, Value};
use std::io::Write;
use std::path::Path;

pub fn catalog_name(tool: Tool) -> &'static str {
    match tool {
        Tool::Codex => "codex",
        Tool::Agy => "antigravity",
    }
}

/// The text after a model's line: its marked roles with the effort, the use note, what it is not for.
pub fn suffix(roles: &ToolRoles, model: &str) -> String {
    let mut text = String::new();
    let picks = roles.picks_of(model);
    for pick in &picks {
        text.push_str(&format!("  [{} at {}]", pick.roles.join(", "), pick.effort));
    }
    let not_for = roles.not_for(model);
    if !not_for.is_empty() {
        text.push_str(&format!("  [not for {}]", not_for.join(", ")));
    }
    if let Some(note) = picks
        .iter()
        .map(|p| p.note.as_str())
        .find(|n| !n.is_empty())
    {
        text.push_str(&format!("\n    use: {note}"));
    }
    text
}

/// One line per effort a tool says no model should use for a role.
pub fn effort_lines(roles: &ToolRoles) -> Vec<String> {
    roles
        .not_for_efforts()
        .into_iter()
        .map(|(effort, roles)| format!("  not for {}: effort {effort}", roles.join(", ")))
        .collect()
}

/// The `roles` and `notFor` fields of one model's JSON entry.
pub fn json_fields(roles: &ToolRoles, model: &str, into: &mut Value) {
    let picks = roles.picks_of(model);
    if !picks.is_empty() {
        into["roles"] = picks
            .iter()
            .flat_map(|p| {
                p.roles
                    .iter()
                    .map(|r| json!({"role": r, "effort": p.effort}))
            })
            .collect();
        if let Some(note) = picks
            .iter()
            .map(|p| p.note.as_str())
            .find(|n| !n.is_empty())
        {
            into["use"] = note.into();
        }
    }
    let not_for = roles.not_for(model);
    if !not_for.is_empty() {
        into["notFor"] = not_for.into();
    }
}

pub fn efforts_json(roles: &ToolRoles) -> Value {
    roles
        .not_for_efforts()
        .into_iter()
        .map(|(effort, roles)| json!({"effort": effort, "notFor": roles}))
        .collect()
}

/// What a tool lists now, and the replacements it names itself (Codex's `upgrade`).
pub struct Live {
    pub models: Vec<String>,
    pub hints: Vec<(String, String)>,
}

/// `models_cache.json`'s `upgrade` on a model: a slug, or an object naming one.
pub fn codex_hints(codex_home: &Path) -> Vec<(String, String)> {
    use std::io::Read;
    let Ok(file) = std::fs::File::open(codex_home.join("models_cache.json")) else {
        return Vec::new();
    };
    let mut data = Vec::new();
    if file.take(4 * 1024 * 1024).read_to_end(&mut data).is_err() {
        return Vec::new();
    }
    hints_of(&data)
}

pub fn hints_of(data: &[u8]) -> Vec<(String, String)> {
    let Ok(root) = serde_json::from_slice::<Value>(data) else {
        return Vec::new();
    };
    let target = |u: &Value| match u {
        Value::String(s) => Some(s.clone()),
        Value::Object(o) => ["model", "id", "slug"]
            .iter()
            .find_map(|k| o.get(*k).and_then(Value::as_str).map(String::from)),
        _ => None,
    };
    root.get("models")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|m| {
            let slug = m.get("slug").and_then(Value::as_str)?;
            let to = target(m.get("upgrade")?).filter(|t| !t.is_empty() && t != slug)?;
            Some((slug.to_string(), to))
        })
        .collect()
}

fn live_of(tool: Tool, deps: &Deps) -> Live {
    match tool {
        Tool::Codex => Live {
            models: models::load(&deps.codex_home)
                .into_iter()
                .map(|m| m.slug)
                .collect(),
            hints: codex_hints(&deps.codex_home),
        },
        Tool::Agy => Live {
            models: deps
                .agy_binary
                .as_deref()
                .and_then(|agy| (deps.capture)(Path::new(agy), &["models"]))
                .map(|l| agy_pick::models(&l).into_iter().map(|(m, _)| m).collect())
                .unwrap_or_default(),
            hints: Vec::new(),
        },
    }
}

/// The model to use for one marked pick, and a note when it isn't the marked one or a newer one exists.
pub fn resolve(
    marked: &str,
    role: &str,
    roles: &ToolRoles,
    live: &Live,
) -> (Option<String>, Option<String>) {
    let here = |m: &str| live.models.iter().any(|x| x == m);
    let hint = live
        .hints
        .iter()
        .find(|(m, _)| m == marked)
        .map(|(_, to)| to.as_str())
        .filter(|to| here(to));
    let family = family::newest(marked, &live.models, |m| {
        roles.not_for(m).iter().any(|r| r == role)
    });
    if here(marked) {
        let note = match (hint, family) {
            (Some(to), _) => Some(format!("{to} replaces {marked} (the provider's hint)")),
            (None, Newest::One(id, true)) => Some(format!("newer in family: {id}")),
            _ => None,
        };
        return (Some(marked.to_string()), note);
    }
    if let Some(to) = hint {
        return (
            Some(to.to_string()),
            Some(format!(
                "{marked} is gone; using {to} (the provider's replacement)"
            )),
        );
    }
    match family {
        Newest::One(id, newer) => (
            Some(id.to_string()),
            Some(format!(
                "{marked} is gone; using {id} (same family{})",
                if newer { ", newer" } else { "" }
            )),
        ),
        Newest::Nothing => (None, None),
    }
}

/// `delegate-models --for <role>`: one line per marked model, or the one that replaces it.
pub fn models_for(role: &str, roles: &Roles, deps: &Deps, out: &mut dyn Write) -> i32 {
    let role = role.to_lowercase();
    let mut lines: Vec<String> = Vec::new();
    let mut notes: Vec<String> = Vec::new();
    let mut marked = false;
    for tool in [Tool::Codex, Tool::Agy] {
        let tool_roles = roles.tool(catalog_name(tool));
        let picks = tool_roles.picks_for(&role);
        if picks.is_empty() {
            continue;
        }
        marked = true;
        let live = live_of(tool, deps);
        for pick in picks {
            let (model, note) = resolve(&pick.model, &role, tool_roles, &live);
            if let Some(model) = model {
                let line = format!("{} --model {model} --effort {}", tool.raw(), pick.effort);
                if !lines.contains(&line) {
                    lines.push(line);
                }
            }
            notes.extend(note.map(|n| format!("note: {n}")));
        }
    }
    if !marked {
        let known = roles.all_roles().join(", ");
        let _ = writeln!(
            out,
            "no model marked for {role}; run delegate-models (roles in the catalog: {known})"
        );
        return 1;
    }
    if lines.is_empty() {
        let _ = writeln!(out, "no model for {role}; run delegate-models");
        return 1;
    }
    lines.extend(notes);
    let _ = writeln!(out, "{}", lines.join("\n"));
    0
}

/// Splits `--for <role>` out of the arguments.
pub fn split_for(args: &[String]) -> (Vec<String>, Option<Option<String>>) {
    let mut rest = Vec::new();
    let mut role = None;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        if a == "--for" {
            role = Some(it.next().cloned());
        } else {
            rest.push(a.clone());
        }
    }
    (rest, role)
}

pub fn run_for(role: Option<String>, deps: &Deps, out: &mut dyn Write) -> i32 {
    let roles = Roles::load(deps.catalog_file.as_deref());
    match role {
        Some(role) => models_for(&role, &roles, deps, out),
        None => {
            let known = roles.all_roles().join(", ");
            let _ = writeln!(out, "--for needs a role: {known}");
            2
        }
    }
}

#[cfg(test)]
#[path = "role_models_tests.rs"]
mod tests;
