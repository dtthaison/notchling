use super::github::{allows, GitHubHttp, HttpAnswer, BODY_CAP};
use super::runner::{Call, Runner};
use super::shell;
use crate::accounts::Secret;
use regex::Regex;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, OnceLock};

const API: &str = "https://api.github.com";
const MAX_IN_FLIGHT: usize = 4;
pub(super) const LIST_LIMIT: usize = 1000;
pub(super) const VIEW_PAGES: usize = 10;
const VIEW_DOUBT: &str = "couldn't read the private repos the token reaches";

pub(super) fn is_repo_name(text: &str) -> bool {
    static RE: OnceLock<Option<Regex>> = OnceLock::new();
    let re = RE
        .get_or_init(|| Regex::new(r"^[A-Za-z0-9][A-Za-z0-9-]{0,38}/[A-Za-z0-9._-]{1,100}$").ok());
    let Some(re) = re else { return false };
    if !re.is_match(text) {
        return false;
    }
    let name = text.split('/').nth(1).unwrap_or("");
    name != "." && name != ".."
}

pub(super) fn api_url(path: &str, query: Option<&str>) -> String {
    match query {
        Some(q) => format!("{API}{path}?{q}"),
        None => format!("{API}{path}"),
    }
}

pub(super) struct Req {
    pub name: String,
    pub url: String,
}

pub(super) fn requests(names: &[String], suffix: &str, query: Option<&str>) -> Vec<Req> {
    names
        .iter()
        .map(|name| Req {
            name: name.clone(),
            url: api_url(&format!("/repos/{name}{suffix}"), query),
        })
        .collect()
}

pub(super) fn fetch_all(
    urls: &[String],
    token: &Secret,
    http: &dyn GitHubHttp,
) -> BTreeMap<String, HttpAnswer> {
    let next = AtomicUsize::new(0);
    let answers = Mutex::new(BTreeMap::new());
    std::thread::scope(|scope| {
        for _ in 0..MAX_IN_FLIGHT.min(urls.len()) {
            scope.spawn(|| {
                while let Some(url) = urls.get(next.fetch_add(1, Ordering::SeqCst)) {
                    let answer = http.get(url, token);
                    answers
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .insert(url.clone(), answer);
                }
            });
        }
    });
    answers.into_inner().unwrap_or_else(|e| e.into_inner())
}

pub(super) fn is_denial(a: &HttpAnswer) -> bool {
    (a.status == 403 || a.status == 404) && !rate_limited(a)
}

fn rate_limited(a: &HttpAnswer) -> bool {
    a.status == 429
        || a.header("x-ratelimit-remaining") == Some("0")
        || a.header("retry-after").is_some()
        || String::from_utf8_lossy(&a.body)
            .to_lowercase()
            .contains("rate limit")
}

pub(super) struct Listing {
    pub others: Vec<String>,
    pub served_missing: Vec<String>,
    pub skipped_lines: usize,
    pub maybe_cut: bool,
}

fn owners(repos: &[String]) -> Vec<String> {
    let mut seen = BTreeSet::new();
    let mut out = Vec::new();
    for repo in repos {
        let owner = repo.split('/').next().unwrap_or("");
        if seen.insert(owner.to_lowercase()) {
            out.push(owner.to_string());
        }
    }
    out
}

pub(super) fn private_listing(served: &[String], runner: &dyn Runner) -> Result<Listing, String> {
    let served_keys: BTreeSet<String> = served.iter().map(|s| s.to_lowercase()).collect();
    let mut seen = BTreeSet::new();
    let mut others = BTreeSet::new();
    let (mut skipped, mut cut) = (0, false);
    for owner in owners(served) {
        let argv = vec![
            shell::tool_path("gh"),
            "repo".into(),
            "list".into(),
            owner.clone(),
            "--visibility".into(),
            "private".into(),
            "--limit".into(),
            LIST_LIMIT.to_string(),
            "--json".into(),
            "nameWithOwner".into(),
            "--jq".into(),
            ".[].nameWithOwner".into(),
        ];
        let result = runner.run(&Call::secs(argv, 60));
        if result.code != 0 {
            return Err(format!(
                "couldn't list the private repos of {owner} with gh"
            ));
        }
        let mut count = 0;
        for line in result.out.split(['\n', '\r']) {
            let name = line.trim_matches([' ', '\t']);
            if name.is_empty() {
                continue;
            }
            count += 1;
            if !is_repo_name(name) {
                skipped += 1;
                continue;
            }
            seen.insert(name.to_lowercase());
            if !served_keys.contains(&name.to_lowercase()) {
                others.insert(name.to_string());
            }
        }
        if count >= LIST_LIMIT {
            cut = true;
        }
    }
    Ok(Listing {
        others: others.into_iter().collect(),
        served_missing: served
            .iter()
            .filter(|s| !seen.contains(&s.to_lowercase()))
            .cloned()
            .collect(),
        skipped_lines: skipped,
        maybe_cut: cut,
    })
}

#[derive(Default)]
pub(super) struct TokenView {
    pub names: Vec<String>,
    pub incomplete: Option<String>,
    pub refused: bool,
}

pub(super) fn token_view(token: &Secret, http: &dyn GitHubHttp) -> TokenView {
    let mut view = TokenView::default();
    let mut url = api_url("/user/repos", Some("visibility=private&per_page=50"));
    for page in 1..=VIEW_PAGES {
        let a = http.get(&url, token);
        if a.status == 401 {
            view.refused = true;
            return view;
        }
        if a.status != 200 {
            view.incomplete = Some(VIEW_DOUBT.into());
            return view;
        }
        let cut = a.body.len() >= BODY_CAP;
        match repo_names_in_page(&a.body, cut) {
            Some((names, unreadable)) => {
                view.names.extend(names);
                if unreadable > 0 && view.incomplete.is_none() {
                    view.incomplete = Some(format!(
                        "the token's list of private repos has {unreadable} name{} this check can't read",
                        if unreadable == 1 { "" } else { "s" }
                    ));
                }
            }
            None => {
                view.incomplete = Some(VIEW_DOUBT.into());
                return view;
            }
        }
        if cut {
            view.incomplete = Some("the token's list of private repos was cut off".into());
            return view;
        }
        let Some(link) = a.header("link").filter(|l| l.contains(r#"rel="next""#)) else {
            return view;
        };
        let next = next_link(link)
            .filter(|n| allows(n).is_some_and(|p| p.split('?').next() == Some("/user/repos")));
        let Some(next) = next else {
            view.incomplete = Some(VIEW_DOUBT.into());
            return view;
        };
        if page == VIEW_PAGES {
            view.incomplete = Some(format!(
                "the token lists more than {VIEW_PAGES} pages of private repos"
            ));
            return view;
        }
        url = next;
    }
    view
}

fn repo_names_in_page(body: &[u8], cut: bool) -> Option<(Vec<String>, usize)> {
    if !cut {
        let serde_json::Value::Array(list) = serde_json::from_slice(body).ok()? else {
            return None;
        };
        let (mut names, mut unreadable) = (Vec::new(), 0);
        for item in &list {
            match item.get("full_name").and_then(|n| n.as_str()) {
                Some(name) if is_repo_name(name) => names.push(name.to_string()),
                _ => unreadable += 1,
            }
        }
        return Some((names, unreadable));
    }
    let text = String::from_utf8_lossy(body);
    let re = Regex::new(r#""full_name"\s*:\s*"([^"]+)""#).ok()?;
    let found: Vec<String> = re.captures_iter(&text).map(|c| c[1].to_string()).collect();
    let unreadable = found.iter().filter(|n| !is_repo_name(n)).count();
    Some((
        found.into_iter().filter(|n| is_repo_name(n)).collect(),
        unreadable,
    ))
}

fn next_link(header: &str) -> Option<String> {
    let re = Regex::new(r#"<([^>]*)>\s*;\s*rel="next""#).ok()?;
    re.captures(header).map(|c| c[1].to_string())
}
