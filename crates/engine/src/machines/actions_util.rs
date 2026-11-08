//! Small pure helpers of [`super::actions`]: what a machine may name, and the words for a failure.

use std::path::{Path, PathBuf};

use super::merge::MergeOutcome;
use super::morning::{MorningEntry, MorningItem};
use super::runner::CommandResult;

pub(super) fn branch_source(item: &MorningItem) -> Option<(String, String)> {
    match &item.entry {
        MorningEntry::Ready(e) => (!e.branch.is_empty() && !e.worktree.is_empty())
            .then(|| (e.branch.clone(), e.worktree.clone())),
        MorningEntry::NeedsYou(e) => Some((e.branch.clone()?, e.worktree.clone()?)),
        MorningEntry::Stuck(e) => Some((e.branch.clone()?, e.worktree.clone()?)),
    }
}

/// A file name the machine may name in its evidence folder: one plain name, no folders (`/`, `.`,
/// `..`), nothing an option parser could take for a flag (leading `-`), no control character (NUL,
/// newline, ...), and an image extension. The names come from the machine's `ls`, so each is
/// untrusted.
pub fn is_plain_image_name(name: &str) -> bool {
    if name.is_empty() || name == "." || name == ".." || name.starts_with('-') || name.contains('/')
    {
        return false;
    }
    if name.chars().any(|c| (c as u32) < 0x20 || c as u32 == 0x7f) {
        return false;
    }
    let ext = Path::new(name)
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_lowercase);
    matches!(ext.as_deref(), Some("png" | "jpg" | "jpeg"))
}

/// Where a copy of `name` goes: a file directly inside `dir`, or `None`.
pub fn destination(name: &str, dir: &Path) -> Option<PathBuf> {
    let plain = !name.is_empty()
        && name != "."
        && name != ".."
        && !name.contains('/')
        && !name.contains('\0');
    plain.then(|| dir.join(name))
}

pub(super) fn failure(r: &CommandResult, what: &str) -> String {
    let text = r.err.trim();
    if r.code == 124 {
        return format!("{what} timed out");
    }
    if text.is_empty() {
        format!("{what} exited {}", r.code)
    } else {
        text.to_string()
    }
}

/// `None` when the merge or the push retry went all the way through.
pub(super) fn failure_of(o: &MergeOutcome) -> Option<String> {
    if let Some(e) = &o.error {
        return Some(e.clone());
    }
    if !o.conflict_files.is_empty() {
        return Some(format!("merge conflict in {}", o.conflict_files.join(", ")));
    }
    if !o.pushed {
        return Some("main was not pushed".into());
    }
    if !o.closed {
        return Some("the issue was not closed".into());
    }
    None
}
