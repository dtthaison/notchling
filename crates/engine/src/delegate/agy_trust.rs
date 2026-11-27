use notchling_platform::files;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug)]
pub struct Files {
    pub claude_json: PathBuf,
    pub settings: PathBuf,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Trusted {
    Added(Vec<String>),
    Already,
    NotTrustedByClaude,
    Skipped,
}

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
