use serde::Serialize;

pub const MAX_WORKTREES: usize = 50;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct WorktreeEntry {
    pub path: String,
    pub head: Option<String>,
    pub branch: Option<String>,
    pub detached: bool,
    pub bare: bool,
    pub locked: bool,
    pub prunable: bool,
}

fn flag(attr: &str, word: &str) -> bool {
    attr == word
        || attr
            .strip_prefix(word)
            .is_some_and(|rest| rest.starts_with(' '))
}

fn apply(entry: &mut WorktreeEntry, attr: &str) {
    if let Some(head) = attr.strip_prefix("HEAD ") {
        let unborn = !head.is_empty() && head.bytes().all(|b| b == b'0');
        entry.head = (!head.is_empty() && !unborn).then(|| head.to_string());
    } else if let Some(branch) = attr.strip_prefix("branch ") {
        let name = branch.strip_prefix("refs/heads/").unwrap_or(branch);
        entry.branch = (!name.is_empty()).then(|| name.to_string());
    } else if attr == "detached" {
        entry.detached = true;
    } else if attr == "bare" {
        entry.bare = true;
    } else if flag(attr, "locked") {
        entry.locked = true;
    } else if flag(attr, "prunable") {
        entry.prunable = true;
    }
}

pub fn parse_worktrees(text: &str) -> (Vec<WorktreeEntry>, bool) {
    let mut entries: Vec<WorktreeEntry> = Vec::new();
    let mut current: Option<WorktreeEntry> = None;
    for attr in text.split('\0') {
        if attr.is_empty() {
            entries.extend(current.take());
            continue;
        }
        if let Some(path) = attr.strip_prefix("worktree ") {
            entries.extend(current.take());
            if entries.len() == MAX_WORKTREES {
                return (entries, true);
            }
            current = Some(WorktreeEntry {
                path: path.to_string(),
                ..Default::default()
            });
        } else if let Some(entry) = current.as_mut() {
            apply(entry, attr);
        }
    }
    entries.extend(current.take());
    (entries, false)
}
