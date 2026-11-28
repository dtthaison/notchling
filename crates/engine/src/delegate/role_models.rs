use super::agy_pick;
use super::deps::Deps;
use super::family::{self, Newest};
use super::options::Tool;
use super::quota::{self, Verdict};
use crate::catalog::roles::{covers, Pick, Roles, ToolRoles};
use crate::catalog::Tool as CatalogTool;
use crate::cost_profile::{Profile, Role as ProfileRole};
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

pub struct Live {
    pub models: Vec<String>,
    pub hints: Vec<(String, String)>,
    pub efforts: Vec<(String, Vec<String>)>,
}

pub fn codex_hints(codex_home: &Path) -> Vec<(String, String)> {
    hints_in(&codex_home.join("models_cache.json"))
}

pub fn codex_hints_with(codex_home: &Path, snapshot: Option<&Path>) -> Vec<(String, String)> {
    hints_in(&models::source_file(codex_home, snapshot))
}

fn hints_in(path: &Path) -> Vec<(String, String)> {
    use std::io::Read;
    let Ok(file) = std::fs::File::open(path) else {
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
        Tool::Codex => {
            let snapshot = deps.codex_snapshot();
            Live {
                models: models::load_with(&deps.codex_home, snapshot.as_deref())
                    .into_iter()
                    .map(|m| m.slug)
                    .collect(),
                hints: codex_hints_with(&deps.codex_home, snapshot.as_deref()),
                efforts: Vec::new(),
            }
        }
        Tool::Agy => {
            let listed = deps
                .agy_binary
                .as_deref()
                .and_then(|agy| (deps.capture)(Path::new(agy), &["models"]))
                .map(|l| agy_pick::models(&l))
                .unwrap_or_default();
            Live {
                models: listed.iter().map(|(m, _)| m.clone()).collect(),
                hints: Vec::new(),
                efforts: listed,
            }
        }
    }
}

pub fn resolve(
    marked: &str,
    effort: &str,
    role: &str,
    roles: &ToolRoles,
    live: &Live,
) -> (Option<String>, Option<String>) {
    let here = |m: &str| {
        live.models
            .iter()
            .any(|x| x == m || agy_pick::is_single_level_of(x, m))
    };
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

struct Cand {
    tool: Tool,
    model: String,
    effort: String,
    note: Option<String>,
    verdict: Verdict,
    stand_in: bool,
}

fn family_of(model: &str) -> &str {
    model.split('-').next().unwrap_or(model)
}

#[derive(Default)]
struct Lives {
    codex: Option<Live>,
    agy: Option<Live>,
}

impl Lives {
    fn of(&mut self, tool: Tool, deps: &Deps) -> &Live {
        let slot = match tool {
            Tool::Codex => &mut self.codex,
            Tool::Agy => &mut self.agy,
        };
        slot.get_or_insert_with(|| live_of(tool, deps))
    }
}

struct Taken {
    picks: Vec<(Tool, String, bool)>,
}

impl Taken {
    fn family_free(&self, c: &Cand) -> bool {
        !self
            .picks
            .iter()
            .any(|(_, m, _)| family_of(m) == family_of(&c.model))
    }

    fn holds(&self, tool: Tool, model: &str) -> bool {
        self.picks.iter().any(|(t, m, _)| *t == tool && m == model)
    }

    fn stands_in(&self, tool: Tool, model: &str) -> bool {
        self.picks
            .iter()
            .any(|(t, m, stand_in)| *stand_in && *t == tool && m == model)
    }

    fn model_free(&self, c: &Cand) -> bool {
        !self.holds(c.tool, &c.model)
    }
}

fn ladder_of(
    role: &str,
    head: &Pick,
    head_tool: Tool,
    roles: &Roles,
) -> Vec<(Tool, String, String)> {
    let mut steps = vec![(head_tool, head.model.clone(), head.effort.clone())];
    for tool in [Tool::Codex, Tool::Agy] {
        for p in roles.tool(catalog_name(tool)).users_picks_for(role) {
            let listed = steps.iter().any(|(t, m, _)| *t == tool && *m == p.model);
            if !listed {
                steps.push((tool, p.model.clone(), p.effort.clone()));
            }
        }
    }
    for s in head.ladder(role) {
        if let Some(tool) = Tool::from_name(&s.tool) {
            steps.push((tool, s.model.clone(), s.effort.clone()));
        }
    }
    let mut tools: Vec<Tool> = Vec::new();
    for (tool, _, _) in &steps {
        if !tools.contains(tool) {
            tools.push(*tool);
        }
    }
    for tool in tools {
        for p in &roles.tool(catalog_name(tool)).picks {
            let capable = p.roles.iter().any(|r| r != role && covers(r, role));
            let listed = steps.iter().any(|(t, m, _)| *t == tool && *m == p.model);
            if capable && !listed {
                steps.push((tool, p.model.clone(), p.effort.clone()));
            }
        }
    }
    steps
}

fn choose(
    role: &str,
    steps: Vec<(Tool, String, String)>,
    roles: &Roles,
    deps: &Deps,
    lives: &mut Lives,
    taken: &Taken,
) -> Result<(Cand, Vec<String>), Option<Vec<String>>> {
    let source = deps.quota.as_ref();
    let now = source.map(|s| s.now()).unwrap_or_default();
    let mut tried: Vec<Cand> = Vec::new();
    let mut own_held = false;
    for (at, (tool, marked, effort)) in steps.into_iter().enumerate() {
        let live = lives.of(tool, deps);
        let tool_roles = roles.tool(catalog_name(tool));
        let (model, note) = resolve(&marked, &effort, role, tool_roles, live);
        let Some(model) = model else {
            continue;
        };
        if at == 0 && taken.stands_in(tool, &model) {
            return Err(None);
        }
        if at == 0 && taken.holds(tool, &model) {
            own_held = true;
            continue;
        }
        let stand_in = at > 0
            && tool_roles
                .users_picks_for(role)
                .iter()
                .any(|p| p.model == marked);
        if tool == Tool::Agy
            && (!agy_pick::accepted(&model, Some(&effort), &live.models)
                || !agy_pick::offers(&model, &effort, &live.efforts))
        {
            continue;
        }
        let quota = source.and_then(|s| s.load(tool));
        let verdict = quota::verdict(quota.as_ref(), tool, &model, now);
        let cand = Cand {
            tool,
            model,
            effort,
            note,
            verdict,
            stand_in,
        };
        if cand.verdict == Verdict::Room && taken.family_free(&cand) {
            return Ok(finish(tried, cand, now));
        }
        tried.push(cand);
    }
    let at = tried
        .iter()
        .position(|c| c.verdict == Verdict::Room && taken.model_free(c));
    match at {
        Some(i) => {
            let cand = tried.remove(i);
            tried.truncate(i);
            Ok(finish(tried, cand, now))
        }
        None if taken.picks.is_empty() && !tried.is_empty() => {
            let notes = skipped(&tried, now);
            let first = tried.swap_remove(0);
            Ok((first, notes))
        }
        None if tried.is_empty() && !own_held => Err(None),
        None => Err(Some(skipped(&tried, now))),
    }
}

fn skipped(tried: &[Cand], now: f64) -> Vec<String> {
    tried
        .iter()
        .filter_map(|c| match &c.verdict {
            Verdict::Room => None,
            Verdict::Out(out) => Some(quota::note(&c.model, out, now)),
            Verdict::SignedOut => Some(format!("{}: {} is signed out", c.model, c.tool.raw())),
        })
        .collect()
}

fn finish(before: Vec<Cand>, chosen: Cand, now: f64) -> (Cand, Vec<String>) {
    let mut notes = skipped(&before, now);
    notes.extend(chosen.note.clone());
    (chosen, notes)
}

fn profile_steps(role: &str, deps: &Deps) -> Vec<(Tool, String, String)> {
    let Some(picked @ (ProfileRole::Code | ProfileRole::Fix)) = ProfileRole::parse(role) else {
        return Vec::new();
    };
    let Some(file) = deps.settings_file() else {
        return Vec::new();
    };
    let profile = Profile::load(&file);
    let mut steps = Vec::new();
    for (cat_tool, run_tool) in [
        (CatalogTool::Antigravity, Tool::Agy),
        (CatalogTool::Codex, Tool::Codex),
    ] {
        if !profile.is_on(cat_tool) {
            continue;
        }
        let Some(cell) = profile.picks.get(&(picked, cat_tool)) else {
            continue;
        };
        let Some(model) = &cell.model else {
            continue;
        };
        steps.push((run_tool, model.clone(), cell.effort.clone()));
    }
    steps
}

pub fn find(role: &str, roles: &Roles, deps: &Deps) -> Found {
    let role = role.to_lowercase();
    let mut found = Found {
        picks: Vec::new(),
        notes: Vec::new(),
        error: None,
    };
    let mut marked = false;
    let mut seats = 0;
    let mut short = false;
    let mut lives = Lives::default();
    let mut taken = Taken { picks: Vec::new() };
    let custom = profile_steps(&role, deps);
    if !custom.is_empty() {
        marked = true;
        seats = 1;
        if let Ok((cand, notes)) = choose(&role, custom, roles, deps, &mut lives, &taken) {
            found
                .picks
                .push((cand.tool.raw().to_string(), cand.model, cand.effort));
            found.notes.extend(notes);
        }
    } else {
        for tool in [Tool::Codex, Tool::Agy] {
            let tool_roles = roles.tool(catalog_name(tool));
            let picks = tool_roles.picks_for(&role);
            if picks.is_empty() {
                continue;
            }
            marked = true;
            seats += picks.len();
            for pick in picks {
                let steps = ladder_of(&role, pick, tool, roles);
                let (cand, notes) = match choose(&role, steps, roles, deps, &mut lives, &taken) {
                    Ok(chosen) => chosen,
                    Err(Some(notes)) if !found.picks.is_empty() => {
                        short = true;
                        found.notes.extend(notes);
                        continue;
                    }
                    Err(_) => continue,
                };
                taken
                    .picks
                    .push((cand.tool, cand.model.clone(), cand.stand_in));
                let one = (cand.tool.raw().to_string(), cand.model, cand.effort);
                if !found.picks.contains(&one) {
                    found.picks.push(one);
                }
                found.notes.extend(notes);
            }
        }
    }
    if seats >= 2 && short && found.picks.len() == 1 {
        found.notes.push(format!(
            "no second model for {role}: no other model for it has room"
        ));
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

fn pick_line(tool: &str, model: &str, effort: &str) -> String {
    if effort.is_empty() {
        format!("{tool} --model {model}")
    } else {
        format!("{tool} --model {model} --effort {effort}")
    }
}

pub fn models_for(role: &str, json: bool, roles: &Roles, deps: &Deps, out: &mut dyn Write) -> i32 {
    let found = find(role, roles, deps);
    let code = i32::from(found.error.is_some());
    if json {
        let mut v = json!({
            "role": role.to_lowercase(),
            "picks": found.picks.iter().map(|(tool, model, effort)| {
                let mut one = json!({"tool": tool, "model": model});
                if !effort.is_empty() {
                    one["effort"] = effort.clone().into();
                }
                one
            }).collect::<Vec<_>>(),
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
        .map(|(tool, model, effort)| pick_line(tool, model, effort))
        .collect();
    lines.extend(found.notes.iter().map(|n| format!("note: {n}")));
    let _ = writeln!(out, "{}", lines.join("\n"));
    code
}

pub fn wants_for(args: &[String]) -> bool {
    args.iter().any(|a| a == "--for" || a.starts_with("--for="))
}

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

#[cfg(test)]
#[path = "role_models_quota_tests.rs"]
mod quota_tests;

#[cfg(test)]
#[path = "role_models_review_tests.rs"]
mod review_tests;
