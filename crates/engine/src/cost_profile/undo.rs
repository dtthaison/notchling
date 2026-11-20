use super::action::{hash, read_text, Env};
use super::keys::{undo_json_keys, undo_toml_keys};
use super::manifest::{Entry, Manifest, Part};
use super::paths::validate;
use super::plan_other::fence_region;
use super::render::FENCE_ID;
use super::safe::{make_parent, publish_new, read_in, replace_if, Publish};
use crate::catalog::Tool;
use std::path::PathBuf;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    Restored,
    Removed,
    Gone,
    Conflict,
    Failed,
}

impl Outcome {
    pub fn as_str(self) -> &'static str {
        match self {
            Outcome::Restored => "restored",
            Outcome::Removed => "removed",
            Outcome::Gone => "gone",
            Outcome::Conflict => "conflict",
            Outcome::Failed => "failed",
        }
    }
}

#[derive(Clone, Debug)]
pub struct UndoResult {
    pub id: String,
    pub path: PathBuf,
    pub outcome: Outcome,
    pub detail: Option<String>,
}

fn done(e: &Entry, outcome: Outcome, detail: Option<String>) -> UndoResult {
    UndoResult {
        id: e.id.clone(),
        path: e.path.clone(),
        outcome,
        detail,
    }
}

fn replace(e: &Entry, now: &str, with: String) -> Result<(), String> {
    replace_if(&e.path, now, false, with.as_bytes())
}

fn never_applied(e: &Entry, now: &str) -> bool {
    e.pending
        && !e.removed
        && e.existed
        && e.before_hash.as_deref().is_some_and(|h| h == hash(now))
        && e.hash.as_deref() != Some(hash(now).as_str())
}

fn undo_file(env: &Env, e: &Entry) -> (UndoResult, bool) {
    let now = match read_in(env.roots, &e.path) {
        Ok(t) => t,
        Err(why) => return (done(e, Outcome::Failed, Some(why)), false),
    };
    if e.removed {
        if let Some(text) = &now {
            if e.pending && e.before_hash.as_deref() == Some(hash(text).as_str()) {
                return (done(e, Outcome::Gone, Some("never-applied".into())), true);
            }
            return (done(e, Outcome::Conflict, Some("re-created".into())), false);
        }
        let Some(original) = e.backup.as_ref().and_then(|b| read_text(b).ok().flatten()) else {
            return (
                done(e, Outcome::Failed, Some("backup-missing".into())),
                false,
            );
        };
        let wrote = make_parent(&e.path).and_then(|_| {
            publish_new(&e.path, original.as_bytes()).map_err(|x| match x {
                Publish::Exists => "re-created".to_string(),
                Publish::Failed(why) => why,
            })
        });
        return match wrote {
            Ok(()) => (done(e, Outcome::Restored, None), true),
            Err(why) => (done(e, Outcome::Failed, Some(why)), false),
        };
    }
    let Some(text) = now else {
        return (done(e, Outcome::Gone, None), true);
    };
    if e.released {
        return (done(e, Outcome::Conflict, Some("kept".into())), false);
    }
    if never_applied(e, &text) {
        return (done(e, Outcome::Gone, Some("never-applied".into())), true);
    }
    if e.hash.as_deref() != Some(hash(&text).as_str()) {
        return (
            done(e, Outcome::Conflict, Some("changed-since".into())),
            false,
        );
    }
    if !e.existed {
        return match std::fs::remove_file(&e.path) {
            Ok(()) => {
                if e.tool == Tool::Antigravity {
                    prune_plugin_dirs(e);
                }
                (done(e, Outcome::Removed, None), true)
            }
            Err(x) => (done(e, Outcome::Failed, Some(x.to_string())), false),
        };
    }
    let Some(original) = e.backup.as_ref().and_then(|b| read_text(b).ok().flatten()) else {
        return (
            done(e, Outcome::Failed, Some("backup-missing".into())),
            false,
        );
    };
    match replace(e, &text, original) {
        Ok(()) => (done(e, Outcome::Restored, None), true),
        Err(why) => (done(e, Outcome::Failed, Some(why)), false),
    }
}

fn prune_plugin_dirs(e: &Entry) {
    let mut dir = e.path.parent().map(|p| p.to_path_buf());
    for _ in 0..2 {
        let Some(d) = dir else {
            return;
        };
        if std::fs::remove_dir(&d).is_err() {
            return;
        }
        dir = d.parent().map(|p| p.to_path_buf());
    }
}

fn undo_keys(env: &Env, e: &mut Entry) -> (UndoResult, bool) {
    let text = match read_in(env.roots, &e.path) {
        Ok(Some(t)) => t,
        Ok(None) => return (done(e, Outcome::Gone, None), true),
        Err(why) => return (done(e, Outcome::Failed, Some(why)), false),
    };
    let undone = if e.tool == Tool::Codex {
        undo_toml_keys(&text, &e.keys)
    } else {
        undo_json_keys(&text, &e.keys)
    };
    let (after, conflicts) = match undone {
        Ok(x) => x,
        Err(why) => return (done(e, Outcome::Failed, Some(why)), false),
    };
    if after != text {
        if let Err(why) = replace(e, &text, after) {
            return (done(e, Outcome::Failed, Some(why)), false);
        }
    }
    if conflicts.is_empty() {
        return (done(e, Outcome::Restored, None), true);
    }
    let names: Vec<String> = conflicts.iter().map(|c| c.path.join(".")).collect();
    e.keys
        .retain(|k| conflicts.iter().any(|c| c.path == k.path));
    (done(e, Outcome::Conflict, Some(names.join(", "))), false)
}

fn undo_block(env: &Env, e: &Entry) -> (UndoResult, bool) {
    let text = match read_in(env.roots, &e.path) {
        Ok(Some(t)) => t,
        Ok(None) => return (done(e, Outcome::Gone, None), true),
        Err(why) => return (done(e, Outcome::Failed, Some(why)), false),
    };
    let Some((s, end)) = fence_region(&text, FENCE_ID) else {
        return (done(e, Outcome::Gone, None), true);
    };
    if e.released {
        return (done(e, Outcome::Conflict, Some("kept".into())), false);
    }
    if e.pending && e.before_hash.as_deref() == Some(hash(&text[s..end]).as_str()) {
        return (done(e, Outcome::Gone, Some("never-applied".into())), true);
    }
    if e.hash.as_deref() != Some(hash(&text[s..end]).as_str()) {
        return (
            done(e, Outcome::Conflict, Some("changed-since".into())),
            false,
        );
    }
    let mut rest = text.clone();
    rest.replace_range(s..end, "");
    let rest = rest.trim_end_matches('\n').to_string();
    if !e.existed && rest.trim().is_empty() {
        return match std::fs::remove_file(&e.path) {
            Ok(()) => (done(e, Outcome::Removed, None), true),
            Err(x) => (done(e, Outcome::Failed, Some(x.to_string())), false),
        };
    }
    let rest = if rest.is_empty() {
        rest
    } else {
        format!("{rest}\n")
    };
    match replace(e, &text, rest) {
        Ok(()) => (done(e, Outcome::Restored, None), true),
        Err(why) => (done(e, Outcome::Failed, Some(why)), false),
    }
}

pub fn undo(env: &Env, tool: Tool, _stamp: u64) -> (Vec<UndoResult>, Manifest) {
    let mut manifest = env.manifest.clone();
    let mut results = Vec::new();
    let mine: Vec<Entry> = manifest.of_tool(tool).into_iter().cloned().collect();
    for mut entry in mine.into_iter().rev() {
        if let Err(why) = validate(env.roots, &entry) {
            results.push(done(&entry, Outcome::Failed, Some(why)));
            continue;
        }
        let (result, resolved) = match entry.part {
            Part::File => undo_file(env, &entry),
            Part::Keys => undo_keys(env, &mut entry),
            Part::Block => undo_block(env, &entry),
        };
        if resolved {
            manifest.drop_id(&entry.id);
        } else if result.outcome == Outcome::Conflict {
            manifest.put(entry);
        }
        results.push(result);
    }
    if let Err(why) = manifest.save(env.data_dir) {
        results.push(UndoResult {
            id: "manifest".into(),
            path: Manifest::path(env.data_dir),
            outcome: Outcome::Failed,
            detail: Some(why),
        });
    }
    (results, manifest)
}
