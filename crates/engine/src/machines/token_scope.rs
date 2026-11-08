//! Step 14: which repositories the issues token reaches and when it expires (Swift `TokenScope`).
//! The probe runs on this Mac and the token never leaves the app: it is read from the vault,
//! compared in memory with the other machines' tokens and sent only in the Authorization header of
//! requests to `https://api.github.com` (through [`super::step::StepContext::http`]). No result,
//! row or message holds it.

use super::github::HttpAnswer;
use super::registry::SdWorkerSettings;
use super::step::{
    Cadence, CheckResult, CheckState, FixOutcome, FixPlan, Probe, SetupStep, StepContext, StepFix,
    StepId, SubRow,
};
use super::token_creation;
use super::token_scope_net::{
    fetch_all, is_denial, is_repo_name, private_listing, requests, token_view, Listing, Req,
    TokenView, LIST_LIMIT,
};
use crate::vault::expiry::{self, Status};
use std::collections::{BTreeMap, BTreeSet};

const REFUSED: &str = "GitHub refused the token: expired or revoked";
/// Names shown in a message before `…`.
const NAMED_REPOS: usize = 3;

/// Issues access on each served repo, nothing on any other private repo (unless the job allows all
/// repos), no Contents access, a value no other machine uses, and an expiry that has not passed.
///
/// The Fix opens GitHub's token page and the vault form through the context's
/// [`super::step::Opener`]; tests pass a recording one and never open anything.
pub fn step(m: &super::registry::Machine, job: &SdWorkerSettings) -> SetupStep {
    let repos: Vec<String> = job.repos.iter().map(|r| r.github.clone()).collect();
    let all_repos = job.token_all_repos == Some(true);
    let scope = if all_repos {
        "Open GitHub's token page (any repositories; Issues: read and write, Metadata: read, no Contents) ".to_string()
    } else {
        format!("Open GitHub's token page for {} (Issues: read and write, Metadata: read, nothing else) ", repos.join(", "))
    };
    let display = format!(
        "{scope}and ask you to save the new token in the Secret Vault as {}. It is never shown.",
        job.issues_secret
    );
    let (secret, machine) = (job.issues_secret.clone(), m.name.clone());
    let fix = FixPlan::new(display, false, move |ctx| {
        // A new token is the one clean fix for a shared, mis-scoped or expired one.
        token_creation::start(
            &*ctx.step.opener,
            &secret,
            &machine,
            &repos,
            all_repos,
            ctx.step.now,
        );
        // Only the page opens here (the form is the vault's own). The step is not done until a check passes, so this
        // is not ok: Set up all stops at it (and the window shows the line as guidance).
        FixOutcome::fail(format!("Opened GitHub's token page. Fix the token, save it in the Secret Vault as {secret}, then Check again."))
    });
    let job = job.clone();
    SetupStep::new(
        StepId::TOKEN_SCOPE,
        "Token scope",
        vec![],
        Cadence::Slow,
        Probe::local(move |ctx| check(ctx, &job)),
        StepFix::Run(fix),
    )
}

/// The day (`YYYY-MM-DD`) in a `github-authentication-token-expiration` header such as
/// `2027-02-01 12:00:00 +0700`: its first 10 characters when they are a real day.
pub fn expiry_day(header: &str) -> Option<String> {
    expiry::parse(&header.chars().take(10).collect::<String>())
}

// MARK: the check

enum Outcome {
    Ok(String),
    Fail(String),
    Unknown(String),
}

impl Outcome {
    fn row(&self) -> (bool, &str) {
        match self {
            Outcome::Ok(d) => (true, d),
            Outcome::Fail(d) | Outcome::Unknown(d) => (false, d),
        }
    }
}

type Answers = BTreeMap<String, HttpAnswer>;

fn plural(n: usize, one: &str) -> String {
    format!("{n} {one}{}", if n == 1 { "" } else { "s" })
}

fn check(ctx: &StepContext, job: &SdWorkerSettings) -> CheckResult {
    let secret_name = &job.issues_secret;
    let token = ctx.secrets.value(secret_name).filter(|t| !t.is_empty());
    let Some(token) = token.filter(|_| !secret_name.is_empty()) else {
        return CheckResult::cant_tell(format!(
            "{} is not in the vault",
            if secret_name.is_empty() {
                "the issues token"
            } else {
                secret_name
            }
        ));
    };
    let served: Vec<String> = job.repos.iter().map(|r| r.github.clone()).collect();
    let all_repos = job.token_all_repos == Some(true);
    if served.is_empty() || !served.iter().all(|s| is_repo_name(s)) {
        return CheckResult::cant_tell("the served repos are not all owner/name");
    }

    // Every private repo of the served owners that is not served, from the user's own gh.
    let listing = private_listing(&served, &*ctx.runner);

    let issues_reqs = requests(&served, "/issues", Some("per_page=1"));
    let commits_reqs = requests(&served, "/commits", Some("per_page=1"));
    let other_reqs = match &listing {
        Ok(l) => requests(&l.others, "", None),
        Err(_) => Vec::new(),
    };
    // The token's own list of private repos, before the other probes so that at most 4 requests
    // run at once.
    let view = token_view(&token, &*ctx.http);
    let urls: Vec<String> = issues_reqs
        .iter()
        .chain(&commits_reqs)
        .chain(&other_reqs)
        .map(|r| r.url.clone())
        .collect();
    let answers = fetch_all(&urls, &token, &*ctx.http);

    // A 401 anywhere means nothing else it said can be trusted.
    if view.refused || answers.values().any(|a| a.status == 401) {
        return CheckResult::fail(REFUSED);
    }

    let header = issues_reqs
        .iter()
        .filter_map(|r| answers.get(&r.url))
        .filter(|a| a.status == 200)
        .find_map(|a| a.header("github-authentication-token-expiration"))
        .map(str::to_string);
    let day = header.as_deref().and_then(expiry_day);

    let outcomes = vec![
        ("Reaches issues", issues_outcome(&issues_reqs, &answers)),
        (
            "Denied elsewhere",
            denied_outcome(&listing, &other_reqs, &answers, &view, &served, all_repos),
        ),
        ("No code access", contents_outcome(&commits_reqs, &answers)),
        (
            "Distinct token",
            distinct_outcome(token.expose(), secret_name, ctx),
        ),
        (
            "Expiry",
            expiry_outcome(header.as_deref(), day.as_deref(), ctx.now),
        ),
    ];
    let state = summary(&outcomes, &served, day.as_deref(), all_repos);
    let rows = outcomes
        .iter()
        .map(|(name, o)| {
            let (ok, detail) = o.row();
            SubRow::new(name, ok, detail)
        })
        .collect();
    let mut result = CheckResult::of(state).rows(rows);
    result.detected.token_expires = day;
    result
}

fn summary(
    outcomes: &[(&str, Outcome)],
    served: &[String],
    day: Option<&str>,
    all_repos: bool,
) -> CheckState {
    let (mut fails, mut unknowns) = (Vec::new(), Vec::new());
    for (_, outcome) in outcomes {
        match outcome {
            Outcome::Fail(t) => fails.push(t.as_str()),
            Outcome::Unknown(t) => unknowns.push(t.as_str()),
            Outcome::Ok(_) => {}
        }
    }
    if !fails.is_empty() {
        return CheckState::Fail(fails.join("; "));
    }
    if !unknowns.is_empty() {
        return CheckState::CantTell(unknowns.join("; "));
    }
    let reach = if all_repos {
        format!(
            "issues on {}, all repositories by choice",
            plural(served.len(), "served repo")
        )
    } else if served.len() == 1 {
        format!("issues on {} only", served[0])
    } else {
        format!("issues on {} repos only", served.len())
    };
    CheckState::Pass(format!(
        "{reach}, no code access{}",
        day.map_or(String::new(), |d| format!(", expires {d}"))
    ))
}

fn issues_outcome(reqs: &[Req], answers: &Answers) -> Outcome {
    let (mut failed, mut unknown) = (Vec::new(), false);
    for req in reqs {
        let Some(a) = answers.get(&req.url) else {
            unknown = true;
            continue;
        };
        if a.status == 200 {
        } else if is_denial(a) {
            failed.push(req.name.as_str());
        } else {
            unknown = true;
        }
    }
    if !failed.is_empty() {
        return Outcome::Fail(format!("no issues access to {}", failed.join(", ")));
    }
    if unknown {
        return Outcome::Unknown("couldn't read the issues of every served repo".into());
    }
    // the check reads; a write would change GitHub. sd's labels row proves the write once the
    // labels exist.
    Outcome::Ok(format!(
        "can read issues of {} (a write is not tried here)",
        plural(reqs.len(), "repo")
    ))
}

/// Two independent views of "no other private repo": a probe of each repo gh lists, and the
/// token's own list. Either one seeing an unserved repo is a fail; a doubt in either (and no fail)
/// is can't tell. When the job allows all repositories, seeing other repos is accepted and so is
/// any doubt about them: the outcome is ok and says how many were seen.
fn denied_outcome(
    listing: &Result<Listing, String>,
    reqs: &[Req],
    answers: &Answers,
    view: &TokenView,
    served: &[String],
    all_repos: bool,
) -> Outcome {
    let (mut fails, mut unknowns) = (Vec::new(), Vec::new());
    let mut seen_others: BTreeSet<String> = BTreeSet::new();

    match listing {
        Err(message) => unknowns.push(message.clone()),
        Ok(l) => {
            let (mut visible, mut unknown) = (Vec::new(), 0);
            for req in reqs {
                let Some(a) = answers.get(&req.url) else {
                    unknown += 1;
                    continue;
                };
                if a.status == 200 {
                    visible.push(req.name.clone());
                } else if !is_denial(a) {
                    unknown += 1;
                }
            }
            for v in &visible {
                seen_others.insert(v.to_lowercase());
            }
            if !visible.is_empty() {
                fails.push(format!(
                    "can see {} ({})",
                    plural(visible.len(), "other private repo"),
                    shown(&visible)
                ));
            }
            // gh must list the served repos (they are private): if it doesn't, it is looking at
            // the wrong account or cannot see private repos, and an empty "others" list would
            // prove nothing.
            if !l.served_missing.is_empty() {
                unknowns.push(format!(
                    "gh doesn't list {} among the private repos it can see",
                    l.served_missing.join(", ")
                ));
            } else if l.skipped_lines > 0 {
                unknowns.push(format!(
                    "gh listed {} lines that are not repo names",
                    l.skipped_lines
                ));
            } else if l.maybe_cut {
                unknowns.push(format!(
                    "gh may have cut its list of private repos at {LIST_LIMIT}"
                ));
            } else if unknown > 0 {
                unknowns.push(format!(
                    "couldn't check {unknown} of {} other private repos",
                    reqs.len()
                ));
            }
        }
    }

    // The token's own list: a fine-grained token lists only the repos it can reach.
    let served_keys: BTreeSet<String> = served.iter().map(|s| s.to_lowercase()).collect();
    let mut extra: Vec<String> = view
        .names
        .iter()
        .filter(|n| !served_keys.contains(&n.to_lowercase()))
        .cloned()
        .collect();
    extra.sort();
    let seen_keys: BTreeSet<String> = view.names.iter().map(|n| n.to_lowercase()).collect();
    let not_shown: Vec<&String> = served
        .iter()
        .filter(|s| !seen_keys.contains(&s.to_lowercase()))
        .collect();
    for e in &extra {
        seen_others.insert(e.to_lowercase());
    }
    if !extra.is_empty() {
        fails.push(format!(
            "token reaches {} (e.g. {}); set Only select repositories",
            plural(extra.len(), "other private repo"),
            shown(&extra)
        ));
    } else if let Some(doubt) = &view.incomplete {
        unknowns.push(doubt.clone());
    } else if !not_shown.is_empty() {
        // The served repos are private and the issues probe reaches them, so a whole list that
        // leaves one out is not the whole truth: it proves nothing about the others either.
        unknowns.push(format!(
            "the token's list of private repos doesn't show {}",
            not_shown
                .iter()
                .map(|s| s.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }

    if all_repos {
        return Outcome::Ok(format!(
            "reaches all repositories, by choice ({})",
            plural(seen_others.len(), "other private repo")
        ));
    }
    if !fails.is_empty() {
        return Outcome::Fail(fails.join("; "));
    }
    if !unknowns.is_empty() {
        return Outcome::Unknown(unknowns.join("; "));
    }
    Outcome::Ok(format!(
        "denied {}; the token lists only the served repos",
        plural(reqs.len(), "other private repo")
    ))
}

/// At most [`NAMED_REPOS`] names, then `…` when there are more.
fn shown(names: &[String]) -> String {
    let head = names
        .iter()
        .take(NAMED_REPOS)
        .cloned()
        .collect::<Vec<_>>()
        .join(", ");
    if names.len() > NAMED_REPOS {
        format!("{head}…")
    } else {
        head
    }
}

fn contents_outcome(reqs: &[Req], answers: &Answers) -> Outcome {
    let (mut readable, mut unknown) = (Vec::new(), false);
    for req in reqs {
        let Some(a) = answers.get(&req.url) else {
            unknown = true;
            continue;
        };
        // 409 is GitHub's answer for an empty repo the token can read.
        if a.status == 200 || a.status == 409 {
            readable.push(req.name.as_str());
        } else if !is_denial(a) {
            unknown = true;
        }
    }
    if !readable.is_empty() {
        return Outcome::Fail(format!("can read code in {}", readable.join(", ")));
    }
    if unknown {
        return Outcome::Unknown("couldn't check code access of every served repo".into());
    }
    Outcome::Ok("no Contents access".into())
}

/// Compares in memory with every other machine's token. Neither value is printed, stored or
/// hashed.
fn distinct_outcome(token: &str, secret_name: &str, ctx: &StepContext) -> Outcome {
    let mut failures = Vec::new();
    let mine = token.trim();
    for other in ctx.all.iter().filter(|o| o.name != ctx.machine.name) {
        let Some(theirs) = other
            .sd_worker()
            .map(|s| s.issues_secret.as_str())
            .filter(|s| !s.is_empty())
        else {
            continue;
        };
        if theirs == secret_name {
            failures.push(format!("{} uses the same secret {theirs}", other.name));
        } else if let Some(value) = ctx.secrets.value(theirs) {
            if !mine.is_empty() && value.expose().trim() == mine {
                failures.push(format!("same token as {} ({theirs})", other.name));
            }
        }
    }
    if failures.is_empty() {
        Outcome::Ok("no other machine uses this token".into())
    } else {
        Outcome::Fail(failures.join("; "))
    }
}

fn expiry_outcome(header: Option<&str>, day: Option<&str>, now: f64) -> Outcome {
    let Some(day) = day else {
        // A header that is there but unreadable is not "no expiry".
        return if header.is_none() {
            Outcome::Ok("GitHub reported no expiry".into())
        } else {
            Outcome::Unknown("couldn't read the expiry header".into())
        };
    };
    if expiry::status(day, expiry::today_local(now)) == Some(Status::Expired) {
        return Outcome::Fail(format!("expired on {day}"));
    }
    Outcome::Ok(format!("expires {day}"))
}
