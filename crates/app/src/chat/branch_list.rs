use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use notchling_engine::git::guard::clean_ref;
use notchling_engine::git::refs::{plain_label, SUBJECT_CHARS};
use notchling_locale as t;
use serde_json::{json, Value};

use super::branch::matches;
use super::changes_target::Target;
use super::format::age;
use super::worktrees::{
    faint, join, parse_work, shown_base, upstream_part, Occupancy, Open, SubPart, Work,
};
use crate::acting::Request;

pub const BRANCH_LIST_OP: &str = "git.branch_list";

const JUST_NOW_MS: i64 = 60_000;
const HEAD_CHARS: usize = 7;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Lines {
    pub added: u64,
    pub removed: u64,
    pub files: u64,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct WorktreeOf {
    pub path: String,
    pub real: String,
    pub main: bool,
    pub current: bool,
    pub missing: bool,
    pub work: Option<Work>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Branch {
    pub name: Option<String>,
    pub reference: Option<String>,
    pub local: bool,
    pub remote: Option<String>,
    pub remote_only: bool,
    pub on_github: bool,
    pub is_base: bool,
    pub worktree: Option<WorktreeOf>,
    pub head: Option<String>,
    pub detached: bool,
    pub committed_at: Option<i64>,
    pub subject: Option<String>,
    pub upstream: Option<String>,
    pub upstream_ahead: u64,
    pub upstream_behind: u64,
    pub upstream_gone: bool,
    pub base: Option<String>,
    pub ahead: Option<u64>,
    pub behind: Option<u64>,
    pub unrelated: bool,
    pub merged_into: Option<String>,
    pub lines: Option<Lines>,
    pub pending: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BranchList {
    pub name: String,
    pub default: Option<String>,
    pub integration: Option<String>,
    pub fetched_at: Option<i64>,
    pub truncated: bool,
    pub pending: u64,
    pub branches: Vec<Branch>,
}

fn parse_lines(value: &Value) -> Option<Lines> {
    value.as_object()?;
    let number = |key: &str| value[key].as_u64().unwrap_or(0);
    Some(Lines {
        added: number("added"),
        removed: number("removed"),
        files: number("files"),
    })
}

fn parse_worktree_of(value: &Value) -> Option<WorktreeOf> {
    let path = value["path"].as_str().filter(|p| !p.is_empty())?;
    let flag = |key: &str| value[key].as_bool().unwrap_or(false);
    Some(WorktreeOf {
        path: path.to_owned(),
        real: value["real"].as_str().unwrap_or(path).to_owned(),
        main: flag("main"),
        current: flag("current"),
        missing: flag("missing"),
        work: parse_work(&value["work"]),
    })
}

fn parse_branch(row: &Value) -> Option<Branch> {
    let text = |key: &str| row[key].as_str().map(str::to_owned);
    let flag = |key: &str| row[key].as_bool().unwrap_or(false);
    let number = |key: &str| row[key].as_u64().unwrap_or(0);
    let reference = text("ref");
    let worktree = parse_worktree_of(&row["worktree"]);
    if reference.is_none() && worktree.is_none() {
        return None;
    }
    Some(Branch {
        name: text("name"),
        reference,
        local: flag("local"),
        remote: text("remote"),
        remote_only: flag("remote_only"),
        on_github: flag("on_github"),
        is_base: flag("is_base"),
        worktree,
        head: text("head"),
        detached: flag("detached"),
        committed_at: row["committed_at"].as_i64(),
        subject: text("subject"),
        upstream: text("upstream"),
        upstream_ahead: number("upstream_ahead"),
        upstream_behind: number("upstream_behind"),
        upstream_gone: flag("upstream_gone"),
        base: text("base"),
        ahead: row["ahead"].as_u64(),
        behind: row["behind"].as_u64(),
        unrelated: flag("unrelated"),
        merged_into: text("merged_into"),
        lines: parse_lines(&row["lines"]),
        pending: flag("pending"),
    })
}

pub fn parse_branch_list(reply: &Value) -> Option<BranchList> {
    if reply["repo"] != json!(true) {
        return None;
    }
    let text = |key: &str| reply[key].as_str().map(str::to_owned);
    Some(BranchList {
        name: text("name").unwrap_or_default(),
        default: text("default"),
        integration: text("integration"),
        fetched_at: reply["fetched_at"].as_i64(),
        truncated: reply["truncated"].as_bool().unwrap_or(false),
        pending: reply["pending"].as_u64().unwrap_or(0),
        branches: reply["branches"]
            .as_array()
            .map(|rows| rows.iter().filter_map(parse_branch).collect())
            .unwrap_or_default(),
    })
}

pub fn branch_list_request(cwd: &str) -> Request {
    Request {
        op: BRANCH_LIST_OP,
        args: json!({ "cwd": cwd }),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tab {
    Working,
    NotMerged,
    Merged,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Group {
    Worktrees,
    NoWorktree,
}

fn uncommitted(b: &Branch) -> bool {
    b.worktree
        .as_ref()
        .and_then(|w| w.work.as_ref())
        .is_some_and(|w| w.uncommitted_files > 0)
}

pub fn in_tab(b: &Branch, tab: Tab) -> bool {
    match tab {
        Tab::Working => b.worktree.is_some() || b.is_base,
        Tab::NotMerged => !b.is_base && (b.merged_into.is_none() || uncommitted(b)),
        Tab::Merged => {
            let settled = match &b.worktree {
                None => true,
                Some(w) => w.work.as_ref().is_some_and(|w| w.uncommitted_files == 0),
            };
            !b.is_base && b.merged_into.is_some() && settled
        }
    }
}

pub fn tab_counts(list: &BranchList) -> [usize; 3] {
    let count = |tab| list.branches.iter().filter(|b| in_tab(b, tab)).count();
    [
        count(Tab::Working),
        count(Tab::NotMerged),
        count(Tab::Merged),
    ]
}

pub fn tab_label(tab: Tab, count: Option<usize>) -> String {
    let name = match tab {
        Tab::Working => t::chat::branches_tab_working(),
        Tab::NotMerged => t::chat::branches_tab_not_merged(),
        Tab::Merged => t::chat::branches_tab_merged(),
    };
    match count {
        Some(count) => t::chat::branches_tab_count(name, &count.to_string()),
        None => name.to_owned(),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Right {
    Lines(u64, u64),
    Pending,
    Empty,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Buttons {
    pub changes: bool,
    pub open_session: bool,
    pub finder: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PanelRow {
    pub key: String,
    pub cloud: bool,
    pub name: String,
    pub right: Right,
    pub sub: Vec<SubPart>,
    pub current: bool,
    pub working: bool,
    pub buttons: Buttons,
    pub target: Option<Target>,
    pub open: Option<Open>,
    pub reveal: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PanelState {
    Loading,
    Ready,
    Failed(String),
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PanelInput {
    pub list: Option<Arc<BranchList>>,
    pub failed: Option<String>,
    pub occupancy: Vec<(String, Option<Occupancy>)>,
    pub session_names: HashMap<String, String>,
    pub minute_ms: i64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PanelModel {
    pub repo: String,
    pub fetched: String,
    pub tabs: [(Tab, Option<usize>); 3],
    pub groups: Vec<(Group, Vec<PanelRow>)>,
    pub note: Option<String>,
    pub state: PanelState,
    pub truncated: bool,
}

struct Ctx<'a> {
    list: &'a BranchList,
    input: &'a PanelInput,
    main: Option<(&'a str, &'a str)>,
    held: HashMap<&'a str, &'a Occupancy>,
}

pub fn held_open(held: &Occupancy) -> &Open {
    match held {
        Occupancy::Running { open, .. }
        | Occupancy::NeedsInput { open, .. }
        | Occupancy::Idle { open } => open,
    }
}

pub fn open_id(open: &Open) -> &str {
    match open {
        Open::Session(id) | Open::Subagent(id) => id,
    }
}

fn display_name(b: &Branch) -> String {
    if let Some(name) = b.name.as_deref().filter(|n| !n.is_empty()) {
        return name.to_owned();
    }
    match b.head.as_deref().filter(|h| !h.is_empty()) {
        Some(head) => t::chat::git_detached_at(&head.chars().take(HEAD_CHARS).collect::<String>()),
        None => t::chat::branches_detached().to_owned(),
    }
}

fn row_key(b: &Branch) -> String {
    match (&b.reference, &b.worktree) {
        (Some(reference), _) => reference.clone(),
        (None, Some(w)) => format!("worktree:{}", w.path),
        (None, None) => display_name(b),
    }
}

fn relative(path: &str, real: &str, main: Option<(&str, &str)>) -> Option<String> {
    let (main_path, main_real) = main?;
    [
        (path, main_path),
        (real, main_real),
        (path, main_real),
        (real, main_path),
    ]
    .into_iter()
    .find_map(|(own, root)| {
        let rest = Path::new(own).strip_prefix(root).ok()?;
        (!rest.as_os_str().is_empty()).then(|| rest.to_string_lossy().into_owned())
    })
}

fn where_text(ctx: &Ctx, b: &Branch) -> String {
    if let Some(w) = &b.worktree {
        if w.main {
            return t::chat::branches_main_checkout().to_owned();
        }
        return relative(&w.path, &w.real, ctx.main).unwrap_or_else(|| w.path.clone());
    }
    if b.remote_only {
        if b.on_github {
            return t::chat::branches_github_only().to_owned();
        }
        if let Some(remote) = b.remote.as_deref() {
            return t::chat::branches_remote_only(remote);
        }
    }
    t::chat::branches_local().to_owned()
}

fn session_part(ctx: &Ctx, held: Option<&Occupancy>) -> Option<SubPart> {
    let held = held?;
    let name = ctx.input.session_names.get(open_id(held_open(held)))?;
    Some(match held {
        Occupancy::Running { .. } => SubPart::Running(t::chat::branches_session(
            name,
            t::chat::branches_state_running(),
        )),
        Occupancy::NeedsInput { .. } => {
            SubPart::Running(t::chat::branches_session(name, t::chat::git_needs_input()))
        }
        Occupancy::Idle { .. } => faint(t::chat::branches_session(
            name,
            t::chat::branches_state_idle(),
        )),
    })
}

fn last_commit(b: &Branch, minute_ms: i64) -> Option<SubPart> {
    let since = b.committed_at.filter(|since| *since > 0)?;
    let subject = b
        .subject
        .as_deref()
        .map(|s| plain_label(s, SUBJECT_CHARS))
        .filter(|s| !s.is_empty());
    let now = minute_ms - since < JUST_NOW_MS;
    Some(faint(match (now, subject) {
        (true, Some(subject)) => t::chat::branches_last_commit_now_subject(&subject),
        (true, None) => t::chat::branches_last_commit_now().to_owned(),
        (false, Some(subject)) => {
            t::chat::branches_last_commit_subject(&age(minute_ms, since), &subject)
        }
        (false, None) => t::chat::branches_last_commit(&age(minute_ms, since)),
    }))
}

fn push_part(b: &Branch) -> Option<SubPart> {
    if b.remote_only || b.detached {
        return None;
    }
    if b.upstream_gone {
        return Some(faint(t::chat::git_upstream_gone().to_owned()));
    }
    Some(upstream_part(
        b.name.as_deref(),
        b.upstream.as_deref(),
        b.upstream_ahead,
        b.upstream_behind,
    ))
}

fn merge_part(ctx: &Ctx, b: &Branch) -> Option<SubPart> {
    if b.is_base {
        return None;
    }
    Some(faint(match b.merged_into.as_deref() {
        Some(base) => t::chat::branches_merged_into(&shown_base(base, ctx.list.default.as_deref())),
        None => t::chat::branches_not_merged().to_owned(),
    }))
}

fn finished(b: &Branch) -> bool {
    b.is_base || b.merged_into.is_some()
}

fn files_part(b: &Branch) -> Option<SubPart> {
    if finished(b) {
        return None;
    }
    if b.unrelated {
        return Some(faint(t::chat::git_no_common().to_owned()));
    }
    let files = b
        .worktree
        .as_ref()
        .and_then(|w| w.work.as_ref())
        .map(|w| w.files)
        .or(b.lines.map(|l| l.files))?;
    Some(faint(t::chat::changes_file_count(
        files,
        &files.to_string(),
    )))
}

fn ahead_part(b: &Branch) -> Option<SubPart> {
    if finished(b) || b.unrelated {
        return None;
    }
    b.base.as_ref()?;
    let ahead = b.ahead?;
    Some(faint(match b.behind.unwrap_or(0) {
        0 => t::chat::git_base_ahead(&ahead.to_string()),
        behind => t::chat::git_base_ahead_behind(&ahead.to_string(), &behind.to_string()),
    }))
}

fn right(b: &Branch) -> Right {
    if b.is_base || b.unrelated || (b.merged_into.is_some() && !uncommitted(b)) {
        return Right::Empty;
    }
    if let Some(w) = b.worktree.as_ref().and_then(|w| w.work.as_ref()) {
        return Right::Lines(w.added, w.removed);
    }
    match b.lines {
        Some(l) => Right::Lines(l.added, l.removed),
        None if b.pending => Right::Pending,
        None => Right::Empty,
    }
}

fn sub_line(ctx: &Ctx, b: &Branch, held: Option<&Occupancy>, current: bool) -> Vec<SubPart> {
    let one = |part: Option<SubPart>| part.into_iter().collect::<Vec<_>>();
    let this_session = current.then(|| faint(t::chat::git_this_session().to_owned()));
    let dirty = b
        .worktree
        .as_ref()
        .and_then(|w| w.work.as_ref())
        .filter(|w| w.uncommitted_files > 0)
        .map(|w| faint(t::chat::git_uncommitted(&w.uncommitted_files.to_string())));
    join(vec![
        vec![faint(where_text(ctx, b))],
        one(this_session),
        one(session_part(ctx, held)),
        one(dirty),
        one(last_commit(b, ctx.input.minute_ms)),
        one(push_part(b)),
        one(merge_part(ctx, b)),
        one(files_part(b)),
        one(ahead_part(b)),
    ])
}

fn build_row(ctx: &Ctx, b: &Branch) -> PanelRow {
    let wt = b.worktree.as_ref();
    let held = wt.and_then(|w| ctx.held.get(w.path.as_str()).copied());
    let current = wt.is_some_and(|w| w.current);
    let missing = wt.is_some_and(|w| w.missing);
    let clean = [b.name.as_deref(), b.reference.as_deref()]
        .into_iter()
        .flatten()
        .all(|name| clean_ref(name).is_ok());
    let target = match (wt, &b.reference) {
        _ if missing || !clean => None,
        (Some(_), _) if current => Some(Target::Own),
        (Some(w), _) => Some(Target::Worktree(w.path.clone())),
        (None, Some(reference)) => Some(Target::Ref(reference.clone())),
        (None, None) => None,
    };
    let open = held.map(|held| held_open(held).clone());
    PanelRow {
        key: row_key(b),
        cloud: b.remote_only,
        name: display_name(b),
        right: right(b),
        sub: sub_line(ctx, b, held, current),
        current,
        working: matches!(held, Some(Occupancy::Running { .. })),
        buttons: Buttons {
            changes: target.is_some(),
            open_session: open.is_some(),
            finder: wt.is_some() && !missing,
        },
        target,
        open,
        reveal: wt.filter(|_| !missing).map(|w| w.path.clone()),
    }
}

fn haystack(ctx: &Ctx, b: &Branch, row: &PanelRow) -> String {
    let session = b
        .worktree
        .as_ref()
        .and_then(|w| ctx.held.get(w.path.as_str()))
        .and_then(|held| ctx.input.session_names.get(open_id(held_open(held))));
    let subject = b.subject.as_deref().map(|s| plain_label(s, SUBJECT_CHARS));
    [
        Some(row.name.as_str()),
        b.worktree.as_ref().map(|w| w.path.as_str()),
        session.map(String::as_str),
        subject.as_deref(),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>()
    .join(" ")
}

fn fetched_line(list: &BranchList, minute_ms: i64) -> String {
    match list.fetched_at.filter(|at| *at > 0) {
        None => t::chat::branches_never_fetched().to_owned(),
        Some(at) if minute_ms - at < JUST_NOW_MS => t::chat::branches_fetched_now().to_owned(),
        Some(at) => t::chat::branches_fetched(&age(minute_ms, at)),
    }
}

fn without_list(input: &PanelInput) -> PanelModel {
    let (state, note) = match &input.failed {
        Some(reason) => (
            PanelState::Failed(reason.clone()),
            t::chat::branches_cant_read(),
        ),
        None => (PanelState::Loading, t::chat::branches_reading()),
    };
    PanelModel {
        repo: String::new(),
        fetched: String::new(),
        tabs: [
            (Tab::Working, None),
            (Tab::NotMerged, None),
            (Tab::Merged, None),
        ],
        groups: Vec::new(),
        note: Some(note.to_owned()),
        state,
        truncated: false,
    }
}

type Placed<'a> = Vec<(&'a Branch, PanelRow)>;

fn sorted(mut worktrees: Placed, mut loose: Placed) -> Vec<(Group, Vec<PanelRow>)> {
    let path = |b: &Branch| b.worktree.as_ref().map(|w| w.path.clone());
    worktrees.sort_by(|(a, x), (b, y)| {
        y.current
            .cmp(&x.current)
            .then(y.working.cmp(&x.working))
            .then_with(|| path(a).cmp(&path(b)))
    });
    loose.sort_by(|(a, x), (b, y)| {
        b.committed_at
            .cmp(&a.committed_at)
            .then_with(|| x.name.cmp(&y.name))
    });
    [(Group::Worktrees, worktrees), (Group::NoWorktree, loose)]
        .into_iter()
        .filter(|(_, rows)| !rows.is_empty())
        .map(|(group, rows)| (group, rows.into_iter().map(|(_, row)| row).collect()))
        .collect()
}

fn note_for(list: &BranchList, tab: Tab, query: &str, empty: bool) -> Option<&'static str> {
    let typed = !query.trim().is_empty();
    let worktrees = list
        .branches
        .iter()
        .filter(|b| b.worktree.is_some())
        .count();
    if empty && typed {
        Some(t::chat::branches_no_match())
    } else if empty {
        Some(t::chat::branches_empty())
    } else if tab == Tab::Working && !typed && worktrees <= 1 {
        Some(t::chat::branches_no_other_worktrees())
    } else if list.truncated {
        Some(t::chat::branches_truncated())
    } else {
        None
    }
}

pub fn panel(input: &PanelInput, tab: Tab, query: &str) -> PanelModel {
    let Some(list) = input.list.as_deref() else {
        return without_list(input);
    };
    let ctx = Ctx {
        list,
        input,
        main: list
            .branches
            .iter()
            .filter_map(|b| b.worktree.as_ref())
            .find(|w| w.main)
            .map(|w| (w.path.as_str(), w.real.as_str())),
        held: input
            .occupancy
            .iter()
            .filter_map(|(path, held)| Some((path.as_str(), held.as_ref()?)))
            .collect(),
    };
    let counts = tab_counts(list);
    let mut worktrees: Placed = Vec::new();
    let mut loose: Placed = Vec::new();
    for b in list.branches.iter().filter(|b| in_tab(b, tab)) {
        let row = build_row(&ctx, b);
        if !matches(&haystack(&ctx, b, &row), query) {
            continue;
        }
        if b.worktree.is_some() {
            worktrees.push((b, row));
        } else {
            loose.push((b, row));
        }
    }
    let groups = sorted(worktrees, loose);
    let note = note_for(list, tab, query, groups.is_empty());
    PanelModel {
        repo: list.name.clone(),
        fetched: fetched_line(list, input.minute_ms),
        tabs: [
            (Tab::Working, Some(counts[0])),
            (Tab::NotMerged, Some(counts[1])),
            (Tab::Merged, Some(counts[2])),
        ],
        groups,
        note: note.map(str::to_owned),
        state: PanelState::Ready,
        truncated: list.truncated,
    }
}

#[cfg(test)]
#[path = "branch_list_tests.rs"]
mod tests;
