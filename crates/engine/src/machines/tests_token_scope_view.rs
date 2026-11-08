//! Token scope: the token's own list of private repos, requests, expiry (Swift
//! `MachinesTokenScopeTests`, middle).

use super::fakes::*;
use super::github::{allows, HttpAnswer, BODY_CAP};
use super::registry::MachineRepo;
use super::runner::{CommandResult, ScriptedRunner};
use super::step::CheckState;
use super::tests_token_scope::{
    cant_tell_text, check, ctx, fail_text, good_http, http, http_with_view, m1, others, raw_view,
    row, runner, table, view_answer, vps, DEFAULT_HEADER, SECRET, VIEW_KEY,
};
use super::token_scope::expiry_day;
use std::sync::Arc;
use std::time::Duration;

// MARK: the token's own view (GET /user/repos?visibility=private)

#[test]
fn a_token_that_lists_all_31_private_repos_fails() {
    // Every per-repo probe says denied; the token's own list is what shows the mistake.
    let mut names = vec![REPO.to_string()];
    names.extend(others(30));
    let result = check(&ctx(
        http_with_view(view_answer(&names, 200, None)),
        &runner(),
    ));
    let text = fail_text(&result);
    assert!(text.starts_with("token reaches 30 other private repos (e.g. dtthaison/private-1, dtthaison/private-10, dtthaison/private-11"), "{text}");
    assert!(text.ends_with("set Only select repositories"), "{text}");
    assert!(!row(&result, "Denied elsewhere").ok);
}

#[test]
fn a_token_that_lists_only_the_served_repo_passes() {
    let result = check(&ctx(good_http(), &runner()));
    assert!(result.state.is_pass(), "{:?}", result.state);
}

/// The issues probe passes, so the token reaches the served repo; a private-repo list that does not
/// show it is not the whole truth, so the result is can't tell, never a pass.
#[test]
fn a_view_that_lacks_a_served_repo_cant_tell() {
    let result = check(&ctx(http_with_view(view_answer(&[], 200, None)), &runner()));
    assert!(cant_tell_text(&result).contains(REPO));
    assert!(!row(&result, "Denied elsewhere").ok);
    // another repo in the list is a fail, which wins over the missing one
    let other = check(&ctx(
        http_with_view(view_answer(
            &["dtthaison/something-else".to_string()],
            200,
            None,
        )),
        &runner(),
    ));
    assert!(other.state.is_fail(), "{:?}", other.state);
    // the served repo's name is compared without regard to case
    let upper = check(&ctx(
        http_with_view(view_answer(&[REPO.to_uppercase()], 200, None)),
        &runner(),
    ));
    assert!(upper.state.is_pass(), "{:?}", upper.state);
}

/// A name that is not owner/name is doubt, not something to drop: the repo it names could be a
/// leak.
#[test]
fn a_repo_name_the_check_cant_read_makes_the_view_doubtful() {
    let odd = check(&ctx(
        http_with_view(raw_view(&format!(
            r#"[{{"full_name":"{REPO}"}},{{"full_name":"bad name with spaces"}}]"#
        ))),
        &runner(),
    ));
    assert!(
        matches!(odd.state, CheckState::CantTell(_)),
        "{:?}",
        odd.state
    );
    for body in [
        format!(r#"[{{"full_name":"{REPO}"}},{{"full_name":42}}]"#),
        format!(r#"[{{"full_name":"{REPO}"}},{{"id":2}}]"#),
        format!(r#"[{{"full_name":"{REPO}"}},"dtthaison/x"]"#),
        format!(r#"[{{"full_name":"{REPO}"}},{{"full_name":"a/../b"}}]"#),
    ] {
        let result = check(&ctx(http_with_view(raw_view(&body)), &runner()));
        assert!(
            matches!(result.state, CheckState::CantTell(_)),
            "{body} gave {:?}",
            result.state
        );
    }
    // a leak that can be read still fails, whatever else is odd
    let leak = format!(
        r#"[{{"full_name":"{REPO}"}},{{"full_name":"dtthaison/leaked"}},{{"full_name":"bad name"}}]"#
    );
    let failed = check(&ctx(http_with_view(raw_view(&leak)), &runner()));
    assert!(failed.state.is_fail(), "{:?}", failed.state);
}

#[test]
fn the_view_call_is_private_only_and_paged() {
    let h = good_http();
    check(&ctx(h.clone(), &runner()));
    let views: Vec<String> = h
        .urls()
        .into_iter()
        .filter(|u| u.contains("/user/repos"))
        .collect();
    assert_eq!(
        views,
        vec!["https://api.github.com/user/repos?visibility=private&per_page=50"]
    );
}

#[test]
fn an_error_on_the_view_cant_tell() {
    for status in [500, 403, 404, 0] {
        let result = check(&ctx(
            http_with_view(view_answer(&[], status, None)),
            &runner(),
        ));
        assert!(
            matches!(result.state, CheckState::CantTell(_)),
            "view {status} gave {:?}",
            result.state
        );
    }
    let limited = HttpAnswer::new(403, &[("x-ratelimit-remaining", "0")], "[]");
    let rate = check(&ctx(http_with_view(limited), &runner()));
    assert!(
        matches!(rate.state, CheckState::CantTell(_)),
        "{:?}",
        rate.state
    );
    let junk = check(&ctx(http_with_view(raw_view("not json")), &runner()));
    assert!(
        matches!(junk.state, CheckState::CantTell(_)),
        "{:?}",
        junk.state
    );
    let revoked = check(&ctx(http_with_view(view_answer(&[], 401, None)), &runner()));
    assert_eq!(
        revoked.state,
        CheckState::Fail("GitHub refused the token: expired or revoked".into())
    );
}

#[test]
fn the_view_follows_link_pages_on_api_github_com_and_sees_the_later_ones() {
    let next = "https://api.github.com/user/repos?visibility=private&per_page=50&page=2";
    let link = format!("<{next}>; rel=\"next\", <{next}>; rel=\"last\"");
    let mut a = table(
        200,
        403,
        404,
        Some(DEFAULT_HEADER),
        Some(view_answer(&[REPO.to_string()], 200, Some(&link))),
    );
    a.insert(
        "/user/repos?visibility=private&per_page=50&page=2".into(),
        view_answer(&["dtthaison/secret-other".to_string()], 200, None),
    );
    let h = Arc::new(PathHttp::new(a));
    let result = check(&ctx(h.clone(), &runner()));
    assert_eq!(
        result.state,
        CheckState::Fail("token reaches 1 other private repo (e.g. dtthaison/secret-other); set Only select repositories".into())
    );
    assert!(h
        .requests()
        .iter()
        .all(|(u, bearer)| u.starts_with("https://api.github.com/") && bearer == SECRET));
}

#[test]
fn a_link_header_cannot_move_the_token_to_another_host() {
    for target in [
        "https://evil.example/user/repos?page=2",
        "http://api.github.com/user/repos?page=2",
        "https://api.github.com.evil.example/user/repos?page=2",
        "https://api.github.com:8443/user/repos?page=2",
        "https://api.github.com/repos/dtthaison/lingeva-mono/issues",
    ] {
        let link = format!("<{target}>; rel=\"next\"");
        let h = http_with_view(view_answer(&[REPO.to_string()], 200, Some(&link)));
        let result = check(&ctx(h.clone(), &runner()));
        assert!(
            matches!(result.state, CheckState::CantTell(_)),
            "{target} gave {:?}",
            result.state
        );
        assert!(!h.urls().iter().any(|u| u == target), "{target}");
        assert!(h
            .urls()
            .iter()
            .all(|u| u.starts_with("https://api.github.com/")));
    }
}

#[test]
fn the_view_stops_after_ten_pages_and_cant_tell() {
    let mut a = table(200, 403, 404, Some(DEFAULT_HEADER), None);
    let base = "https://api.github.com/user/repos?visibility=private&per_page=50";
    let repo = [REPO.to_string()];
    a.insert(
        VIEW_KEY.into(),
        view_answer(&repo, 200, Some(&format!("<{base}&page=2>; rel=\"next\""))),
    );
    for n in 2..=30 {
        a.insert(
            format!("/user/repos?visibility=private&per_page=50&page={n}"),
            view_answer(
                &repo,
                200,
                Some(&format!("<{base}&page={}>; rel=\"next\"", n + 1)),
            ),
        );
    }
    let h = Arc::new(PathHttp::new(a));
    let result = check(&ctx(h.clone(), &runner()));
    assert!(
        matches!(result.state, CheckState::CantTell(_)),
        "{:?}",
        result.state
    );
    assert_eq!(
        h.urls()
            .iter()
            .filter(|u| u.contains("/user/repos"))
            .count(),
        10
    );
}

#[test]
fn a_cut_off_view_page_still_fails_on_what_it_showed_but_never_passes() {
    // A page the 1 MiB cap cut: the names seen before the cut count, and without one the result is
    // can't tell.
    let pad = "x".repeat(BODY_CAP);
    let cut_to_cap = |head: &str| -> HttpAnswer {
        let mut body = format!("{head}{pad}").into_bytes();
        body.truncate(BODY_CAP);
        HttpAnswer {
            status: 200,
            body,
            ..HttpAnswer::default()
        }
    };
    let leaked = check(&ctx(
        http_with_view(cut_to_cap(
            r#"[{"full_name":"dtthaison/leaked"},{"description":""#,
        )),
        &runner(),
    ));
    assert!(leaked.state.is_fail(), "{:?}", leaked.state);
    let quiet = check(&ctx(
        http_with_view(cut_to_cap(&format!(
            r#"[{{"full_name":"{REPO}"}},{{"description":""#
        ))),
        &runner(),
    ));
    assert!(
        matches!(quiet.state, CheckState::CantTell(_)),
        "{:?}",
        quiet.state
    );
}

#[test]
fn a_failed_gh_list_cant_tell() {
    let r = Arc::new(ScriptedRunner::new());
    r.on(
        |a| a.iter().any(|x| x == "list"),
        CommandResult::new(1, "", "gh: not logged in"),
    );
    let result = check(&ctx(good_http(), &r));
    assert!(!cant_tell_text(&result).is_empty());
}

#[test]
fn gh_lists_each_owner_once_with_the_documented_arguments() {
    let mut m = m1();
    m.sd_worker_mut().unwrap().repos.push(MachineRepo {
        github: "dtthaison/second".into(),
        local_path: "/x".into(),
        check_command: "true".into(),
        machine_path: None,
        verify_skill: None,
    });
    let r = runner();
    let mut c = ctx(good_http(), &r);
    c.machine = m.clone();
    c.all = Some(vec![m, vps()]);
    check(&c);
    let lists: Vec<_> = r
        .calls()
        .into_iter()
        .filter(|c| c.argv.iter().any(|a| a == "list"))
        .collect();
    assert_eq!(lists.len(), 1);
    assert_eq!(
        lists[0].argv[1..],
        [
            "repo",
            "list",
            "dtthaison",
            "--visibility",
            "private",
            "--limit",
            "1000",
            "--json",
            "nameWithOwner",
            "--jq",
            ".[].nameWithOwner"
        ]
    );
    assert!(lists[0].argv[0].ends_with("/gh"));
}

#[test]
fn token_scope_never_puts_the_token_in_any_result() {
    // A good run, a shared-token run, a 401 run and a wide-open run: none shows the value anywhere.
    let two: Vec<(&str, &str)> = vec![
        ("m1-github-issues", SECRET),
        ("vps-sing-1-github-issues", "github_pat_OTHER"),
    ];
    let shared: Vec<(&str, &str)> = vec![
        ("m1-github-issues", SECRET),
        ("vps-sing-1-github-issues", SECRET),
    ];
    type Run<'a> = (Arc<PathHttp>, Vec<(&'a str, &'a str)>);
    let runs: Vec<Run> = vec![
        (good_http(), two.clone()),
        (good_http(), shared),
        (http(401, 403, 404, Some(DEFAULT_HEADER)), two.clone()),
        (http(200, 403, 200, Some(DEFAULT_HEADER)), two),
    ];
    for (h, values) in runs {
        let r = runner();
        let mut c = ctx(h.clone(), &r);
        c.secrets = Arc::new(FakeSecrets::with(&values));
        let result = check(&c);
        assert!(!format!("{result:?}").contains("SECRET123"));
        assert!(!h.requests().is_empty());
        for (url, bearer) in h.requests() {
            assert!(!url.contains("SECRET123"));
            assert_eq!(bearer, SECRET);
        }
        for call in r.calls() {
            assert!(!call.argv.join(" ").contains("SECRET123"));
            assert!(call.env.iter().all(|(_, v)| !v.contains("SECRET123")));
            assert!(call.stdin.is_none());
        }
    }
    // The other machine's value stays out as well, and is never sent as a bearer.
    let h = good_http();
    let result = check(&ctx(h.clone(), &runner()));
    assert!(!format!("{result:?}").contains("github_pat_OTHER"));
    assert!(!h.requests().iter().any(|(_, b)| b == "github_pat_OTHER"));
}

#[test]
fn the_token_goes_only_to_api_github_com() {
    let h = good_http();
    check(&ctx(h.clone(), &runner()));
    // issues, commits, the others, the token's own list
    assert_eq!(h.requests().len(), 2 + 30 + 1);
    for (url, _) in h.requests() {
        assert!(url.starts_with("https://api.github.com/"), "{url}");
        assert!(allows(&url).is_some(), "{url}");
    }
}

#[test]
fn a_repo_name_that_is_not_owner_slash_name_never_becomes_a_request() {
    let mut m = m1();
    m.sd_worker_mut().unwrap().repos = vec![MachineRepo {
        github: "../../user".into(),
        local_path: "/x".into(),
        check_command: "true".into(),
        machine_path: None,
        verify_skill: None,
    }];
    let h = good_http();
    let mut c = ctx(h.clone(), &runner());
    c.machine = m.clone();
    c.all = Some(vec![m, vps()]);
    let result = check(&c);
    assert!(
        matches!(result.state, CheckState::CantTell(_)),
        "{:?}",
        result.state
    );
    assert!(h.requests().is_empty());
}

#[test]
fn requests_run_at_most_four_at_a_time() {
    let slow = Arc::new(PathHttp::slow(Duration::from_millis(5)));
    check(&ctx(slow.clone(), &runner()));
    assert!(slow.max_in_flight() > 1, "{}", slow.max_in_flight());
    assert!(slow.max_in_flight() <= 4, "{}", slow.max_in_flight());
}

#[test]
fn expiry_header_parsing() {
    assert_eq!(
        expiry_day("2027-02-01 12:00:00 UTC").as_deref(),
        Some("2027-02-01")
    );
    assert_eq!(
        expiry_day("2027-02-01 12:00:00 +0700").as_deref(),
        Some("2027-02-01")
    );
    assert_eq!(expiry_day("soon"), None);
    assert_eq!(expiry_day(""), None);
    assert_eq!(expiry_day("2027-02-30 00:00:00 UTC"), None);
}
