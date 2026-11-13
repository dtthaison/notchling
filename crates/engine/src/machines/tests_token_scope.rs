use super::fakes::*;
use super::github::{GitHubHttp, HttpAnswer};
use super::registry::{Machine, SdWorkerSettings};
use super::runner::{CommandResult, ScriptedRunner};
use super::step::{CheckResult, CheckState, Probe, StepFix};
use super::token_scope::step;
use std::collections::BTreeMap;
use std::sync::Arc;

pub(super) const SECRET: &str = "github_pat_SECRET123";
pub(super) const EXPIRY_HEADER: &str = "github-authentication-token-expiration";
pub(super) const VIEW_KEY: &str = "/user/repos?visibility=private&per_page=50";
pub(super) const DEFAULT_HEADER: &str = "2027-02-01 12:00:00 +0700";

pub(super) fn m1() -> Machine {
    TestMachines::mac("m1")
}

pub(super) fn vps() -> Machine {
    TestMachines::linux("vps-sing-1")
}

pub(super) fn answer(status: u16, headers: &[(&str, &str)]) -> HttpAnswer {
    HttpAnswer::new(status, headers, "")
}

pub(super) fn others(n: usize) -> Vec<String> {
    (1..=n).map(|i| format!("dtthaison/private-{i}")).collect()
}

pub(super) fn runner_listing(listing: Option<Vec<String>>) -> Arc<ScriptedRunner> {
    let r = Arc::new(ScriptedRunner::new());
    let names = listing.unwrap_or_else(|| {
        let mut all = vec![REPO.to_string()];
        all.extend(others(30));
        all
    });
    r.on(
        |a| a.iter().any(|x| x == "repo") && a.iter().any(|x| x == "list"),
        CommandResult::ok(&(names.join("\n") + "\n")),
    );
    r
}

pub(super) fn runner() -> Arc<ScriptedRunner> {
    runner_listing(None)
}

pub(super) fn view_answer(names: &[String], status: u16, link: Option<&str>) -> HttpAnswer {
    let json = format!(
        "[{}]",
        names
            .iter()
            .map(|n| format!(r#"{{"id":1,"full_name":"{n}","private":true}}"#))
            .collect::<Vec<_>>()
            .join(",")
    );
    let headers: Vec<(&str, &str)> = link.map(|l| ("link", l)).into_iter().collect();
    HttpAnswer::new(status, &headers, &json)
}

pub(super) fn served_view() -> HttpAnswer {
    view_answer(&[REPO.to_string()], 200, None)
}

pub(super) fn raw_view(body: &str) -> HttpAnswer {
    HttpAnswer::new(200, &[], body)
}

pub(super) fn table(
    issues: u16,
    commits: u16,
    other: u16,
    header: Option<&str>,
    view: Option<HttpAnswer>,
) -> BTreeMap<String, HttpAnswer> {
    let mut a = BTreeMap::new();
    let headers: Vec<(&str, &str)> = header.map(|h| (EXPIRY_HEADER, h)).into_iter().collect();
    a.insert(
        format!("/repos/{REPO}/issues?per_page=1"),
        answer(issues, &headers),
    );
    a.insert(
        format!("/repos/{REPO}/commits?per_page=1"),
        answer(commits, &[]),
    );
    a.insert(VIEW_KEY.to_string(), view.unwrap_or_else(served_view));
    for name in others(30) {
        a.insert(format!("/repos/{name}"), answer(other, &[]));
    }
    a
}

pub(super) fn http(issues: u16, commits: u16, other: u16, header: Option<&str>) -> Arc<PathHttp> {
    Arc::new(PathHttp::new(table(issues, commits, other, header, None)))
}

pub(super) fn good_http() -> Arc<PathHttp> {
    http(200, 403, 404, Some(DEFAULT_HEADER))
}

pub(super) fn http_with_view(view: HttpAnswer) -> Arc<PathHttp> {
    Arc::new(PathHttp::new(table(
        200,
        403,
        404,
        Some(DEFAULT_HEADER),
        Some(view),
    )))
}

pub(super) fn ctx(http: Arc<dyn GitHubHttp>, runner: &Arc<ScriptedRunner>) -> Ctx {
    let mut c = Ctx::new(m1())
        .all(vec![m1(), vps()])
        .runner(runner)
        .http(http)
        .secrets(&[
            ("m1-github-issues", SECRET),
            ("vps-sing-1-github-issues", "github_pat_OTHER"),
        ]);
    c.now = 1_800_000_000.0;
    c
}

pub(super) fn check(c: &Ctx) -> CheckResult {
    let job = c.machine.sd_worker().unwrap().clone();
    let s = step(&c.machine, &job);
    let Probe::Local(probe) = &s.probe else {
        panic!("token scope is not a local probe");
    };
    probe(&c.step())
}

pub(super) fn all_repos_machine() -> Machine {
    let mut m = m1();
    m.sd_worker_mut().unwrap().token_all_repos = Some(true);
    m
}

pub(super) fn all_repos_ctx(http: Arc<dyn GitHubHttp>) -> Ctx {
    let mut c = ctx(http, &runner());
    c.machine = all_repos_machine();
    c.all = Some(vec![all_repos_machine(), vps()]);
    c
}

pub(super) fn row<'a>(r: &'a CheckResult, name: &str) -> &'a super::step::SubRow {
    r.rows.iter().find(|x| x.name == name).unwrap()
}

pub(super) fn fail_text(r: &CheckResult) -> &str {
    match &r.state {
        CheckState::Fail(t) => t,
        other => panic!("expected fail, got {other:?}"),
    }
}

pub(super) fn cant_tell_text(r: &CheckResult) -> &str {
    match &r.state {
        CheckState::CantTell(t) => t,
        other => panic!("expected can't tell, got {other:?}"),
    }
}

#[test]
fn a_good_token_passes() {
    let result = check(&ctx(good_http(), &runner()));
    assert!(result.state.is_pass(), "{:?}", result.state);
    assert_eq!(result.rows.len(), 5);
    assert!(result.rows.iter().all(|r| r.ok), "{:?}", result.rows);
    assert_eq!(result.detected.token_expires.as_deref(), Some("2027-02-01"));
}

#[test]
fn the_issues_row_states_that_write_is_not_tried_here() {
    let result = check(&ctx(good_http(), &runner()));
    let detail = &row(&result, "Reaches issues").detail;
    assert!(detail.starts_with("can read issues of "), "{detail}");
    assert!(detail.contains("a write is not tried here"), "{detail}");
}

#[test]
fn the_probe_is_local_and_has_a_fix() {
    let job = m1().sd_worker().unwrap().clone();
    let s = step(&m1(), &job);
    assert_eq!(s.id, super::step::StepId::TOKEN_SCOPE);
    assert!(matches!(s.probe, Probe::Local(_)));
    assert!(matches!(s.fix, StepFix::Run(_)));
}

#[test]
fn a_token_that_sees_all_private_repos_fails() {
    let result = check(&ctx(http(200, 403, 200, Some(DEFAULT_HEADER)), &runner()));
    let text = fail_text(&result);
    assert!(text.starts_with("can see 30 other private repos"), "{text}");
    assert!(
        text.ends_with("(dtthaison/private-1, dtthaison/private-10, dtthaison/private-11…)"),
        "{text}"
    );
    assert_eq!(result.rows.iter().filter(|r| !r.ok).count(), 1);
}

#[test]
fn a_few_visible_repos_are_all_named() {
    let mut a = BTreeMap::new();
    a.insert(format!("/repos/{REPO}/issues?per_page=1"), answer(200, &[]));
    a.insert(
        format!("/repos/{REPO}/commits?per_page=1"),
        answer(404, &[]),
    );
    a.insert(VIEW_KEY.to_string(), served_view());
    a.insert("/repos/dtthaison/private-2".into(), answer(200, &[]));
    let result = check(&ctx(Arc::new(PathHttp::new(a)), &runner()));
    assert_eq!(
        result.state,
        CheckState::Fail("can see 1 other private repo (dtthaison/private-2)".into())
    );
}

#[test]
fn a_strict_job_still_fails_when_the_token_reaches_other_repos() {
    let mut strict = m1();
    strict.sd_worker_mut().unwrap().token_all_repos = Some(false);
    let mut names = vec![REPO.to_string()];
    names.extend(others(3));
    let mut c = ctx(
        Arc::new(PathHttp::new(table(
            200,
            403,
            200,
            Some(DEFAULT_HEADER),
            Some(view_answer(&names, 200, None)),
        ))),
        &runner(),
    );
    c.machine = strict.clone();
    c.all = Some(vec![strict, vps()]);
    let result = check(&c);
    assert!(
        fail_text(&result).contains("other private repo"),
        "{:?}",
        result.state
    );
}

#[test]
fn an_all_repos_job_accepts_other_repos_by_choice() {
    let mut names = vec![REPO.to_string()];
    names.extend(others(30));
    let h = Arc::new(PathHttp::new(table(
        200,
        403,
        200,
        Some(DEFAULT_HEADER),
        Some(view_answer(&names, 200, None)),
    )));
    let result = check(&all_repos_ctx(h));
    assert!(result.state.is_pass(), "{:?}", result.state);
    assert_eq!(
        row(&result, "Denied elsewhere").detail,
        "reaches all repositories, by choice (30 other private repos)"
    );
    assert!(result.rows.iter().all(|r| r.ok), "{:?}", result.rows);
}

#[test]
fn an_all_repos_job_ignores_doubts_about_the_other_repos() {
    let r = Arc::new(ScriptedRunner::new());
    r.on(
        |a| a.iter().any(|x| x == "repo") && a.iter().any(|x| x == "list"),
        CommandResult::new(1, "", "no"),
    );
    let mut c = ctx(http_with_view(view_answer(&[], 500, None)), &r);
    c.machine = all_repos_machine();
    c.all = Some(vec![all_repos_machine(), vps()]);
    let result = check(&c);
    assert!(result.state.is_pass(), "{:?}", result.state);
}

#[test]
fn an_all_repos_job_still_fails_on_contents_access() {
    let mut names = vec![REPO.to_string()];
    names.extend(others(3));
    let h = Arc::new(PathHttp::new(table(
        200,
        200,
        200,
        Some(DEFAULT_HEADER),
        Some(view_answer(&names, 200, None)),
    )));
    let result = check(&all_repos_ctx(h));
    assert!(fail_text(&result).contains("can read code in"));
}

#[test]
fn an_all_repos_job_still_fails_when_a_served_repo_lacks_issues_access() {
    let mut names = vec![REPO.to_string()];
    names.extend(others(3));
    let h = Arc::new(PathHttp::new(table(
        404,
        403,
        200,
        Some(DEFAULT_HEADER),
        Some(view_answer(&names, 200, None)),
    )));
    let result = check(&all_repos_ctx(h));
    assert!(fail_text(&result).contains("no issues access to"));
}

#[test]
fn an_all_repos_job_still_fails_on_a_shared_token() {
    let mut c = all_repos_ctx(http(200, 403, 200, Some(DEFAULT_HEADER)));
    c.secrets = Arc::new(FakeSecrets::with(&[
        ("m1-github-issues", SECRET),
        ("vps-sing-1-github-issues", SECRET),
    ]));
    assert!(check(&c).state.is_fail());
}

#[test]
fn the_all_repos_fix_does_not_ask_for_selected_repositories() {
    let machine = all_repos_machine();
    let s = step(&machine, machine.sd_worker().unwrap());
    let StepFix::Run(plan) = &s.fix else {
        panic!("no fix");
    };
    assert!(
        plan.display.contains("any repositories"),
        "{}",
        plan.display
    );
    assert!(
        plan.display.contains("Issues: read and write") && plan.display.contains("no Contents")
    );
    assert!(!plan.display.contains(REPO), "{}", plan.display);
    let c = Ctx::new(machine);
    let (fix, _) = c.fix(None);
    (plan.run)(&fix);
    let how = c.opener.forms()[0].how_to.clone().unwrap();
    assert!(how.contains("All repositories"));
}

#[test]
fn the_all_repos_flag_decodes_from_old_files_and_is_written_only_when_true() {
    let old =
        r#"{"python":"python3","sdPath":"/a/sd.py","repos":[],"kinds":[],"issuesSecret":"x"}"#;
    let mut s: SdWorkerSettings = serde_json::from_str(old).unwrap();
    assert_eq!(s.token_all_repos, None);
    assert!(!serde_json::to_string(&s).unwrap().contains("tokenAllRepos"));
    s.token_all_repos = Some(true);
    let back: SdWorkerSettings = serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
    assert_eq!(back.token_all_repos, Some(true));
}

#[test]
fn the_same_token_on_two_machines_fails() {
    let mut c = ctx(good_http(), &runner());
    c.secrets = Arc::new(FakeSecrets::with(&[
        ("m1-github-issues", SECRET),
        ("vps-sing-1-github-issues", SECRET),
    ]));
    let result = check(&c);
    assert_eq!(
        result.state,
        CheckState::Fail("same token as vps-sing-1 (vps-sing-1-github-issues)".into())
    );
    assert!(!row(&result, "Distinct token").ok);
    assert_eq!(result.rows.iter().filter(|r| !r.ok).count(), 1);
}

#[test]
fn two_machines_naming_one_secret_fails() {
    let mut other = vps();
    other.sd_worker_mut().unwrap().issues_secret = "m1-github-issues".into();
    let mut c = ctx(good_http(), &runner());
    c.all = Some(vec![m1(), other]);
    assert_eq!(
        check(&c).state,
        CheckState::Fail("vps-sing-1 uses the same secret m1-github-issues".into())
    );
}

#[test]
fn contents_access_fails() {
    for code in [200, 409] {
        let result = check(&ctx(http(200, code, 404, Some(DEFAULT_HEADER)), &runner()));
        assert_eq!(
            result.state,
            CheckState::Fail(format!("can read code in {REPO}")),
            "commits {code}"
        );
    }
}

#[test]
fn an_expired_or_revoked_token_fails() {
    let refused = CheckState::Fail("GitHub refused the token: expired or revoked".into());
    let revoked = check(&ctx(http(401, 403, 404, Some(DEFAULT_HEADER)), &runner()));
    assert_eq!(revoked.state, refused);
    let late = check(&ctx(http(200, 403, 401, Some(DEFAULT_HEADER)), &runner()));
    assert_eq!(late.state, refused);
    let past = check(&ctx(
        http(200, 403, 404, Some("2026-11-01 00:00:00 UTC")),
        &runner(),
    ));
    assert_eq!(past.state, CheckState::Fail("expired on 2026-11-01".into()));
    assert_eq!(past.detected.token_expires.as_deref(), Some("2026-11-01"));
}

#[test]
fn a_missing_vault_entry_cant_tell() {
    let h = good_http();
    let mut c = ctx(h.clone(), &runner());
    c.secrets = Arc::new(FakeSecrets::new());
    let result = check(&c);
    assert_eq!(
        result.state,
        CheckState::CantTell("m1-github-issues is not in the vault".into())
    );
    assert!(h.requests().is_empty());
}

#[test]
fn a_missing_expiry_header_is_reported_not_invented() {
    let result = check(&ctx(http(200, 403, 404, None), &runner()));
    assert!(result.state.is_pass(), "{:?}", result.state);
    assert_eq!(result.detected.token_expires, None);
}

#[test]
fn a_server_error_or_rate_limit_cant_tell_and_never_counts_as_denied() {
    let down = check(&ctx(http(503, 403, 404, Some(DEFAULT_HEADER)), &runner()));
    assert!(
        matches!(down.state, CheckState::CantTell(_)),
        "{:?}",
        down.state
    );
    let unreachable = check(&ctx(http(200, 403, 0, Some(DEFAULT_HEADER)), &runner()));
    assert!(matches!(unreachable.state, CheckState::CantTell(_)));

    let mut a = table(200, 403, 404, Some(DEFAULT_HEADER), None);
    for name in others(30) {
        a.insert(
            format!("/repos/{name}"),
            answer(403, &[("x-ratelimit-remaining", "0")]),
        );
    }
    let limited = check(&ctx(Arc::new(PathHttp::new(a)), &runner()));
    assert!(
        matches!(limited.state, CheckState::CantTell(_)),
        "{:?}",
        limited.state
    );
}
