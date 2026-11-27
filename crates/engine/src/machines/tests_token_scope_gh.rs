use super::fakes::*;
use super::github::{allows, GitHubHttp, HttpAnswer, LiveGitHub};
use super::step::{CheckState, StepFix};
use super::tests_token_scope::{
    cant_tell_text, check, ctx, good_http, http, m1, others, row, runner, runner_listing, table,
    DEFAULT_HEADER, SECRET,
};
use super::token_scope::step;
use crate::accounts::Secret;
use std::sync::Arc;

#[test]
fn an_empty_gh_listing_cant_tell_even_when_the_others_answer_200() {
    for listing in [vec![], vec!["dtthaison/some-other".to_string()]] {
        let result = check(&ctx(
            http(200, 403, 200, Some(DEFAULT_HEADER)),
            &runner_listing(Some(listing.clone())),
        ));
        assert!(
            cant_tell_text(&result).contains(REPO),
            "{listing:?} gave {:?}",
            result.state
        );
        assert!(!row(&result, "Denied elsewhere").ok);
    }
}

#[test]
fn a_served_repo_is_found_in_the_listing_whatever_its_case() {
    let mut list = vec![REPO.to_uppercase()];
    list.extend(others(30));
    let result = check(&ctx(good_http(), &runner_listing(Some(list))));
    assert!(result.state.is_pass(), "{:?}", result.state);
}

#[test]
fn a_listing_with_only_the_served_repos_passes() {
    let result = check(&ctx(
        good_http(),
        &runner_listing(Some(vec![REPO.to_string()])),
    ));
    assert!(result.state.is_pass(), "{:?}", result.state);
}

#[test]
fn a_listing_that_may_be_cut_or_has_odd_lines_cant_tell() {
    let mut full = vec![REPO.to_string()];
    full.extend((1..=999).map(|i| format!("dtthaison/p-{i}")));
    let wide = good_http();
    let cut = check(&ctx(wide.clone(), &runner_listing(Some(full.clone()))));
    let probes = wide
        .urls()
        .iter()
        .filter(|u| u.contains("/repos/dtthaison/p-"))
        .count();
    assert_eq!(probes, 999);
    assert!(cant_tell_text(&cut).contains("cut"), "{:?}", cut.state);
    let mut shorter = full.clone();
    shorter.pop();
    let not_cut = check(&ctx(good_http(), &runner_listing(Some(shorter))));
    assert!(not_cut.state.is_pass(), "{:?}", not_cut.state);
    let mut odd = vec![REPO.to_string(), "not a repo name".to_string()];
    odd.extend(others(30));
    let odd = check(&ctx(good_http(), &runner_listing(Some(odd))));
    assert!(
        matches!(odd.state, CheckState::CantTell(_)),
        "{:?}",
        odd.state
    );
}

#[test]
fn a_hostile_name_in_the_listing_never_becomes_a_request() {
    let list = vec![
        REPO.to_string(),
        "dtthaison/../../user".to_string(),
        "dtthaison/x?per_page=1#".to_string(),
        "x y/z".to_string(),
    ];
    let h = good_http();
    let result = check(&ctx(h.clone(), &runner_listing(Some(list))));
    assert!(
        matches!(result.state, CheckState::CantTell(_)),
        "{:?}",
        result.state
    );
    for url in h.urls() {
        assert!(!url.contains(".."), "{url}");
        assert!(!url.contains(' '), "{url}");
        assert!(allows(&url).is_some(), "{url}");
    }
}

#[test]
fn a_secondary_rate_limit_body_is_not_a_denial() {
    let mut a = table(200, 403, 404, Some(DEFAULT_HEADER), None);
    for name in others(30) {
        a.insert(
            format!("/repos/{name}"),
            HttpAnswer::new(
                403,
                &[],
                r#"{"message":"You have exceeded a secondary rate limit."}"#,
            ),
        );
    }
    let result = check(&ctx(Arc::new(PathHttp::new(a)), &runner()));
    assert!(
        matches!(result.state, CheckState::CantTell(_)),
        "{:?}",
        result.state
    );
}

#[test]
fn an_unreadable_expiry_header_cant_tell() {
    let result = check(&ctx(http(200, 403, 404, Some("soon")), &runner()));
    assert!(cant_tell_text(&result).contains("expiry"));
    assert_eq!(result.detected.token_expires, None);
}

#[test]
fn the_same_token_with_stray_whitespace_is_still_the_same() {
    let mut c = ctx(good_http(), &runner());
    let spaced = format!("{SECRET} \n");
    c.secrets = Arc::new(FakeSecrets::with(&[
        ("m1-github-issues", SECRET),
        ("vps-sing-1-github-issues", &spaced),
    ]));
    assert_eq!(
        check(&c).state,
        CheckState::Fail("same token as vps-sing-1 (vps-sing-1-github-issues)".into())
    );
}

#[test]
fn the_live_client_refuses_any_other_host_without_sending_anything() {
    for url in [
        "https://evil.example/repos/a/b",
        "http://api.github.com/repos/a/b",
        "https://api.github.com.evil.example/x",
        "https://user@api.github.com/x",
        "https://api.github.com:8443/x",
    ] {
        let a = LiveGitHub.get(url, &Secret::new(SECRET));
        assert_eq!(a.status, 0, "{url}");
        assert!(a.headers.is_empty(), "{url}");
    }
}

#[test]
fn the_fix_opens_the_token_flow_for_the_machines_secret() {
    let machine = m1();
    let s = step(&machine, machine.sd_worker().unwrap());
    let StepFix::Run(plan) = &s.fix else {
        panic!("no fix");
    };
    assert!(!plan.needs_admin);
    assert!(plan.display.contains("m1-github-issues"));
    assert!(
        plan.display.contains("the Secret Vault") && !plan.display.contains("form"),
        "{}",
        plan.display
    );
    assert!(
        plan.display.contains("Issues: read and write"),
        "{}",
        plan.display
    );
    assert!(!plan.display.contains("read-only"), "{}", plan.display);
    assert!(plan.display.contains("Metadata: read"), "{}", plan.display);
    let c = ctx(good_http(), &runner());
    let (fix, _) = c.fix(None);
    let outcome = (plan.run)(&fix);
    assert!(!outcome.ok);
    assert!(
        outcome
            .message
            .ends_with("Opened GitHub's token page. Fix the token, save it in the Secret Vault as m1-github-issues, then Check again."),
        "{}",
        outcome.message
    );
    assert!(!outcome.message.contains("SECRET123"));
    assert_eq!(c.opener.pages().len(), 1);
    assert_eq!(c.opener.forms()[0].name, "m1-github-issues");
}
