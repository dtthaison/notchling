use super::action::*;
use super::apply::{apply, Applied};
use super::import::cost_table_picks;
use super::keys::current_raw;
use super::manifest::{Entry, Manifest, Part};
use super::model::Profile;
use super::plan::{plan, ID_RULE};
use super::plan_other::fence_region;
use super::render::FENCE_ID;
use super::safe::read_in;
use crate::catalog::Tool;
use std::path::PathBuf;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    pub id: String,
    pub tool: Tool,
    pub path: PathBuf,
    pub state: &'static str,
    pub detail: Option<String>,
}

fn a_kept(a: &Action) -> bool {
    a.reason.as_deref() == Some("kept")
}

fn everything() -> Options {
    let parts: Vec<&str> = CONSENTS
        .iter()
        .copied()
        .filter(|c| *c != CONSENT_RULES)
        .collect();
    Options::consenting(&parts)
}

fn fallback(env: &Env, entry: &Entry) -> (&'static str, Option<String>) {
    match read_in(env.roots, &entry.path) {
        Ok(Some(text)) => match (entry.part, &entry.hash) {
            (Part::File, Some(h)) if !entry.removed && *h != hash(&text) => ("edited", None),
            (Part::File, _) if entry.removed => ("returned", None),
            _ => ("in_step", None),
        },
        Ok(None) if entry.removed => ("in_step", None),
        Ok(None) => ("missing", None),
        Err(why) => ("skipped", Some(why)),
    }
}

pub fn check(env: &Env, profile: &Profile) -> Vec<Row> {
    let planned = plan(env, profile, &everything());
    let mut rows = Vec::new();
    for entry in &env.manifest.entries {
        let action = planned.get(&entry.id);
        let (state, detail) = match action {
            None => fallback(env, entry),
            Some(a) => match (a.kind, a.state) {
                (Kind::Unchanged, _) if !a.notes.is_empty() => {
                    ("changed_by_tool", Some(a.notes.join("; ")))
                }
                (Kind::Unchanged, _) => ("in_step", None),
                (Kind::Skip, _) if a_kept(a) => ("skipped", Some("kept".to_string())),
                (Kind::Skip, State::Edited) => ("edited", None),
                (Kind::Skip, _) => ("skipped", a.reason.clone()),
                (Kind::Remove | Kind::Offer, _) if entry.removed => ("returned", None),
                (_, State::Missing) => ("missing", None),
                (_, State::Edited) => ("edited", None),
                (_, _) if a.notes.iter().any(|n| n.contains("changed-since")) => {
                    ("changed_by_tool", Some(a.notes.join("; ")))
                }
                _ => ("outdated", None),
            },
        };
        rows.push(Row {
            id: entry.id.clone(),
            tool: entry.tool,
            path: entry.path.clone(),
            state,
            detail,
        });
    }
    rows
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Choice {
    Keep,
    Restore,
}

impl Choice {
    pub fn parse(text: &str) -> Option<Choice> {
        match text {
            "keep" => Some(Choice::Keep),
            "restore" => Some(Choice::Restore),
            _ => None,
        }
    }
}

pub struct Resolved {
    pub manifest: Manifest,
    pub profile: Profile,
    pub applied: Option<Applied>,
}

pub fn resolve(
    env: &Env,
    profile: &Profile,
    id: &str,
    choice: Choice,
    stamp: u64,
) -> Result<Resolved, String> {
    let entry = env
        .manifest
        .get(id)
        .cloned()
        .ok_or_else(|| format!("nothing of Notchling's is recorded as {id}"))?;
    match choice {
        Choice::Restore => rewrite_one(env, profile, id, stamp),
        Choice::Keep => {
            let mut manifest = env.manifest.clone();
            let mut entry = entry;
            let text = read_in(env.roots, &entry.path)?.unwrap_or_default();
            let mut profile = profile.clone();
            match entry.part {
                Part::File => {
                    entry.hash = Some(hash(&text));
                    entry.released = true;
                    if id == ID_RULE {
                        for (role, cell) in cost_table_picks(&text) {
                            if profile.cell(role, Tool::Claude).is_none_or(|c| c != cell) {
                                profile.picks.insert((role, Tool::Claude), cell);
                            }
                        }
                    }
                }
                Part::Block => {
                    let (s, e) = fence_region(&text, FENCE_ID).ok_or("the block is gone")?;
                    entry.hash = Some(hash(&text[s..e]));
                    entry.released = true;
                }
                Part::Keys => {
                    for rec in &mut entry.keys {
                        if let Some(now) = current_raw(&text, entry.tool, &rec.path) {
                            if now != rec.after {
                                rec.after = now;
                                rec.kept = true;
                            }
                        }
                    }
                }
            }
            manifest.put(entry);
            manifest.save(env.data_dir)?;
            Ok(Resolved {
                manifest,
                profile,
                applied: None,
            })
        }
    }
}

fn rewrite_one(env: &Env, profile: &Profile, id: &str, stamp: u64) -> Result<Resolved, String> {
    let mut opts = everything();
    opts.force.insert(id.to_string());
    let mut planned = plan(env, profile, &opts);
    for action in planned.actions.iter_mut().filter(|a| a.id != id) {
        action.kind = Kind::Unchanged;
    }
    let applied = apply(env, &planned, stamp);
    Ok(Resolved {
        manifest: applied.manifest.clone(),
        profile: profile.clone(),
        applied: Some(applied),
    })
}
