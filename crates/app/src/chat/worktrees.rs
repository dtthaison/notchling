use std::path::Path;

use notchling_engine::sessions::{Agent, AgentGroup};
use notchling_locale as t;
use serde_json::{json, Value};

use super::branch::BranchModel;
use super::data::Session;
use super::format::age;
use super::git::GitModel;
use super::inspector::{counted_title, SubagentRow};
use crate::acting::Request;

pub const WORKTREES_OP: &str = "git.worktrees";
pub const SHOWN_WORKTREES: usize = 6;

const JUST_NOW_MS: i64 = 60_000;
const HEAD_CHARS: usize = 7;
const SEPARATOR: &str = "\u{b7}";

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Work {
    pub added: u64,
    pub removed: u64,
    pub files: u64,
    pub uncommitted_files: u64,
    pub changed_at: Option<i64>,
    pub read_at: i64,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Worktree {
    pub path: String,
    pub real: String,
    pub main: bool,
    pub current: bool,
    pub branch: Option<String>,
    pub head: Option<String>,
    pub detached: bool,
    pub locked: bool,
    pub missing: bool,
    pub upstream: Option<String>,
    pub upstream_ahead: u64,
    pub upstream_behind: u64,
    pub upstream_gone: bool,
    pub base: Option<String>,
    pub ahead: u64,
    pub behind: u64,
    pub unrelated: bool,
    pub committed_at: Option<i64>,
    pub work: Option<Work>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Worktrees {
    pub name: String,
    pub default: Option<String>,
    pub integration: Option<String>,
    pub fetched_at: Option<i64>,
    pub no_worktree: u64,
    pub truncated: bool,
    pub list: Vec<Worktree>,
}

pub fn parse_work(value: &Value) -> Option<Work> {
    value.as_object()?;
    let number = |key: &str| value[key].as_u64().unwrap_or(0);
    Some(Work {
        added: number("added"),
        removed: number("removed"),
        files: number("files"),
        uncommitted_files: number("uncommitted_files"),
        changed_at: value["changed_at"].as_i64(),
        read_at: value["read_at"].as_i64().unwrap_or(0),
    })
}

fn parse_worktree(row: &Value) -> Option<Worktree> {
    let path = row["path"].as_str().filter(|p| !p.is_empty())?;
    let text = |key: &str| row[key].as_str().map(str::to_owned);
    let flag = |key: &str| row[key].as_bool().unwrap_or(false);
    let number = |key: &str| row[key].as_u64().unwrap_or(0);
    Some(Worktree {
        path: path.to_owned(),
        real: text("real").unwrap_or_else(|| path.to_owned()),
        main: flag("main"),
        current: flag("current"),
        branch: text("branch"),
        head: text("head"),
        detached: flag("detached"),
        locked: flag("locked"),
        missing: flag("missing"),
        upstream: text("upstream"),
        upstream_ahead: number("upstream_ahead"),
        upstream_behind: number("upstream_behind"),
        upstream_gone: flag("upstream_gone"),
        base: text("base"),
        ahead: number("ahead"),
        behind: number("behind"),
        unrelated: flag("unrelated"),
        committed_at: row["committed_at"].as_i64(),
        work: parse_work(&row["work"]),
    })
}

pub fn parse_worktrees(reply: &Value) -> Option<Worktrees> {
    if reply["repo"] != json!(true) {
        return None;
    }
    let text = |key: &str| reply[key].as_str().map(str::to_owned);
    Some(Worktrees {
        name: text("name").unwrap_or_default(),
        default: text("default"),
        integration: text("integration"),
        fetched_at: reply["fetched_at"].as_i64(),
        no_worktree: reply["no_worktree"].as_u64().unwrap_or(0),
        truncated: reply["truncated"].as_bool().unwrap_or(false),
        list: reply["worktrees"]
            .as_array()
            .map(|rows| rows.iter().filter_map(parse_worktree).collect())
            .unwrap_or_default(),
    })
}

pub fn worktrees_request(cwd: &str, fresh: &[String]) -> Request {
    let mut args = json!({ "cwd": cwd });
    if !fresh.is_empty() {
        args["fresh"] = json!(fresh);
    }
    Request {
        op: WORKTREES_OP,
        args,
    }
}

pub fn shown_base(base: &str, default: Option<&str>) -> String {
    match (default, base.strip_prefix("origin/")) {
        (Some(default), Some(short)) if default == base => short.to_owned(),
        _ => base.to_owned(),
    }
}

pub fn session_folder(agent: &Agent) -> &str {
    agent.worktree_path.as_deref().unwrap_or(&agent.cwd)
}

pub fn owner(folder: &str, paths: &[(&str, &str)]) -> Option<usize> {
    if folder.is_empty() {
        return None;
    }
    let folder = Path::new(folder);
    let mut best: Option<(usize, usize)> = None;
    for (index, (path, real)) in paths.iter().enumerate() {
        for candidate in [path, real] {
            if candidate.is_empty() || !folder.starts_with(candidate) {
                continue;
            }
            let depth = Path::new(candidate).components().count();
            if best.is_none_or(|(_, deepest)| depth > deepest) {
                best = Some((index, depth));
            }
        }
    }
    best.map(|(index, _)| index)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Open {
    Session(String),
    Subagent(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Occupancy {
    Running {
        since_ms: i64,
        open: Open,
        more: usize,
    },
    NeedsInput {
        open: Open,
        more: usize,
    },
    Idle {
        open: Open,
    },
}

type Matches = Vec<(i64, Open)>;

fn oldest_first(matches: &mut Matches) {
    matches.sort_by_key(|(since, _)| *since);
}

fn newest(matches: Matches) -> Option<Open> {
    let mut best: Option<(i64, Open)> = None;
    for (since, open) in matches {
        if best.as_ref().is_none_or(|(was, _)| since > *was) {
            best = Some((since, open));
        }
    }
    best.map(|(_, open)| open)
}

pub fn occupancy(
    paths: &[(&str, &str)],
    sessions: &[Session],
    subagents: &[SubagentRow],
) -> Vec<Option<Occupancy>> {
    let mut running: Vec<Matches> = vec![Vec::new(); paths.len()];
    let mut needs: Vec<Matches> = vec![Vec::new(); paths.len()];
    let mut idle: Vec<Matches> = vec![Vec::new(); paths.len()];
    for session in sessions {
        let agent = &session.agent;
        if agent.node.is_some() {
            continue;
        }
        let Some(index) = owner(session_folder(agent), paths) else {
            continue;
        };
        let found = (agent.since_ms, Open::Session(agent.id.clone()));
        match agent.group {
            AgentGroup::Working => running[index].push(found),
            AgentGroup::NeedsInput => needs[index].push(found),
            AgentGroup::Completed => idle[index].push(found),
        }
    }
    for row in subagents.iter().filter(|row| row.running) {
        let Some(index) = row.cwd.as_deref().and_then(|cwd| owner(cwd, paths)) else {
            continue;
        };
        running[index].push((row.started_ms, Open::Subagent(row.id.clone())));
    }
    running
        .into_iter()
        .zip(needs)
        .zip(idle)
        .map(|((mut running, mut needs), idle)| {
            if !running.is_empty() {
                oldest_first(&mut running);
                let more = running.len() - 1;
                let (since_ms, open) = running.swap_remove(0);
                return Some(Occupancy::Running {
                    since_ms,
                    open,
                    more,
                });
            }
            if !needs.is_empty() {
                oldest_first(&mut needs);
                let more = needs.len() - 1;
                let (_, open) = needs.swap_remove(0);
                return Some(Occupancy::NeedsInput { open, more });
            }
            newest(idle).map(|open| Occupancy::Idle { open })
        })
        .collect()
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SubPart {
    Faint(String),
    Added(u64),
    Removed(u64),
    Running(String),
}

pub fn sub_text(parts: &[SubPart]) -> String {
    parts
        .iter()
        .map(|part| match part {
            SubPart::Faint(text) | SubPart::Running(text) => text.clone(),
            SubPart::Added(n) => format!("+{n}"),
            SubPart::Removed(n) => format!("\u{2212}{n}"),
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CurrentRow {
    pub branch: String,
    pub right: Option<(u64, u64)>,
    pub sub: Vec<SubPart>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OtherRow {
    pub path: String,
    pub branch: String,
    pub right: Option<(u64, u64)>,
    pub sub: Vec<SubPart>,
    pub working: bool,
    pub missing: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GitSection {
    pub title: String,
    pub current: CurrentRow,
    pub others: Vec<OtherRow>,
    pub more: Option<String>,
}

pub fn title(running_others: usize, worktrees: usize) -> String {
    if worktrees <= 1 {
        return t::chat::details_git().to_owned();
    }
    counted_title(t::chat::details_git(), running_others, worktrees)
}

fn faint(text: String) -> SubPart {
    SubPart::Faint(text)
}

fn join(groups: Vec<Vec<SubPart>>) -> Vec<SubPart> {
    let mut out: Vec<SubPart> = Vec::new();
    for group in groups.into_iter().filter(|group| !group.is_empty()) {
        if !out.is_empty() {
            out.push(faint(SEPARATOR.to_owned()));
        }
        out.extend(group);
    }
    out
}

fn last_part(since: Option<i64>, minute_ms: i64) -> Option<SubPart> {
    let since = since.filter(|since| *since > 0)?;
    if minute_ms - since < JUST_NOW_MS {
        return Some(faint(t::chat::git_last_now().to_owned()));
    }
    Some(faint(t::chat::git_last_ago(&age(minute_ms, since))))
}

fn newer(a: Option<i64>, b: Option<i64>) -> Option<i64> {
    match (a, b) {
        (Some(a), Some(b)) => Some(a.max(b)),
        (a, b) => a.or(b),
    }
}

fn uncommitted(status: &GitModel) -> Vec<SubPart> {
    if status.files.is_empty() {
        return Vec::new();
    }
    let plus = if status.truncated { "+" } else { "" };
    let sum = |pick: fn(&super::git::GitFile) -> Option<u64>| -> u64 {
        status.files.iter().filter_map(pick).sum()
    };
    let (added, removed) = (sum(|f| f.added), sum(|f| f.removed));
    let mut out = vec![faint(t::chat::git_uncommitted(&format!(
        "{}{plus}",
        status.files.len()
    )))];
    if added > 0 {
        out.push(SubPart::Added(added));
    }
    if removed > 0 {
        out.push(SubPart::Removed(removed));
    }
    out
}

fn upstream_label<'a>(upstream: &'a str, branch: Option<&str>) -> &'a str {
    match (upstream.split_once('/'), branch) {
        (Some((remote, name)), Some(branch)) if name == branch => remote,
        _ => upstream,
    }
}

fn upstream_part(branch: Option<&str>, upstream: Option<&str>, ahead: u64, behind: u64) -> SubPart {
    let Some(upstream) = upstream else {
        return faint(t::chat::git_no_upstream().to_owned());
    };
    faint(match (ahead, behind) {
        (0, 0) => t::chat::git_even_with(upstream_label(upstream, branch)),
        (a, 0) => t::chat::git_to_push(&a.to_string()),
        (0, b) => t::chat::git_only_behind(&b.to_string()),
        (a, b) => t::chat::git_push_and_behind(&a.to_string(), &b.to_string()),
    })
}

fn current_upstream(status: &GitModel, entry: Option<&Worktree>) -> Option<SubPart> {
    let Some(entry) = entry else {
        return Some(upstream_part(
            Some(&status.branch),
            status.upstream.as_deref(),
            status.ahead,
            status.behind,
        ));
    };
    if entry.detached {
        return None;
    }
    if entry.upstream_gone {
        return Some(faint(t::chat::git_upstream_gone().to_owned()));
    }
    Some(upstream_part(
        entry.branch.as_deref(),
        entry.upstream.as_deref(),
        entry.upstream_ahead,
        entry.upstream_behind,
    ))
}

fn current_row(
    status: &GitModel,
    stat: Option<&BranchModel>,
    worktrees: Option<&Worktrees>,
    entry: Option<&Worktree>,
    minute_ms: i64,
) -> CurrentRow {
    let own = uncommitted(status);
    if status.initial {
        return CurrentRow {
            branch: status.branch.clone(),
            right: None,
            sub: own,
        };
    }
    let last = last_part(
        newer(entry.and_then(|e| e.committed_at), status.changed_at),
        minute_ms,
    );
    let default = worktrees.and_then(|w| w.default.as_deref());
    let versus = stat.map(|stat| faint(t::chat::git_vs_base(&shown_base(&stat.base, default))));
    CurrentRow {
        branch: status.branch.clone(),
        right: stat.map(|stat| (stat.added, stat.removed)),
        sub: join(vec![
            own,
            last.into_iter().collect(),
            current_upstream(status, entry).into_iter().collect(),
            versus.into_iter().collect(),
        ]),
    }
}

fn row_branch(entry: &Worktree) -> String {
    if let Some(branch) = entry.branch.as_deref().filter(|b| !b.is_empty()) {
        return branch.to_owned();
    }
    if !entry.detached {
        return t::chat::git_no_branch().to_owned();
    }
    match entry.head.as_deref() {
        Some(head) => t::chat::git_detached_at(&head.chars().take(HEAD_CHARS).collect::<String>()),
        None => t::chat::git_detached().to_owned(),
    }
}

fn running_text(since_ms: i64, more: usize, minute_ms: i64) -> String {
    let how_long = age(minute_ms, since_ms);
    let base = if how_long.is_empty() {
        t::chat::status_running().to_lowercase()
    } else {
        t::chat::git_running_for(&how_long)
    };
    match more {
        0 => base,
        n => format!("{base} {}", t::chat::git_more_matches(&n.to_string())),
    }
}

fn ahead_part(entry: &Worktree) -> Option<SubPart> {
    entry.base.as_ref()?;
    Some(faint(match entry.behind {
        0 => t::chat::git_base_ahead(&entry.ahead.to_string()),
        behind => t::chat::git_base_ahead_behind(&entry.ahead.to_string(), &behind.to_string()),
    }))
}

fn other_row(entry: &Worktree, held: Option<&Occupancy>, minute_ms: i64) -> OtherRow {
    let branch = row_branch(entry);
    if entry.missing {
        return OtherRow {
            path: entry.path.clone(),
            branch,
            right: None,
            sub: vec![faint(t::chat::git_folder_missing().to_owned())],
            working: false,
            missing: true,
        };
    }
    let work = entry.work.as_ref();
    let (lead, ahead) = if entry.unrelated {
        (vec![faint(t::chat::git_no_common().to_owned())], None)
    } else {
        let files = work.map(|w| faint(t::chat::changes_file_count(w.files, &w.files.to_string())));
        (files.into_iter().collect(), ahead_part(entry))
    };
    let tail = match held {
        Some(Occupancy::Running { since_ms, more, .. }) => {
            Some(SubPart::Running(running_text(*since_ms, *more, minute_ms)))
        }
        Some(Occupancy::NeedsInput { .. }) => {
            Some(SubPart::Running(t::chat::git_needs_input().to_owned()))
        }
        _ => last_part(
            newer(entry.committed_at, work.and_then(|w| w.changed_at)),
            minute_ms,
        ),
    };
    OtherRow {
        path: entry.path.clone(),
        branch,
        right: work
            .filter(|_| !entry.unrelated)
            .map(|w| (w.added, w.removed)),
        sub: join(vec![
            lead,
            ahead.into_iter().collect(),
            tail.into_iter().collect(),
        ]),
        working: matches!(held, Some(Occupancy::Running { .. })),
        missing: false,
    }
}

fn more_line(worktrees: &Worktrees, hidden: usize) -> Option<String> {
    if worktrees.no_worktree == 0 {
        return match hidden {
            0 => None,
            n => Some(t::chat::git_more_worktrees_only(n as u64, &n.to_string())),
        };
    }
    let count = worktrees.no_worktree;
    let text = count.to_string();
    let default = worktrees
        .default
        .as_deref()
        .map(|default| shown_base(default, Some(default)));
    let checked_out = default.as_deref().is_some_and(|short| {
        worktrees
            .list
            .iter()
            .any(|entry| entry.branch.as_deref() == Some(short))
    });
    let line = match default {
        Some(default) if !checked_out => t::chat::git_default_and(count, &default, &text),
        _ => t::chat::git_other_branches(count, &text),
    };
    Some(match hidden {
        0 => line,
        n => t::chat::git_more_worktrees(n as u64, &n.to_string(), &line),
    })
}

pub fn section(
    status: &GitModel,
    stat: Option<&BranchModel>,
    worktrees: Option<&Worktrees>,
    occupancy: &[Option<Occupancy>],
    minute_ms: i64,
) -> GitSection {
    let entry = worktrees.and_then(|w| w.list.iter().find(|e| e.current));
    let current = current_row(status, stat, worktrees, entry, minute_ms);
    let mut others: Vec<OtherRow> = worktrees
        .map(|w| {
            w.list
                .iter()
                .enumerate()
                .filter(|(_, e)| !e.current)
                .map(|(i, e)| other_row(e, occupancy.get(i).and_then(Option::as_ref), minute_ms))
                .collect()
        })
        .unwrap_or_default();
    others.sort_by(|a, b| b.working.cmp(&a.working).then_with(|| a.path.cmp(&b.path)));
    let running = others.iter().filter(|row| row.working).count();
    let hidden = others.len().saturating_sub(SHOWN_WORKTREES);
    let total = worktrees.map_or(0, |_| 1 + others.len());
    others.truncate(SHOWN_WORKTREES);
    GitSection {
        title: title(running, total),
        current,
        others,
        more: worktrees.and_then(|w| more_line(w, hidden)),
    }
}

#[cfg(test)]
#[path = "worktrees_tests.rs"]
mod tests;
