use super::action::*;
use super::detect::Detected;
use super::diff::unified;
use super::keys::codex_keys_with;
use super::manifest::Part;
use super::model::Profile;
use super::plan::file_action;
use super::render::{self as r, fence_close, fence_open};
use super::safe::read_in;
use crate::catalog::Tool;
use crate::tools::agy_sidecar::plugin::{self, Plugin, MARK, MARK_TEXT};
use crate::tools::agy_sidecar::Roots;
use std::path::PathBuf;

pub const ID_CODEX_AGENTS: &str = "codex.agents_md";
pub const ID_CODEX_CONFIG: &str = "codex.config";
pub const ID_AGY_RULE: &str = "agy.rule";
pub const ID_AGY_PLUGIN_JSON: &str = "agy.plugin.json";
pub const ID_AGY_PLUGIN_MARK: &str = "agy.plugin.mark";

pub fn fence_region(text: &str, id: &str) -> Option<(usize, usize)> {
    let open = fence_open(id);
    let close = fence_close(id);
    let start = text.find(&open)?;
    let after = text[start..].find(&close)? + start + close.len();
    let end = if text[after..].starts_with("\r\n") {
        after + 2
    } else if text[after..].starts_with('\n') {
        after + 1
    } else {
        after
    };
    Some((start, end))
}

fn block_action(env: &Env, opts: &Options, profile: &Profile, path: &std::path::Path) -> Action {
    let mut a = Action::new(ID_CODEX_AGENTS, Tool::Codex, path, Part::Block);
    let block = r::fenced(
        r::FENCE_ID,
        &r::codex_block(profile, env.avail, opts.features),
    );
    let current = match read_in(env.roots, path) {
        Ok(t) => t,
        Err(why) => return a.skip(&format!("unreadable: {why}")),
    };
    a.block = Some(block.clone());
    let entry = env.manifest.get(ID_CODEX_AGENTS).filter(|e| !e.removed);
    let forced = opts.force.contains(ID_CODEX_AGENTS);
    let (after, kind, state) = match &current {
        None => (
            block.clone(),
            Kind::Create,
            if entry.is_some() {
                State::Missing
            } else {
                State::Fresh
            },
        ),
        Some(text) if text.trim().is_empty() => (block.clone(), Kind::Update, State::Fresh),
        Some(text) => match fence_region(text, r::FENCE_ID) {
            Some((s, e)) => {
                let region = &text[s..e];
                if entry.is_some_and(|x| x.released) && !forced {
                    a.before = current.clone();
                    a.state = State::Edited;
                    a.kind = Kind::Skip;
                    a.reason = Some("kept".to_string());
                    return a;
                }
                let unedited =
                    entry.is_none_or(|x| x.hash.as_deref() == Some(hash(region).as_str()));
                if !unedited && !forced {
                    a.before = current.clone();
                    a.state = State::Edited;
                    a.kind = Kind::Skip;
                    a.reason = Some("edited".to_string());
                    a.diff = unified(&path.to_string_lossy(), region, &block);
                    return a;
                }
                let mut after = text.clone();
                after.replace_range(s..e, &block);
                let same = after == *text;
                (
                    after,
                    if same { Kind::Unchanged } else { Kind::Update },
                    if same {
                        State::InStep
                    } else if unedited {
                        State::Outdated
                    } else {
                        State::Edited
                    },
                )
            }
            None => {
                a.needs = Some(CONSENT_CODEX_AGENTS);
                let sep = if text.ends_with("\n\n") {
                    ""
                } else if text.ends_with('\n') {
                    "\n"
                } else {
                    "\n\n"
                };
                (format!("{text}{sep}{block}"), Kind::Update, State::NotOurs)
            }
        },
    };
    a.diff = unified(
        &path.to_string_lossy(),
        current.as_deref().unwrap_or(""),
        &after,
    );
    a.before = current;
    a.after = Some(after);
    a.kind = kind;
    a.state = state;
    if a.kind.writes() && !opts.allows(a.needs) {
        a.kind = Kind::Offer;
    }
    a
}

pub fn codex_plan(
    env: &Env,
    opts: &Options,
    profile: &Profile,
    found: &Detected,
    out: &mut Vec<Action>,
) {
    let codex = &env.roots.codex;
    let agents = codex.join("AGENTS.md");
    let config = codex.join("config.toml");
    let skip_all = |why: &str, out: &mut Vec<Action>| {
        out.push(Action::new(ID_CODEX_AGENTS, Tool::Codex, &agents, Part::Block).skip(why));
        out.push(Action::new(ID_CODEX_CONFIG, Tool::Codex, &config, Part::Keys).skip(why));
    };
    if !found.codex {
        return skip_all("tool-missing", out);
    }
    if !profile.is_on(Tool::Codex) {
        return skip_all("tool-off", out);
    }
    if codex.join("AGENTS.override.md").exists() {
        out.push(
            Action::new(ID_CODEX_AGENTS, Tool::Codex, &agents, Part::Block)
                .skip("override-present"),
        );
    } else {
        out.push(block_action(env, opts, profile, &agents));
    }
    out.push(config_action(env, opts, profile, &config));
}

fn config_action(env: &Env, opts: &Options, profile: &Profile, path: &std::path::Path) -> Action {
    let mut a = Action::new(ID_CODEX_CONFIG, Tool::Codex, path, Part::Keys);
    a.needs = Some(CONSENT_CODEX_CONFIG);
    let text = match read_in(env.roots, path) {
        Ok(Some(t)) => t,
        Ok(None) => return a.skip("no-file"),
        Err(why) => return a.skip(&format!("unreadable: {why}")),
    };
    let prior = env
        .manifest
        .get(ID_CODEX_CONFIG)
        .map(|e| e.keys.clone())
        .unwrap_or_default();
    let forced = opts.force.contains(ID_CODEX_CONFIG);
    match codex_keys_with(&text, profile, env.avail, env.probe, &prior, forced) {
        Err(why) => a.skip(&format!("refused: {why}")),
        Ok(plan) => {
            a.diff = unified(&path.to_string_lossy(), &text, &plan.text);
            a.notes = plan
                .skipped
                .iter()
                .map(|s| format!("{}: {}", s.path.join("."), s.why))
                .collect();
            a.keys = plan.recs;
            a.kind = if plan.text == text {
                Kind::Unchanged
            } else {
                Kind::Update
            };
            a.state = match (a.kind, env.manifest.get(ID_CODEX_CONFIG)) {
                (Kind::Unchanged, _) => State::InStep,
                (_, Some(_)) => State::Outdated,
                _ => State::Fresh,
            };
            a.before = Some(text);
            a.after = Some(plan.text);
            if a.kind.writes() && !opts.allows(a.needs) {
                a.kind = Kind::Offer;
            }
            a
        }
    }
}

pub fn agy_plan(
    env: &Env,
    opts: &Options,
    profile: &Profile,
    found: &Detected,
    out: &mut Vec<Action>,
) {
    let roots = Roots {
        config: env.roots.antigravity.clone(),
        data: PathBuf::new(),
        apps: Vec::new(),
    };
    let dir = roots.plugin_dir();
    let rule = dir.join("rules").join(r::RULE_NAME);
    let json_path = dir.join("plugin.json");
    let mark_path = dir.join(MARK);
    let skipped = |why: &str, out: &mut Vec<Action>| {
        out.push(Action::new(ID_AGY_RULE, Tool::Antigravity, &rule, Part::File).skip(why));
    };
    if !found.antigravity {
        return skipped("tool-missing", out);
    }
    if !profile.is_on(Tool::Antigravity) {
        return skipped("tool-off", out);
    }
    let state = plugin::state(&roots);
    if state == Plugin::Foreign {
        return skipped("plugin-not-ours", out);
    }
    if state == Plugin::Absent {
        let manifest_json =
            serde_json::to_string_pretty(&plugin::manifest_json()).unwrap_or_default();
        out.push(file_action(
            env,
            opts,
            ID_AGY_PLUGIN_JSON,
            Tool::Antigravity,
            &json_path,
            &format!("{manifest_json}\n"),
            None,
            false,
        ));
        out.push(file_action(
            env,
            opts,
            ID_AGY_PLUGIN_MARK,
            Tool::Antigravity,
            &mark_path,
            MARK_TEXT,
            None,
            false,
        ));
    }
    let text = r::agy_rule(profile, env.avail, opts.features);
    out.push(file_action(
        env,
        opts,
        ID_AGY_RULE,
        Tool::Antigravity,
        &rule,
        &text,
        None,
        false,
    ));
}
