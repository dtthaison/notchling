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

/// One line per entry that names an effort, with the model name it is limited to if any.
pub fn effort_lines(roles: &ToolRoles) -> Vec<String> {
    roles
        .not_for_efforts()
        .into_iter()
        .map(|(matching, effort, roles)| {
            let of = matching.map(|m| format!("{m} at ")).unwrap_or_default();
            format!("  not for {}: {of}effort {effort}", roles.join(", "))
        })
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
        .map(|(matching, effort, roles)| {
            let mut v = json!({"effort": effort, "notFor": roles});
            if let Some(m) = matching {
                v["match"] = m.into();
            }
            v
        })
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

/// The model to use for one marked pick, and a note when it isn't the marked one or a
/// newer one exists. A model or effort the catalog says is not for the role is never chosen.
pub fn resolve(
    marked: &str,
    effort: &str,
    role: &str,
    roles: &ToolRoles,
    live: &Live,
) -> (Option<String>, Option<String>) {
    let here = |m: &str| live.models.iter().any(|x| x == m);
    let excluded = |m: &str| roles.excluded(m, effort, role);
    let usable = |m: &str| here(m) && !excluded(m);
    let hint = live
        .hints
        .iter()
        .find(|(m, _)| m == marked)
        .map(|(_, to)| to.as_str())
        .filter(|to| usable(to));
    let family = family::newest(marked, &live.models, excluded);
    if usable(marked) {
        let note = match (hint, family) {
            (Some(to), _) => Some(format!("{to} replaces {marked} (the provider's hint)")),
            (None, Newest::One(id, true)) => Some(format!("newer in family: {id}")),
            _ => None,
        };
        return (Some(marked.to_string()), note);
    }
    let why = if here(marked) {
        format!("is not for {role}")
    } else {
        "is gone".to_string()
    };
    if let Some(to) = hint {
        return (
            Some(to.to_string()),
            Some(format!(
                "{marked} {why}; using {to} (the provider's replacement)"
            )),
        );
    }
    match family {
        Newest::One(id, newer) => (
            Some(id.to_string()),
            Some(format!(
                "{marked} {why}; using {id} (same family{})",
                if newer { ", newer" } else { "" }
            )),
        ),
        Newest::Nothing => (None, None),
    }
}

pub struct Found {
    pub picks: Vec<(String, String, String)>,
    pub notes: Vec<String>,
    pub error: Option<String>,
}

/// What `--for <role>` answers: a pick per marked model that has a usable model now.
pub fn find(role: &str, roles: &Roles, deps: &Deps) -> Found {
    let role = role.to_lowercase();
    let mut found = Found {
        picks: Vec::new(),
        notes: Vec::new(),
        error: None,
    };
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
            let (model, note) = resolve(&pick.model, &pick.effort, &role, tool_roles, &live);
            if let Some(model) = model {
                let one = (tool.raw().to_string(), model, pick.effort.clone());
                if !found.picks.contains(&one) {
                    found.picks.push(one);
                }
            }
            found.notes.extend(note);
        }
    }
    if !marked {
        let known = roles.all_roles().join(", ");
        found.error = Some(format!(
            "no model marked for {role}; run delegate-models (roles in the catalog: {known})"
        ));
    } else if found.picks.is_empty() {
        found.notes.clear();
        found.error = Some(format!("no model for {role}; run delegate-models"));
    }
    found
}

/// `delegate-models --for <role>` as text (one line per pick, then `note:` lines) or, with
/// `--json`, `{"role", "picks": [{"tool","model","effort"}], "notes": [], "error"?}`.
pub fn models_for(role: &str, json: bool, roles: &Roles, deps: &Deps, out: &mut dyn Write) -> i32 {
    let found = find(role, roles, deps);
    let code = i32::from(found.error.is_some());
    if json {
        let mut v = json!({
            "role": role.to_lowercase(),
            "picks": found.picks.iter().map(|(tool, model, effort)| json!({"tool": tool, "model": model, "effort": effort})).collect::<Vec<_>>(),
            "notes": found.notes,
        });
        if let Some(e) = &found.error {
            v["error"] = e.clone().into();
        }
        let _ = writeln!(
            out,
            "{}",
            serde_json::to_string_pretty(&v).unwrap_or_default()
        );
        return code;
    }
    if let Some(e) = found.error {
        let _ = writeln!(out, "{e}");
        return code;
    }
    let mut lines: Vec<String> = found
        .picks
        .iter()
        .map(|(tool, model, effort)| format!("{tool} --model {model} --effort {effort}"))
        .collect();
    lines.extend(found.notes.iter().map(|n| format!("note: {n}")));
    let _ = writeln!(out, "{}", lines.join("\n"));
    code
}

/// Whether the arguments ask for `--for`.
pub fn wants_for(args: &[String]) -> bool {
    args.iter().any(|a| a == "--for" || a.starts_with("--for="))
}

/// `--for <role>` or `--for=<role>`; nothing else but `--json` is accepted beside it.
pub fn run_for(args: &[String], json: bool, deps: &Deps, out: &mut dyn Write) -> i32 {
    let roles = Roles::load(deps.catalog_file.as_deref());
    let mut role: Option<String> = None;
    let mut json = json;
    let mut extra: Option<&String> = None;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        if a == "--for" {
            role = Some(it.next().cloned().unwrap_or_default());
        } else if let Some(r) = a.strip_prefix("--for=") {
            role = Some(r.to_string());
        } else if a == "--json" {
            json = true;
        } else {
            extra.get_or_insert(a);
        }
    }
    let role = role.map(|r| r.trim().to_string()).filter(|r| !r.is_empty());
    let Some(role) = role else {
        let _ = writeln!(out, "--for needs a role: {}", roles.all_roles().join(", "));
        return 2;
    };
    if let Some(extra) = extra {
        let _ = writeln!(out, "--for takes only a role, not {extra}");
        return 2;
    }
    models_for(&role, json, &roles, deps, out)
}

#[cfg(test)]
#[path = "role_models_tests.rs"]
mod tests;
