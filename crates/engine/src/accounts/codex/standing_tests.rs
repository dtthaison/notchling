use super::login;
use super::standing::standing;
use super::store_tests::{Rig, FUTURE};
use super::testkit::login as login_at;
use crate::accounts::procs::{FakeApps, FakeProcs};
use crate::providers::usage::{Credits, Reached, Standing};
use crate::providers::AccountProvider;

const TEAM_BLOCKED: &str =
    include_str!("../../../tests/fixtures/codex/wham-usage-team-blocked.json");
const NOW: f64 = 1_794_734_000.0;

#[test]
fn the_captured_team_response_reads_as_used_percents_of_two_windows() {
    let (plan, usage) = login::usage(TEAM_BLOCKED.as_bytes(), NOW);
    assert_eq!(plan.as_deref(), Some("Team"));
    let usage = usage.unwrap();
    assert_eq!(usage.groups.len(), 1, "code review and extras are null");
    assert_eq!(usage.groups[0].name, "Codex");
    let bars = &usage.groups[0].bars;
    assert_eq!(bars.len(), 2);
    assert_eq!(bars[0].0, "5h");
    assert_eq!(bars[0].1.percent, 100.0);
    assert_eq!(bars[0].1.resets_at, Some(1_794_742_031.0));
    assert_eq!(bars[1].0, "Week");
    assert_eq!(bars[1].1.percent, 33.0);
    assert_eq!(bars[1].1.resets_at, Some(1_795_231_176.0));
}

#[test]
fn the_captured_response_says_the_workspace_is_out_of_credits() {
    assert_eq!(
        standing(TEAM_BLOCKED.as_bytes()),
        Some(Standing {
            reached: Some(Reached::Credits),
            credits: None,
        })
    );
}

fn of(body: &str) -> Option<Standing> {
    standing(body.as_bytes())
}

#[test]
fn a_member_out_of_credits_and_an_owner_out_of_credits_both_say_credits() {
    for kind in [
        "workspace_owner_credits_depleted",
        "workspace_member_credits_depleted",
    ] {
        let body = format!(
            r#"{{"rate_limit":{{"allowed":false,"limit_reached":true}},"rate_limit_reached_type":{{"type":"{kind}"}}}}"#
        );
        assert_eq!(of(&body).unwrap().reached, Some(Reached::Credits), "{kind}");
    }
}

#[test]
fn other_reached_types_and_a_bare_not_allowed_say_limit() {
    let typed = r#"{"rate_limit":{"allowed":false},"rate_limit_reached_type":{"type":"rate_limit_reached"}}"#;
    assert_eq!(of(typed).unwrap().reached, Some(Reached::Limit));
    let usage = r#"{"rate_limit_reached_type":{"type":"workspace_owner_usage_limit_reached"}}"#;
    assert_eq!(of(usage).unwrap().reached, Some(Reached::Limit));
    let bare = r#"{"rate_limit":{"allowed":false,"limit_reached":true}}"#;
    assert_eq!(of(bare).unwrap().reached, Some(Reached::Limit));
    let flag_only = r#"{"rate_limit":{"allowed":true,"limit_reached":true}}"#;
    assert_eq!(of(flag_only).unwrap().reached, Some(Reached::Limit));
}

#[test]
fn an_allowed_response_with_nothing_special_has_no_standing() {
    let body = r#"{"rate_limit":{"allowed":true,"limit_reached":false,"primary_window":{"used_percent":3}},
        "rate_limit_reached_type":null,
        "credits":{"has_credits":false,"unlimited":false,"balance":null}}"#;
    assert_eq!(of(body), None);
    assert_eq!(
        of(r#"{"credits":{"has_credits":true,"unlimited":false,"balance":"0"}}"#),
        None
    );
}

#[test]
fn credits_are_kept_when_unlimited_or_when_there_is_a_balance() {
    let unlimited = r#"{"credits":{"has_credits":true,"unlimited":true,"balance":null}}"#;
    assert_eq!(
        of(unlimited),
        Some(Standing {
            reached: None,
            credits: Some(Credits {
                unlimited: true,
                balance: None
            })
        })
    );
    let text = r#"{"credits":{"has_credits":true,"unlimited":false,"balance":"1250.5"}}"#;
    assert_eq!(
        of(text).unwrap().credits,
        Some(Credits {
            unlimited: false,
            balance: Some("1250.5".into())
        })
    );
    let number = r#"{"credits":{"has_credits":true,"unlimited":false,"balance":42}}"#;
    assert_eq!(
        of(number).unwrap().credits,
        Some(Credits {
            unlimited: false,
            balance: Some("42".into())
        })
    );
}

#[test]
fn a_balance_that_is_not_a_plain_number_is_dropped() {
    for hostile in [
        r#""<b>free</b>""#,
        r#""12 credits, click here""#,
        r#""""#,
        r#""-5""#,
        r#""12345678901234567890""#,
        r#"-3"#,
        r#"true"#,
    ] {
        let body = format!(
            r#"{{"credits":{{"has_credits":true,"unlimited":false,"balance":{hostile}}}}}"#
        );
        assert_eq!(of(&body), None, "{hostile}");
    }
}

#[test]
fn nonsense_has_no_standing() {
    assert_eq!(of("[]"), None);
    assert_eq!(of("nope"), None);
    assert_eq!(of("{}"), None);
}

const ACCOUNT: &str = "user-1:ws-1";

#[test]
fn the_store_keeps_the_standing_beside_the_bars_and_clears_it_when_a_read_has_none() {
    let rig = Rig::new("standing-clears", FakeProcs::new(), FakeApps::new());
    rig.live(&login_at(FUTURE));
    rig.http.ok(200, TEAM_BLOCKED);
    rig.store.sync(false);
    assert_eq!(
        rig.store.standing(ACCOUNT).unwrap().reached,
        Some(Reached::Credits)
    );
    assert_eq!(
        rig.store.usage_report(ACCOUNT).unwrap().bars()[0].percent,
        100.0
    );
    rig.http.ok(
        200,
        r#"{"rate_limit":{"allowed":true,"primary_window":{"used_percent":4,"limit_window_seconds":18000}}}"#,
    );
    rig.store.sync(true);
    assert_eq!(rig.store.standing(ACCOUNT), None);
    assert_eq!(
        rig.store.usage_report(ACCOUNT).unwrap().bars()[0].percent,
        4.0
    );
}

#[test]
fn a_failed_read_keeps_the_last_standing_and_removing_the_account_forgets_it() {
    let rig = Rig::new("standing-keeps", FakeProcs::new(), FakeApps::new());
    rig.live(&login_at(FUTURE));
    rig.http.ok(200, TEAM_BLOCKED);
    rig.store.sync(false);
    rig.http.ok(500, "");
    rig.store.sync(true);
    assert!(rig.store.standing(ACCOUNT).is_some());
    rig.store.remove(ACCOUNT);
    assert_eq!(rig.store.standing(ACCOUNT), None);
}
