use super::manifest::KeyRec;
use super::model::Profile;
use super::model::Role;
use super::resolve::{resolve, Available};
use crate::catalog::Tool;
use crate::install::edit::{self, key as seg, Seg};
use crate::install::jsonc::{self, Node};
use crate::install::render::Json;
use crate::install::toml_edit;
use serde_json::Value;

pub trait Probe {
    fn accepts(&self, key: &str, raw: &str) -> bool;
}

pub struct Skipped {
    pub path: Vec<String>,
    pub why: &'static str,
}

pub struct KeyPlan {
    pub text: String,
    pub recs: Vec<KeyRec>,
    pub skipped: Vec<Skipped>,
}

pub struct Conflict {
    pub path: Vec<String>,
}

fn path_of(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|p| p.to_string()).collect()
}

fn lookup<'a>(root: &'a Node, path: &[String]) -> Option<&'a Node> {
    path.iter().try_fold(root, |node, key| node.get(key))
}

fn current(text: &str, path: &[String]) -> Result<Option<Value>, String> {
    let root = jsonc::parse(text).map_err(|e| e.to_string())?;
    Ok(lookup(&root, path).map(|n| jsonc::to_value(n, text)))
}

pub fn current_raw(text: &str, tool: Tool, path: &[String]) -> Option<String> {
    if tool == Tool::Codex {
        let key = path.get(1)?;
        return toml_edit::get_raw(text, Some(AGENTS), key).ok().flatten();
    }
    current(text, path).ok().flatten().map(|v| v.to_string())
}

fn nested(path: &[String], leaf: Json) -> Json {
    match path.split_first() {
        None => leaf,
        Some((key, rest)) => Json::Obj(vec![(key.clone(), nested(rest, leaf))]),
    }
}

fn set_path(text: &str, path: &[String], leaf: Json) -> Result<String, String> {
    let root = jsonc::parse(text).map_err(|e| e.to_string())?;
    let mut depth = 0;
    let mut node = &root;
    while depth + 1 < path.len() {
        match node.get(&path[depth]) {
            Some(child) if child.members().is_some() => {
                node = child;
                depth += 1;
            }
            Some(_) => return Err(format!("{} is not an object", path[depth])),
            None => break,
        }
    }
    let at: Vec<Seg> = path[..depth].iter().map(|k| seg(k)).collect();
    edit::set_member(text, &at, &path[depth], &nested(&path[depth + 1..], leaf))
        .map_err(|e| e.to_string())
}

fn remove_path(text: &str, path: &[String]) -> Result<String, String> {
    let mut text = text.to_string();
    let mut path = path.to_vec();
    while !path.is_empty() {
        let root = jsonc::parse(&text).map_err(|e| e.to_string())?;
        let Some(parent) = lookup(&root, &path[..path.len() - 1]) else {
            break;
        };
        let Some(members) = parent.members() else {
            break;
        };
        let Some(index) = members.iter().position(|m| m.key == path[path.len() - 1]) else {
            break;
        };
        let at: Vec<Seg> = path[..path.len() - 1].iter().map(|k| seg(k)).collect();
        text = edit::remove(&text, &at, index).map_err(|e| e.to_string())?;
        path.pop();
        let root = jsonc::parse(&text).map_err(|e| e.to_string())?;
        let empty = lookup(&root, &path)
            .and_then(Node::members)
            .is_some_and(|m| m.is_empty());
        if !empty || path.is_empty() {
            break;
        }
    }
    Ok(text)
}

fn json_of(value: &Value) -> Json {
    match value {
        Value::Number(n) if n.is_u64() => Json::Num(n.as_u64().unwrap_or(0)),
        Value::String(s) => Json::Str(s.clone()),
        other => Json::Raw(other.to_string()),
    }
}

fn wanted_claude(profile: &Profile, avail: &Available) -> Vec<(Vec<String>, u64)> {
    let habits = profile.habits();
    let mut out = vec![(
        path_of(&["bashOutputMaxChars"]),
        habits.bash_output_max_chars,
    )];
    for (family, tokens) in &habits.compact_window {
        if let Some(id) = avail.claude_id(family) {
            out.push((
                path_of(&["modelSettings", id, "autoCompactWindow"]),
                *tokens,
            ));
        }
    }
    out
}

pub fn claude_keys(
    text: &str,
    profile: &Profile,
    avail: &Available,
    prior: &[KeyRec],
) -> Result<KeyPlan, String> {
    let mut plan = KeyPlan {
        text: text.to_string(),
        recs: Vec::new(),
        skipped: Vec::new(),
    };
    for (path, want) in wanted_claude(profile, avail) {
        let now = current(&plan.text, &path)?;
        let want_value = Value::from(want);
        let before_rec = prior.iter().find(|r| r.path == path);
        if now.as_ref() == Some(&want_value) {
            if let Some(rec) = before_rec {
                plan.recs.push(rec.clone());
            }
            continue;
        }
        let ours = match (&now, before_rec) {
            (None, _) | (_, None) => true,
            (Some(v), Some(rec)) => {
                let shown = v.to_string();
                shown == rec.after
            }
        };
        if !ours {
            plan.skipped.push(Skipped {
                path,
                why: "changed-since",
            });
            continue;
        }
        plan.text = set_path(&plan.text, &path, Json::Num(want))?;
        plan.recs.push(KeyRec {
            before: before_rec
                .map(|r| r.before.clone())
                .unwrap_or_else(|| now.map(|v| v.to_string())),
            after: want_value.to_string(),
            path,
        });
    }
    Ok(plan)
}

pub fn undo_json_keys(text: &str, recs: &[KeyRec]) -> Result<(String, Vec<Conflict>), String> {
    let mut text = text.to_string();
    let mut conflicts = Vec::new();
    for rec in recs.iter().rev() {
        let now = current(&text, &rec.path)?;
        let ours = now.as_ref().map(Value::to_string).as_deref() == Some(rec.after.as_str());
        if now.is_none() {
            continue;
        }
        if !ours {
            conflicts.push(Conflict {
                path: rec.path.clone(),
            });
            continue;
        }
        text = match &rec.before {
            Some(raw) => {
                let value: Value = serde_json::from_str(raw).map_err(|e| e.to_string())?;
                set_path(&text, &rec.path, json_of(&value))?
            }
            None => remove_path(&text, &rec.path)?,
        };
    }
    Ok((text, conflicts))
}

const AGENTS: &str = "agents";
const ALIAS_OF_MAX_THREADS: &str = "max_concurrent_threads_per_session";

fn quoted(text: &str) -> String {
    serde_json::to_string(text).unwrap_or_else(|_| "\"\"".to_string())
}

fn wanted_codex(profile: &Profile, avail: &Available) -> Vec<(&'static str, String)> {
    let mut out = vec![("max_threads", profile.habits().parallel_cap.to_string())];
    if let Some(r) = resolve(profile, Role::Code, Tool::Codex, avail) {
        if let Some(model) = &r.model {
            out.push(("default_subagent_model", quoted(model)));
        }
        out.push(("default_subagent_reasoning_effort", quoted(&r.effort)));
    }
    out
}

pub fn codex_keys(
    text: &str,
    profile: &Profile,
    avail: &Available,
    probe: &dyn Probe,
    prior: &[KeyRec],
) -> Result<KeyPlan, String> {
    let mut plan = KeyPlan {
        text: text.to_string(),
        recs: Vec::new(),
        skipped: Vec::new(),
    };
    let toml = |e: toml_edit::TomlEditError| e.to_string();
    for (key, raw) in wanted_codex(profile, avail) {
        let path = path_of(&[AGENTS, key]);
        let now = toml_edit::get_raw(&plan.text, Some(AGENTS), key).map_err(toml)?;
        let before_rec = prior.iter().find(|r| r.path == path);
        if now.as_deref() == Some(raw.as_str()) {
            if let Some(rec) = before_rec {
                plan.recs.push(rec.clone());
            }
            continue;
        }
        if key == "max_threads"
            && toml_edit::get_raw(&plan.text, Some(AGENTS), ALIAS_OF_MAX_THREADS)
                .map_err(toml)?
                .is_some()
        {
            plan.skipped.push(Skipped {
                path,
                why: "alias-set",
            });
            continue;
        }
        let ours = match (&now, before_rec) {
            (None, _) | (_, None) => true,
            (Some(v), Some(rec)) => {
                let shown = v.to_string();
                shown == rec.after
            }
        };
        if !ours {
            plan.skipped.push(Skipped {
                path,
                why: "changed-since",
            });
            continue;
        }
        if !probe.accepts(&format!("{AGENTS}.{key}"), &raw) {
            plan.skipped.push(Skipped {
                path,
                why: "codex-rejects",
            });
            continue;
        }
        plan.text = toml_edit::set_raw(&plan.text, Some(AGENTS), key, &raw).map_err(toml)?;
        plan.recs.push(KeyRec {
            before: before_rec.map(|r| r.before.clone()).unwrap_or(now),
            after: raw,
            path,
        });
    }
    Ok(plan)
}

pub fn undo_toml_keys(text: &str, recs: &[KeyRec]) -> Result<(String, Vec<Conflict>), String> {
    let toml = |e: toml_edit::TomlEditError| e.to_string();
    let mut text = text.to_string();
    let mut conflicts = Vec::new();
    for rec in recs.iter().rev() {
        let Some(key) = rec.path.get(1) else {
            continue;
        };
        let now = toml_edit::get_raw(&text, Some(AGENTS), key).map_err(toml)?;
        match now {
            None => continue,
            Some(v) if v != rec.after => {
                conflicts.push(Conflict {
                    path: rec.path.clone(),
                });
                continue;
            }
            Some(_) => {}
        }
        text = match &rec.before {
            Some(raw) => toml_edit::set_raw(&text, Some(AGENTS), key, raw).map_err(toml)?,
            None => toml_edit::remove(&text, Some(AGENTS), key).map_err(toml)?,
        };
    }
    if conflicts.is_empty() && recs.iter().all(|r| r.before.is_none()) {
        text = toml_edit::prune_empty_table(&text, AGENTS).map_err(toml)?;
    }
    Ok((text, conflicts))
}
