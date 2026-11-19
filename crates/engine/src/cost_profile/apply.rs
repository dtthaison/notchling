use super::action::*;
use super::manifest::{Entry, Manifest, Part};
use super::model::Profile;
use crate::install::file::{rewrite, write_backup};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug)]
pub struct ActionResult {
    pub id: String,
    pub path: PathBuf,
    pub kind: Kind,
    pub done: bool,
    pub backup: Option<PathBuf>,
    pub error: Option<String>,
}

#[derive(Clone, Debug)]
pub struct Applied {
    pub results: Vec<ActionResult>,
    pub profile: Profile,
    pub manifest: Manifest,
}

impl Applied {
    pub fn failed(&self) -> bool {
        self.results.iter().any(|r| r.error.is_some())
    }
}

pub(crate) fn make_parent(path: &Path) -> Result<(), String> {
    match path.parent() {
        Some(dir) => std::fs::create_dir_all(dir).map_err(|e| format!("the folder: {e}")),
        None => Ok(()),
    }
}

fn create(a: &Action) -> Result<Option<PathBuf>, String> {
    let text = a.after.as_deref().unwrap_or_default();
    make_parent(&a.path)?;
    if std::fs::symlink_metadata(&a.path).is_ok() {
        return Err("the file appeared while the plan was made; nothing was written".into());
    }
    notchling_platform::files::write_private_no_follow(&a.path, text.as_bytes())
        .map_err(|e| e.to_string())?;
    Ok(None)
}

fn update(a: &Action, stamp: u64) -> Result<Option<PathBuf>, String> {
    let before = a.before.clone().unwrap_or_default();
    let after = a.after.clone().unwrap_or_default();
    let blank_ok = before.is_empty();
    let change =
        move |old: &str, _: crate::install::file::State| -> Result<Option<String>, String> {
            if old != before && !(blank_ok && old.trim().is_empty()) {
                return Err("the file changed while the plan was made; nothing was written".into());
            }
            Ok(Some(after.clone()))
        };
    rewrite(&a.path, stamp, &change)
        .map(|done| done.backup)
        .map_err(|e| e.to_string())
}

fn remove(a: &Action, stamp: u64) -> Result<Option<PathBuf>, String> {
    let before = a.before.clone().unwrap_or_default();
    match read_text(&a.path)? {
        Some(now) if now == before => {}
        Some(_) => {
            return Err("the file changed while the plan was made; nothing was removed".into())
        }
        None => return Ok(None),
    }
    let backup = write_backup(&a.path, &before, stamp).map_err(|e| e.to_string())?;
    std::fs::remove_file(&a.path).map_err(|e| e.to_string())?;
    Ok(Some(backup))
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
    }
}

pub fn apply(env: &Env, plan: &Plan, stamp: u64) -> Applied {
    let mut manifest = env.manifest.clone();
    let mut results = Vec::new();
    let mut ordered: Vec<&Action> = plan.actions.iter().filter(|a| a.kind.writes()).collect();
    ordered.sort_by_key(|a| a.kind == Kind::Remove);
    for a in ordered {
        let done = match (a.part, a.kind) {
            (_, Kind::Remove) => remove(a, stamp),
            (_, Kind::Create) => create(a),
            _ => update(a, stamp),
        };
        let mut result = ActionResult {
            id: a.id.clone(),
            path: a.path.clone(),
            kind: a.kind,
            done: false,
            backup: None,
            error: None,
        };
        match done {
            Ok(backup) => {
                let prior = manifest.get(&a.id).cloned();
                manifest.put(entry_for(a, backup.clone(), prior.as_ref(), stamp));
                result.done = true;
                result.backup = backup;
            }
            Err(why) => result.error = Some(why),
        }
        results.push(result);
    }
    if let Err(why) = manifest.save(env.data_dir) {
        results.push(ActionResult {
            id: "manifest".to_string(),
            path: super::manifest::Manifest::path(env.data_dir),
            kind: Kind::Update,
            done: false,
            backup: None,
            error: Some(why),
        });
    }
    Applied {
        results,
        profile: plan.profile.clone(),
        manifest,
    }
}
