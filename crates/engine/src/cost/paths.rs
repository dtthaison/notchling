//! Path and byte helpers of the tracker: the directory walk, the names a transcript is known
//! by and the FNV-1a hash of the dedupe keys.

use super::tracker::MAX_DEPTH;
use std::path::{Path, PathBuf};

/// Files under `dir` (symlinks are not followed, except `dir` itself) with their sizes.
pub(super) fn walk(dir: &Path, depth: usize, out: &mut Vec<(PathBuf, u64)>) {
    if depth > MAX_DEPTH {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<_> = entries.flatten().collect();
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        let Ok(meta) = std::fs::symlink_metadata(entry.path()) else {
            continue;
        };
        if meta.is_dir() {
            walk(&entry.path(), depth + 1, out);
        } else if meta.is_file() {
            out.push((entry.path(), meta.len()));
        }
    }
}

/// What names a transcript apart from the folder it is reached through (a symlinked temp folder
/// reads as `/private/var` or `/var`, and the same file must count once): the last four parts of
/// the path. Codex's file name has the session id in it; Antigravity's is
/// `<conversation>/.system_generated/logs/transcript.jsonl`.
pub(super) fn file_id(path: &str) -> String {
    let parts: Vec<&str> = path.split('/').filter(|p| !p.is_empty()).collect();
    parts[parts.len().saturating_sub(4)..].join("/")
}

/// The project folder name is the cwd with "/" turned into "-"; its last part is close enough.
pub(super) fn project_name(path: &Path, root: &Path) -> String {
    let first = path
        .strip_prefix(root)
        .ok()
        .and_then(|rel| rel.components().next())
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .unwrap_or_default();
    first.rsplit('-').next().map(String::from).unwrap_or(first)
}

pub(super) fn is_uuid(part: &str) -> bool {
    let groups: Vec<&str> = part.split('-').collect();
    let lens = [8, 4, 4, 4, 12];
    groups.len() == 5
        && groups
            .iter()
            .zip(lens)
            .all(|(g, n)| g.len() == n && g.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')))
}

/// The account id ("accountUuid:orgUuid") in a Desktop transcript's path, if any.
pub(super) fn account_in_path(path: &Path) -> Option<String> {
    let parts: Vec<String> = path
        .components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect();
    parts
        .windows(2)
        .find(|w| is_uuid(&w[0]) && is_uuid(&w[1]))
        .map(|w| format!("{}:{}", w[0], w[1]))
}

pub(super) fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    let Some(&first) = needle.first() else {
        return true;
    };
    let mut from = 0;
    while let Some(i) = haystack[from..].iter().position(|&b| b == first) {
        let at = from + i;
        if haystack.len() - at < needle.len() {
            return false;
        }
        if &haystack[at..at + needle.len()] == needle {
            return true;
        }
        from = at + 1;
    }
    false
}

/// FNV-1a, stable across launches.
pub fn hash(text: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in text.bytes() {
        h = (h ^ u64::from(byte)).wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}
