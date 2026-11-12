//! Antigravity CLI asks before it works in a folder it has not been told to trust, and a job
//! delegated from a session runs in the caller's folder, often a fresh git worktree, so it would ask
//! every time. Claude Code already asked the user about the folder (`~/.claude.json`,
//! `projects[<path>].hasTrustDialogAccepted`), and a folder it trusts covers the ones under it. So,
//! before `agy acp` starts, a folder Claude Code trusts is added to agy's own list, `trustedWorkspaces`
//! in `~/.gemini/antigravity-cli/settings.json`. A folder Claude Code does not trust is left alone:
//! agy asks as usual.
//!
//! The settings file is the tool's own, so it is changed with care: read, one entry added to
//! `trustedWorkspaces`, written back whole and atomically with its mode kept. Nothing is ever
//! removed, an entry already there is not added again, and a file that can't be read, isn't JSON or
//! whose list isn't a list is left exactly as it is. Both paths are parameters, so a test never
//! reaches the real files.

use notchling_platform::files;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

/// The two files, as the caller names them.
#[derive(Clone, Debug)]
pub struct Files {
    /// `~/.claude.json`.
    pub claude_json: PathBuf,
    /// `~/.gemini/antigravity-cli/settings.json`.
    pub settings: PathBuf,
}

/// What [`trust`] did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Trusted {
    /// The folders added to the list (the folder as given, and its real path when that differs).
    Added(Vec<String>),
    /// Already in the list.
    Already,
    /// Claude Code doesn't trust the folder (or its parents), so nothing was done.
    NotTrustedByClaude,
    /// A file couldn't be used; nothing was changed.
    Skipped,
}

/// Whether Claude Code's state (`~/.claude.json`'s text) trusts `folder` or one of its parents.
pub fn claude_trusts(claude_json: &str, folder: &Path) -> bool {
    let Ok(Value::Object(root)) = serde_json::from_str::<Value>(claude_json) else {
        return false;
    };
    let Some(Value::Object(projects)) = root.get("projects") else {
        return false;
    };
    folder.ancestors().any(|dir| {
        let key = dir.to_string_lossy();
        projects
            .get(key.trim_end_matches('/'))
            .or_else(|| projects.get(key.as_ref()))
            .and_then(|p| p.get("hasTrustDialogAccepted"))
            .and_then(Value::as_bool)
            == Some(true)
    })
}

/// The settings text with `folders` in `trustedWorkspaces`, and the ones that were new. `None` when
/// the text isn't a JSON object or its `trustedWorkspaces` isn't a list: it is then not touched. A
/// missing list is made. Every other key and every entry stays.
pub fn merged(settings: &str, folders: &[String]) -> Option<(String, Vec<String>)> {
    let mut root: Value = if settings.trim().is_empty() {
        json!({})
    } else {
        serde_json::from_str(settings).ok()?
    };
    let object = root.as_object_mut()?;
    let list = object
        .entry("trustedWorkspaces")
        .or_insert_with(|| json!([]))
        .as_array_mut()?;
    let same = |a: &str, b: &str| a.trim_end_matches('/') == b.trim_end_matches('/');
    let mut added = Vec::new();
    for folder in folders {
        let present = list
            .iter()
            .any(|e| e.as_str().is_some_and(|e| same(e, folder)));
        if !present && !added.contains(folder) {
            list.push(Value::String(folder.clone()));
            added.push(folder.clone());
        }
    }
    let mut text = serde_json::to_string_pretty(&root).ok()?;
    text.push('\n');
    Some((text, added))
}

/// Trusts `folder` in Antigravity CLI when Claude Code trusts it.
pub fn trust(files_at: &Files, folder: &Path) -> Trusted {
    let real = std::fs::canonicalize(folder).ok();
    let trusted = std::fs::read_to_string(&files_at.claude_json).is_ok_and(|text| {
        claude_trusts(&text, folder) || real.as_deref().is_some_and(|r| claude_trusts(&text, r))
    });
    if !trusted {
        return Trusted::NotTrustedByClaude;
    }
    let Ok(settings) = std::fs::read_to_string(&files_at.settings) else {
        return Trusted::Skipped;
    };
    // agy resolves links, so its list may hold the real path; both spellings are recorded.
    let mut wanted = vec![folder.to_string_lossy().into_owned()];
    if let Some(real) = real {
        let real = real.to_string_lossy().into_owned();
        if !wanted.contains(&real) {
            wanted.push(real);
        }
    }
    let Some((text, added)) = merged(&settings, &wanted) else {
        return Trusted::Skipped;
    };
    if added.is_empty() {
        return Trusted::Already;
    }
    let mode = std::fs::metadata(&files_at.settings).ok();
    if files::write_private(&files_at.settings, text.as_bytes()).is_err() {
        return Trusted::Skipped;
    }
    // `write_private` makes the file 0600; the file keeps the mode it had.
    #[cfg(unix)]
    if let Some(meta) = mode {
        let _ = std::fs::set_permissions(&files_at.settings, meta.permissions());
    }
    #[cfg(not(unix))]
    let _ = mode;
    Trusted::Added(added)
}

#[cfg(test)]
#[path = "agy_trust_tests.rs"]
mod tests;
