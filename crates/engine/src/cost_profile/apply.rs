use super::action::*;
use super::manifest::{Entry, Manifest, Part};
use super::model::Profile;
use super::paths::target_of;
use super::plan_other::fence_region;
use super::render::FENCE_ID;
use super::safe::{backup, guard, make_parent, prune_backups, publish_new, replace_if, Publish};
use std::collections::HashSet;
use std::path::PathBuf;

#[derive(Clone, Debug)]
pub struct ActionResult {
    pub id: String,
    pub path: PathBuf,
    pub kind: Kind,
    pub done: bool,
    pub stale: bool,
    pub backup: Option<PathBuf>,
    pub error: Option<String>,
}

pub type Only = std::collections::BTreeMap<String, Option<String>>;

#[derive(Clone, Debug)]
pub struct Applied {
    pub results: Vec<ActionResult>,
    pub profile: Profile,
    pub manifest: Manifest,
}

impl Applied {
    pub fn failed(&self) -> bool {
        self.results.iter().any(|r| r.error.is_some() && !r.stale)
    }
}

enum Stop {
    Failed(String),
    Halt(String),
}

fn create(a: &Action) -> Result<(), String> {
    let text = a.after.as_deref().unwrap_or_default();
    make_parent(&a.path)?;
    publish_new(&a.path, text.as_bytes()).map_err(|e| match e {
        Publish::Exists => "the file appeared while the plan was made; nothing was written".into(),
        Publish::Failed(why) => why,
    })
}

fn update(a: &Action) -> Result<(), String> {
    let before = a.before.clone().unwrap_or_default();
    let after = a.after.clone().unwrap_or_default();
    replace_if(&a.path, &before, before.is_empty(), after.as_bytes())
}

fn remove(a: &Action) -> Result<(), String> {
    let before = a.before.clone().unwrap_or_default();
    match read_text(&a.path)? {
        Some(now) if now == before => {}
        Some(_) => {
            return Err("the file changed while the plan was made; nothing was removed".into())
        }
        None => return Ok(()),
    }
    std::fs::remove_file(&a.path).map_err(|e| e.to_string())
}

fn before_hash_of(a: &Action) -> Option<String> {
    let before = a.before.as_deref()?;
    match a.part {
        Part::Block => fence_region(before, FENCE_ID).map(|(s, e)| hash(&before[s..e])),
        _ => Some(hash(before)),
    }
}

fn entry_for(a: &Action, backup: Option<PathBuf>, prior: Option<&Entry>, stamp: u64) -> Entry {
    let removed = a.kind == Kind::Remove;
    let carried = prior.filter(|p| !p.removed && !removed);
    Entry {
        id: a.id.clone(),
        tool: a.tool,
        path: a.path.clone(),
        part: a.part,
        removed,
        hash: match a.part {
            Part::Keys => None,
            Part::Block => a.block.as_deref().map(hash),
            Part::File if removed => a.before.as_deref().map(hash),
            Part::File => a.after.as_deref().map(hash),
        },
        existed: carried.map_or(a.before.is_some(), |p| p.existed),
        backup: carried.map_or(backup.clone(), |p| p.backup.clone().or(backup)),
        keys: a.keys.clone(),
        at: stamp,
        pending: true,
        before_hash: before_hash_of(a),
        released: false,
    }
}

fn revert(env: &Env, manifest: &mut Manifest, id: &str, prior: Option<Entry>) {
    match prior {
        Some(entry) => manifest.put(entry),
        None => manifest.drop_id(id),
    }
    let _ = manifest.save(env.data_dir);
}

fn run_one(
    env: &Env,
    plan: &Plan,
    a: &Action,
    manifest: &mut Manifest,
    done: &HashSet<String>,
    stamp: u64,
) -> Result<Option<PathBuf>, Stop> {
    for need in &a.requires {
        let met = done.contains(need) || plan.get(need).is_some_and(|x| x.kind == Kind::Unchanged);
        if !met {
            return Err(Stop::Failed(
                "its replacement wasn't written, so this was left alone".into(),
            ));
        }
    }
    match target_of(env.roots, &a.id) {
        Some((tool, path)) if tool == a.tool && path == a.path => {}
        _ => return Err(Stop::Failed("this is not one of Notchling's files".into())),
    }
    guard(env.roots, &a.path).map_err(Stop::Failed)?;
    let made = match (a.kind, &a.before) {
        (Kind::Update | Kind::Remove, Some(before)) => {
            Some(backup(&a.path, before, stamp).map_err(Stop::Failed)?)
        }
        _ => None,
    };
    let prior = manifest.get(&a.id).cloned();
    manifest.put(entry_for(a, made.clone(), prior.as_ref(), stamp));
    if let Err(why) = manifest.save(env.data_dir) {
        match prior {
            Some(entry) => manifest.put(entry),
            None => manifest.drop_id(&a.id),
        }
        return Err(Stop::Halt(why));
    }
    let wrote = match a.kind {
        Kind::Remove => remove(a),
        Kind::Create => create(a),
        _ => update(a),
    };
    match wrote {
        Ok(()) => Ok(made),
        Err(why) => {
            revert(env, manifest, &a.id, prior);
            Err(Stop::Failed(why))
        }
    }
}

pub fn before_text_hash(a: &Action) -> Option<String> {
    a.before.as_deref().map(hash)
}

pub fn apply(env: &Env, plan: &Plan, stamp: u64) -> Applied {
    apply_only(env, plan, stamp, None)
}

pub fn apply_only(env: &Env, plan: &Plan, stamp: u64, only: Option<&Only>) -> Applied {
    let mut manifest = env.manifest.clone();
    let mut results = Vec::new();
    let mut done: HashSet<String> = HashSet::new();
    let mut ordered: Vec<&Action> = plan
        .actions
        .iter()
        .filter(|a| a.kind.writes())
        .filter(|a| only.is_none_or(|o| o.contains_key(&a.id)))
        .collect();
    ordered.sort_by_key(|a| a.kind == Kind::Remove);
    let mut halted = false;
    if let Some(only) = only {
        for a in plan.actions.iter().filter(|a| !a.kind.writes()) {
            if only.get(&a.id).is_some_and(|expect| {
                matches!(a.kind, Kind::Skip | Kind::Offer) || before_text_hash(a) != *expect
            }) {
                results.push(ActionResult {
                    id: a.id.clone(),
                    path: a.path.clone(),
                    kind: a.kind,
                    done: false,
                    stale: true,
                    backup: None,
                    error: Some("the file changed after you said yes; nothing was written".into()),
                });
            }
        }
    }
    for a in ordered {
        let mut result = ActionResult {
            id: a.id.clone(),
            path: a.path.clone(),
            kind: a.kind,
            done: false,
            stale: false,
            backup: None,
            error: None,
        };
        if let Some(expect) = only.and_then(|o| o.get(&a.id)) {
            if before_text_hash(a) != *expect {
                result.stale = true;
                result.error =
                    Some("the file changed after you said yes; nothing was written".into());
                results.push(result);
                continue;
            }
        }
        if halted {
            result.error = Some("left alone: the record of earlier changes can't be saved".into());
            results.push(result);
            continue;
        }
        match run_one(env, plan, a, &mut manifest, &done, stamp) {
            Ok(made) => {
                done.insert(a.id.clone());
                result.done = true;
                result.backup = made;
            }
            Err(Stop::Failed(why)) => result.error = Some(why),
            Err(Stop::Halt(why)) => {
                halted = true;
                result.error = Some(format!("the record of changes: {why}"));
            }
        }
        results.push(result);
    }
    for entry in manifest.entries.iter_mut() {
        if done.contains(&entry.id) {
            entry.pending = false;
        }
    }
    if !done.is_empty() {
        if let Err(why) = manifest.save(env.data_dir) {
            results.push(ActionResult {
                id: "manifest".to_string(),
                path: Manifest::path(env.data_dir),
                kind: Kind::Update,
                done: false,
                stale: false,
                backup: None,
                error: Some(why),
            });
        }
    }
    let pinned: HashSet<PathBuf> = manifest
        .entries
        .iter()
        .filter_map(|e| e.backup.clone())
        .collect();
    for id in &done {
        if let Some(entry) = manifest.get(id) {
            prune_backups(&entry.path, &pinned);
        }
    }
    Applied {
        results,
        profile: plan.profile.clone(),
        manifest,
    }
}
