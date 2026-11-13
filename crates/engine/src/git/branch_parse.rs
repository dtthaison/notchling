use super::porcelain::parse_numstat;
use serde::Serialize;
use std::collections::HashSet;

pub const MAX_BRANCH_FILES: usize = 2000;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct BranchFile {
    pub path: String,
    pub status: String,
    pub from: Option<String>,
    pub added: Option<u64>,
    pub removed: Option<u64>,
    pub uncommitted: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct BranchStat {
    pub branch: Option<String>,
    pub head: Option<String>,
    pub detached: bool,
    pub base: Option<String>,
    pub merge_base: Option<String>,
    pub files: Vec<BranchFile>,
    pub added: u64,
    pub removed: u64,
    pub file_count: u64,
    pub uncommitted_files: u64,
    pub truncated: bool,
}

fn letter(code: &str) -> &'static str {
    match code.chars().next() {
        Some('A') => "A",
        Some('D') => "D",
        Some('R') | Some('C') => "R",
        Some('U') => "U",
        _ => "M",
    }
}

pub fn parse_name_status(text: &str) -> Vec<(String, String, Option<String>)> {
    let mut out = Vec::new();
    let mut records = text.split('\0');
    while let Some(code) = records.next() {
        if code.is_empty() {
            continue;
        }
        let pair = matches!(code.chars().next(), Some('R') | Some('C'));
        let first = records.next().unwrap_or("");
        if pair {
            let new = records.next().unwrap_or("");
            if !new.is_empty() {
                out.push((
                    letter(code).to_owned(),
                    new.to_owned(),
                    Some(first.to_owned()).filter(|f| !f.is_empty()),
                ));
            }
        } else if !first.is_empty() {
            out.push((letter(code).to_owned(), first.to_owned(), None));
        }
    }
    out
}

pub fn assemble(
    name_status: &str,
    numstat: &str,
    uncommitted: &HashSet<String>,
    untracked: Vec<(String, Option<u64>)>,
) -> (Vec<BranchFile>, bool) {
    let counts = parse_numstat(numstat);
    let mut files: Vec<BranchFile> = parse_name_status(name_status)
        .into_iter()
        .map(|(status, path, from)| {
            let (added, removed) = counts.get(&path).copied().unwrap_or((None, None));
            BranchFile {
                uncommitted: uncommitted.contains(&path),
                path,
                status,
                from,
                added,
                removed,
            }
        })
        .collect();
    let known: HashSet<String> = files.iter().map(|f| f.path.clone()).collect();
    for (path, lines) in untracked {
        if known.contains(&path) {
            continue;
        }
        files.push(BranchFile {
            path,
            status: "?".to_owned(),
            from: None,
            added: lines,
            removed: lines.map(|_| 0),
            uncommitted: true,
        });
    }
    files.sort_by(|a, b| a.path.cmp(&b.path));
    let cut = files.len() > MAX_BRANCH_FILES;
    (files, cut)
}

pub fn finish(stat: &mut BranchStat, files: Vec<BranchFile>, cut: bool) {
    stat.file_count = files.len() as u64;
    stat.added = files.iter().filter_map(|f| f.added).sum();
    stat.removed = files.iter().filter_map(|f| f.removed).sum();
    stat.uncommitted_files = files.iter().filter(|f| f.uncommitted).count() as u64;
    stat.truncated |= cut;
    stat.files = files.into_iter().take(MAX_BRANCH_FILES).collect();
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct Branches {
    pub current: Option<String>,
    pub default: Option<String>,
    pub remote_default: Option<String>,
    pub local: Vec<String>,
    pub truncated: bool,
}

pub const MAX_BRANCHES: usize = 200;

pub fn parse_branches(text: &str) -> (Option<String>, Vec<String>, bool) {
    let mut current = None;
    let mut local = Vec::new();
    let mut cut = false;
    for line in text.lines() {
        let Some(name) = line.get(2..).filter(|n| !n.is_empty()) else {
            continue;
        };
        if local.len() >= MAX_BRANCHES {
            cut = true;
            break;
        }
        if line.starts_with('*') {
            current = Some(name.to_owned());
        }
        local.push(name.to_owned());
    }
    (current, local, cut)
}
