//! The morning list (Swift `MorningItems`): what the overnight workers left for you, a branch to
//! merge, a question to answer, or a job to retry, built from the poller's reports and the open
//! issues `gh` lists. Issue titles and comments are text from GitHub and from the machines:
//! shown, never run.

use regex::Regex;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

use super::poller::LastReport;
use super::runner::{Call, Runner};
use super::shell;
use super::status::{parse_status_date, NeedsYouEntry, ReadyEntry, StuckEntry};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MorningKind {
    Ready,
    NeedsYou,
    Stuck,
}

/// The worker entry behind a morning item.
#[derive(Clone, Debug, PartialEq)]
pub enum MorningEntry {
    Ready(ReadyEntry),
    NeedsYou(NeedsYouEntry),
    Stuck(StuckEntry),
}

/// One thing the overnight workers left for you.
#[derive(Clone, Debug, PartialEq)]
pub struct MorningItem {
    /// `<machine>/<repo>#<issue>`
    pub id: String,
    pub kind: MorningKind,
    pub machine: String,
    pub repo: String,
    pub issue: i64,
    pub title: String,
    pub entry: MorningEntry,
    /// The latest worker comment on the issue, with the hidden `<!-- sd:… -->` marks removed.
    pub comment: Option<String>,
}

/// One comment on an issue.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GhComment {
    pub author: String,
    pub body: String,
    pub created_at: Option<f64>,
}

impl GhComment {
    pub fn body(body: &str) -> GhComment {
        GhComment {
            body: body.into(),
            ..GhComment::default()
        }
    }
}

/// One issue as `gh issue list --json number,title,labels,comments` prints it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GhIssue {
    pub number: i64,
    pub title: String,
    pub labels: Vec<String>,
    pub comments: Vec<GhComment>,
}

impl GhIssue {
    pub fn new(number: i64, title: &str) -> GhIssue {
        GhIssue {
            number,
            title: title.into(),
            ..GhIssue::default()
        }
    }

    /// Reads the JSON array `gh issue list --json …` prints; empty when it is not one, or when
    /// an element has no integer `number`.
    pub fn decode_list(text: &str) -> Vec<GhIssue> {
        let Ok(Value::Array(all)) = serde_json::from_str::<Value>(text) else {
            return Vec::new();
        };
        all.iter()
            .map(GhIssue::read)
            .collect::<Option<Vec<_>>>()
            .unwrap_or_default()
    }

    fn read(v: &Value) -> Option<GhIssue> {
        let o = v.as_object()?;
        let number = o.get("number")?.as_i64()?;
        let title = o
            .get("title")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        // a label that is not an object with a name is skipped, not fatal
        let labels = match o.get("labels") {
            Some(Value::Array(list)) => list
                .iter()
                .filter_map(|l| l.get("name")?.as_str().map(str::to_string))
                .collect(),
            _ => Vec::new(),
        };
        // one comment that is not an object makes the whole list empty
        let comments = match o.get("comments") {
            Some(Value::Array(list)) => list
                .iter()
                .map(|c| {
                    let c = c.as_object()?;
                    Some(GhComment {
                        author: c
                            .get("author")
                            .and_then(|a| a.get("login"))
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_string(),
                        body: c
                            .get("body")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_string(),
                        created_at: c
                            .get("createdAt")
                            .and_then(Value::as_str)
                            .and_then(parse_status_date),
                    })
                })
                .collect::<Option<Vec<_>>>()
                .unwrap_or_default(),
            _ => Vec::new(),
        };
        Some(GhIssue {
            number,
            title,
            labels,
            comments,
        })
    }
}

pub const WORKER_MARK: &str = "<!-- sd:worker -->";

/// The labels the worker puts on an issue it leaves for you.
pub const LABELS: [&str; 3] = ["sd:ready", "sd:needs-you", "sd:stuck"];

pub fn item_id(machine: &str, repo: &str, issue: i64) -> String {
    format!("{machine}/{repo}#{issue}")
}

/// Ready first, then Needs you, then Stuck. Within a group the oldest comes first: a ready branch
/// by when it became ready, the others (the status carries no date) by issue number, which grows
/// with time. `issues` is keyed by repo (`owner/name`).
pub fn build(
    reports: &BTreeMap<String, LastReport>,
    issues: &BTreeMap<String, Vec<GhIssue>>,
) -> Vec<MorningItem> {
    let mut ready: Vec<(MorningItem, Option<f64>)> = Vec::new();
    let mut needs: Vec<MorningItem> = Vec::new();
    let mut stuck: Vec<MorningItem> = Vec::new();

    // the map's keys are sorted: machines in name order
    for (machine, report) in reports {
        let Some(status) = &report.status else {
            continue;
        };
        let make = |kind: MorningKind, repo: &str, n: i64, entry: MorningEntry| {
            let issue = issues
                .get(repo)
                .and_then(|list| list.iter().find(|i| i.number == n));
            MorningItem {
                id: item_id(machine, repo, n),
                kind,
                machine: machine.clone(),
                repo: repo.into(),
                issue: n,
                title: issue.map_or_else(|| format!("#{n}"), |i| i.title.clone()),
                entry,
                comment: issue.and_then(|i| latest_worker_comment(i, machine)),
            }
        };
        for e in &status.ready {
            ready.push((
                make(
                    MorningKind::Ready,
                    &e.repo,
                    e.issue,
                    MorningEntry::Ready(e.clone()),
                ),
                e.ready_at,
            ));
        }
        for e in &status.needs_you {
            needs.push(make(
                MorningKind::NeedsYou,
                &e.repo,
                e.issue,
                MorningEntry::NeedsYou(e.clone()),
            ));
        }
        for e in &status.stuck {
            stuck.push(make(
                MorningKind::Stuck,
                &e.repo,
                e.issue,
                MorningEntry::Stuck(e.clone()),
            ));
        }
    }

    let by_issue = |a: &MorningItem, b: &MorningItem| {
        (a.issue, &a.machine, &a.repo).cmp(&(b.issue, &b.machine, &b.repo))
    };
    ready.sort_by(|a, b| match (a.1, b.1) {
        (Some(x), Some(y)) if x != y => x.partial_cmp(&y).unwrap_or(std::cmp::Ordering::Equal),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        _ => by_issue(&a.0, &b.0),
    });
    needs.sort_by(by_issue);
    stuck.sort_by(by_issue);
    let mut out: Vec<MorningItem> = ready.into_iter().map(|r| r.0).collect();
    out.extend(needs);
    out.extend(stuck);
    out
}

/// A worker comment ends with the hidden `<!-- sd:worker -->` mark (trailing whitespace allowed)
/// or starts with `<machine>: `.
pub fn is_worker_comment(body: &str, machine: &str) -> bool {
    body.trim_end().ends_with(WORKER_MARK) || body.starts_with(&format!("{machine}: "))
}

/// Removes every `<!-- sd:… -->` mark and the blank space around what is left; `None` when nothing
/// is left.
pub fn clean_comment(body: &str) -> Option<String> {
    let Ok(re) = Regex::new(r"(?s)<!--\s*sd:.*?-->") else {
        return None;
    };
    let text = re.replace_all(body, "");
    let text = text.trim();
    (!text.is_empty()).then(|| text.to_string())
}

pub fn latest_worker_comment(issue: &GhIssue, machine: &str) -> Option<String> {
    issue
        .comments
        .iter()
        .rev()
        .find(|c| is_worker_comment(&c.body, machine))
        .and_then(|c| clean_comment(&c.body))
}

/// A repo as `owner/name`, plain characters only and not starting with `-`: it goes after `-R`.
pub fn is_repo_slug(repo: &str) -> bool {
    let plain = |s: &str| {
        !s.is_empty()
            && !s.starts_with('-')
            && s.chars()
                .all(|c| c.is_ascii_alphanumeric() || "._-".contains(c))
    };
    match repo.split_once('/') {
        Some((owner, name)) => plain(owner) && plain(name),
        None => false,
    }
}

/// The open issues under each label of each repo, merged by issue number. A failing call leaves
/// that label out; the items then show `#<n>` for a title.
pub fn fetch_issues(repos: &[String], runner: &dyn Runner) -> BTreeMap<String, Vec<GhIssue>> {
    fetch_issues_checked(repos, runner).0
}

/// Like [`fetch_issues`], and also names the repos where any `gh` call failed (their issues are
/// then partial, so a caller that holds older issues for them should keep those). A repo name
/// that is not `owner/name` is never given to `gh` and counts as failed.
pub fn fetch_issues_checked(
    repos: &[String],
    runner: &dyn Runner,
) -> (BTreeMap<String, Vec<GhIssue>>, BTreeSet<String>) {
    let mut all = BTreeMap::new();
    let mut failed = BTreeSet::new();
    let unique: BTreeSet<&String> = repos.iter().collect();
    for repo in unique {
        if !is_repo_slug(repo) {
            failed.insert(repo.clone());
            continue;
        }
        let mut by_number: BTreeMap<i64, GhIssue> = BTreeMap::new();
        for label in LABELS {
            let argv = vec![
                shell::tool_path("gh"),
                "issue".into(),
                "list".into(),
                "-R".into(),
                repo.clone(),
                "--label".into(),
                label.into(),
                "--json".into(),
                "number,title,labels,comments".into(),
                "--limit".into(),
                "100".into(),
            ];
            let result = runner.run(&Call::secs(argv, 30));
            if result.code != 0 {
                failed.insert(repo.clone());
                continue;
            }
            for issue in GhIssue::decode_list(&result.out) {
                by_number.insert(issue.number, issue);
            }
        }
        all.insert(repo.clone(), by_number.into_values().collect());
    }
    (all, failed)
}

/// The issues to show after a fetch: the new ones, except for a repo whose fetch failed, which
/// keeps what was last known.
pub fn merged(
    old: &BTreeMap<String, Vec<GhIssue>>,
    fetched: &BTreeMap<String, Vec<GhIssue>>,
    failed: &BTreeSet<String>,
) -> BTreeMap<String, Vec<GhIssue>> {
    let mut out = fetched.clone();
    for repo in failed {
        match old.get(repo).or_else(|| fetched.get(repo)) {
            Some(list) => {
                out.insert(repo.clone(), list.clone());
            }
            None => {
                out.remove(repo);
            }
        }
    }
    out
}
