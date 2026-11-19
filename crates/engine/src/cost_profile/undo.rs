use super::action::{hash, read_text, Env};
use super::apply::make_parent;
use super::keys::{undo_json_keys, undo_toml_keys};
use super::manifest::{Entry, Manifest, Part};
use super::plan_other::fence_region;
use super::render::FENCE_ID;
use crate::catalog::Tool;
use crate::install::file::rewrite;
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

fn replace(e: &Entry, now: &str, with: String, stamp: u64) -> Result<(), String> {
    let now = now.to_string();
    let change = move |old: &str,
                       _: crate::install::file::State|
          -> Result<Option<String>, String> {
        if old != now {
            return Err("the file changed while it was being undone; nothing was written".into());
        }
        Ok(Some(with.clone()))
    };
    rewrite(&e.path, stamp, &change)
        .map(|_| ())
        .map_err(|x| x.to_string())
}

fn undo_file(e: &Entry, stamp: u64) -> (UndoResult, bool) {
    let now = match read_text(&e.path) {
        Ok(t) => t,
        Err(why) => return (done(e, Outcome::Failed, Some(why)), false),
    };
    if e.removed {
        if now.is_some() {
            return (done(e, Outcome::Conflict, Some("re-created".into())), false);
        }
        let Some(original) = e.backup.as_ref().and_then(|b| read_text(b).ok().flatten()) else {
            return (
                done(e, Outcome::Failed, Some("backup-missing".into())),
                false,
            );
        };
        let wrote = make_parent(&e.path).and_then(|_| {
            notchling_platform::files::write_private_no_follow(&e.path, original.as_bytes())
                .map_err(|x| x.to_string())
        });
        return match wrote {
            Ok(()) => (done(e, Outcome::Restored, None), true),
            Err(why) => (done(e, Outcome::Failed, Some(why)), false),
        };
    }
    let Some(text) = now else {
        return (done(e, Outcome::Gone, None), true);
    };
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
    match replace(e, &text, original, stamp) {
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

fn undo_keys(e: &mut Entry, stamp: u64) -> (UndoResult, bool) {
    let text = match read_text(&e.path) {
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
        if let Err(why) = replace(e, &text, after, stamp) {
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

fn undo_block(e: &Entry, stamp: u64) -> (UndoResult, bool) {
    let text = match read_text(&e.path) {
        Ok(Some(t)) => t,
        Ok(None) => return (done(e, Outcome::Gone, None), true),
        Err(why) => return (done(e, Outcome::Failed, Some(why)), false),
    };
    let Some((s, end)) = fence_region(&text, FENCE_ID) else {
        return (done(e, Outcome::Gone, None), true);
    };
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
    match replace(e, &text, rest, stamp) {
        Ok(()) => (done(e, Outcome::Restored, None), true),
        Err(why) => (done(e, Outcome::Failed, Some(why)), false),
    }
}

pub fn undo(env: &Env, tool: Tool, stamp: u64) -> (Vec<UndoResult>, Manifest) {
    let mut manifest = env.manifest.clone();
    let mut results = Vec::new();
    let mine: Vec<Entry> = manifest.of_tool(tool).into_iter().cloned().collect();
    for mut entry in mine.into_iter().rev() {
        let (result, resolved) = match entry.part {
            Part::File => undo_file(&entry, stamp),
            Part::Keys => undo_keys(&mut entry, stamp),
            Part::Block => undo_block(&entry, stamp),
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
