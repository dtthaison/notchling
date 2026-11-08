//! Port of `LimitLogicTests` and the logic half of `ProviderLimitsTests`.

use super::logic::*;
use super::report::{Bar, BarKind, Group, UsageReport};
use super::usage::UsageWindow;
use std::collections::HashMap;

const NOW: f64 = 1_800_000_000.0;
const RESET: f64 = NOW + 3600.0;

fn w(percent: f64, resets_at: Option<f64>) -> UsageWindow {
    UsageWindow { percent, resets_at }
}

fn bar(label: &str, kind: BarKind, percent: f64) -> Bar {
    Bar {
        label: label.into(),
        kind,
        percent,
        resets_at: None,
    }
}

fn report(session: f64, week: f64, fetched: f64, extra: Vec<Group>) -> UsageReport {
    let mut groups = vec![Group {
        name: Some("Codex".into()),
        bars: vec![
            Bar {
                resets_at: Some(RESET),
                ..bar("5h", BarKind::Session, session)
            },
            Bar {
                resets_at: Some(RESET + 86_400.0),
                ..bar("Week", BarKind::Week, week)
            },
        ],
    }];
    groups.extend(extra);
    UsageReport { groups, fetched }
}

fn cand(id: &str, fetched: f64, session: Option<f64>, week: Option<f64>) -> Candidate {
    Candidate::new(id, fetched, session, week)
}

#[test]
fn levels() {
    assert_eq!(level(89.9), 0);
    assert_eq!(level(90.0), 90);
    assert_eq!(level(99.9), 90);
    assert_eq!(level(100.0), 100);
}

#[test]
fn step_announces_each_level_once() {
    let mut mark = Mark::default();
    let (m, fire) = step(mark, 91.0, Some(RESET), false);
    assert_eq!(fire, Some(90));
    mark = m;
    let (m, fire) = step(mark, 95.0, Some(RESET), false);
    assert_eq!(fire, None);
    mark = m;
    assert_eq!(step(mark, 100.0, Some(RESET), false).1, Some(100));
}

#[test]
fn step_fires_again_after_the_window_resets() {
    let first = step(Mark::default(), 92.0, Some(NOW), false).0;
    assert_eq!(
        step(first, 92.0, Some(NOW + 5.0 * 3600.0), false).1,
        Some(90)
    );
}

#[test]
fn step_fires_again_after_usage_drops() {
    let mut mark = step(Mark::default(), 92.0, Some(NOW), false).0;
    mark = step(mark, 10.0, Some(NOW), false).0;
    assert_eq!(step(mark, 93.0, Some(NOW), false).1, Some(90));
}

#[test]
fn skip90_still_records_the_level() {
    let (mark, fire) = step(Mark::default(), 90.0, Some(NOW), true);
    assert_eq!(fire, None);
    assert_eq!(mark.level, 90);
}

#[test]
fn pick_target_prefers_the_most_room() {
    let all = [
        cand("active", NOW, Some(5.0), Some(5.0)),
        cand("busy", NOW, Some(50.0), Some(10.0)),
        cand("roomy", NOW, Some(20.0), Some(30.0)),
    ];
    let got = pick_target(&all, Some("active"), NOW, &HashMap::new());
    assert_eq!(got.map(|c| c.id.as_str()), Some("roomy"));
}

#[test]
fn pick_target_skips_stale_full_blocked_and_signed_out() {
    let mut signed_out = cand("signedOut", NOW, Some(1.0), Some(1.0));
    signed_out.needs_login = true;
    let all = [
        cand("stale", NOW - FRESH_FOR - 1.0, Some(1.0), Some(1.0)),
        cand("full", NOW, Some(80.0), Some(1.0)),
        cand("blocked", NOW, Some(1.0), Some(1.0)),
        signed_out,
        cand("unknown", NOW, None, None),
    ];
    let blocked = HashMap::from([("blocked".to_string(), NOW + 60.0)]);
    assert!(pick_target(&all, None, NOW, &blocked).is_none());
    assert_eq!(
        pick_target(&all, None, NOW + 61.0, &blocked).map(|c| c.id.as_str()),
        Some("blocked"),
        "once its reset passes, the blocked account is the only one with no other fault"
    );
}

#[test]
fn a_blocked_account_comes_back_after_its_reset() {
    let c = [cand("a", NOW, Some(1.0), Some(1.0))];
    let until = |t: f64| HashMap::from([("a".to_string(), t)]);
    assert!(pick_target(&c, None, NOW, &until(NOW + 1.0)).is_none());
    assert_eq!(
        pick_target(&c, None, NOW, &until(NOW)).map(|c| c.id.as_str()),
        Some("a")
    );
}

#[test]
fn ties_go_to_the_first_listed() {
    let all = [
        cand("first", NOW, Some(10.0), Some(10.0)),
        cand("second", NOW, Some(10.0), Some(10.0)),
    ];
    assert_eq!(
        pick_target(&all, None, NOW, &HashMap::new()).map(|c| c.id.as_str()),
        Some("first")
    );
}

#[test]
fn may_switch_at_most_every_half_hour() {
    assert!(may_switch(None, NOW));
    assert!(!may_switch(Some(NOW - 29.0 * 60.0), NOW));
    assert!(may_switch(Some(NOW - 30.0 * 60.0), NOW));
}

#[test]
fn blocked_until_uses_the_latest_reset_over_the_threshold() {
    let session = w(96.0, Some(NOW + 3600.0));
    let week = w(97.0, Some(NOW + 86_400.0));
    let low = w(10.0, Some(NOW + 999_999.0));
    assert_eq!(
        blocked_until(Some(&session), Some(&week), &[low], 95.0, NOW),
        NOW + 86_400.0
    );
    assert_eq!(
        blocked_until(None, None, &[], 95.0, NOW),
        NOW + 5.0 * 3600.0,
        "no window over the line: wait five hours"
    );
}

#[test]
fn codex_watches_only_its_main_group() {
    let review = Group {
        name: Some("Code Review".into()),
        bars: vec![bar("Week", BarKind::Week, 99.0)],
    };
    let windows = watched_windows(&report(10.0, 20.0, NOW, vec![review]), Some("Codex"));
    let names: Vec<_> = windows.iter().map(|(n, _)| n.as_str()).collect();
    assert_eq!(names, ["5-hour", "weekly"]);
    let percents: Vec<_> = windows.iter().map(|(_, w)| w.percent).collect();
    assert_eq!(percents, [10.0, 20.0]);
}

#[test]
fn antigravity_watches_every_group_and_names_it() {
    let r = UsageReport {
        groups: vec![
            Group {
                name: Some("Gemini Pro".into()),
                bars: vec![bar("5h", BarKind::Session, 50.0)],
            },
            Group {
                name: Some("Flash".into()),
                bars: vec![bar("Week", BarKind::Week, 70.0)],
            },
        ],
        fetched: NOW,
    };
    let names: Vec<_> = watched_windows(&r, None)
        .into_iter()
        .map(|(n, _)| n)
        .collect();
    assert_eq!(names, ["Gemini Pro 5-hour", "Flash weekly"]);
}

#[test]
fn a_candidate_from_a_report_takes_the_highest_short_and_weekly_bars() {
    let c = Candidate::from_report(
        "a",
        Some(&report(30.0, 60.0, NOW, vec![])),
        Some("Codex"),
        false,
    );
    assert_eq!(
        (c.session, c.week, c.fetched),
        (Some(30.0), Some(60.0), NOW)
    );
    let none = Candidate::from_report("b", None, Some("Codex"), false);
    assert_eq!((none.session, none.week), (None, None));
    assert!(pick_target(&[none], None, NOW, &HashMap::new()).is_none());
}

#[test]
fn report_candidates_follow_the_same_room_and_freshness_rules_as_claude() {
    let all = [
        Candidate::from_report(
            "full",
            Some(&report(80.0, 1.0, NOW, vec![])),
            Some("Codex"),
            false,
        ),
        Candidate::from_report(
            "stale",
            Some(&report(1.0, 1.0, NOW - FRESH_FOR - 1.0, vec![])),
            Some("Codex"),
            false,
        ),
        Candidate::from_report(
            "roomy",
            Some(&report(20.0, 30.0, NOW, vec![])),
            Some("Codex"),
            false,
        ),
        Candidate::from_report(
            "roomier",
            Some(&report(5.0, 10.0, NOW, vec![])),
            Some("Codex"),
            false,
        ),
    ];
    assert_eq!(
        pick_target(&all, None, NOW, &HashMap::new()).map(|c| c.id.as_str()),
        Some("roomier")
    );
}

#[test]
fn an_announcement_fires_once_per_window_and_keeps_the_highest_level() {
    let mut marks = HashMap::new();
    let windows = vec![
        ("5-hour".to_string(), Some(w(92.0, Some(RESET)))),
        ("weekly".to_string(), Some(w(100.0, Some(RESET)))),
    ];
    let first = announcement(&mut marks, "a", &windows, false).expect("fires");
    assert_eq!((first.level, first.name.as_str()), (100, "weekly"));
    assert!(announcement(&mut marks, "a", &windows, false).is_none());
    assert!(
        announcement(&mut marks, "b", &windows, false).is_some(),
        "another account has its own marks"
    );
}

fn decide(
    active: f64,
    candidates: Option<Vec<Candidate>>,
    blocked: HashMap<String, f64>,
    last: Option<f64>,
    idle: bool,
    calls: &mut u32,
) -> SwitchDecision {
    let windows = vec![("5-hour".to_string(), w(active, Some(RESET)))];
    let pool = candidates.unwrap_or_else(|| {
        vec![Candidate::from_report(
            "other",
            Some(&report(10.0, 10.0, NOW, vec![])),
            Some("Codex"),
            false,
        )]
    });
    decide_switch(
        &windows,
        95,
        Some("me"),
        &pool,
        &blocked,
        last,
        NOW,
        &mut || {
            *calls += 1;
            idle
        },
    )
}

fn decide_simple(active: f64) -> SwitchDecision {
    decide(active, None, HashMap::new(), None, true, &mut 0)
}

#[test]
fn switches_to_the_account_with_room() {
    assert_eq!(
        decide_simple(96.0),
        SwitchDecision::SwitchTo("other".into())
    );
}

#[test]
fn stays_below_the_switch_point() {
    assert_eq!(decide_simple(94.0), SwitchDecision::Stay);
}

#[test]
fn no_room_when_every_other_account_is_full_stale_or_blocked() {
    let full = vec![Candidate::from_report(
        "full",
        Some(&report(85.0, 10.0, NOW, vec![])),
        Some("Codex"),
        false,
    )];
    assert_eq!(
        decide(96.0, Some(full), HashMap::new(), None, true, &mut 0),
        SwitchDecision::NoRoom
    );
    let blocked = HashMap::from([("other".to_string(), NOW + 60.0)]);
    assert_eq!(
        decide(96.0, None, blocked, None, true, &mut 0),
        SwitchDecision::NoRoom
    );
    let reset = HashMap::from([("other".to_string(), NOW)]);
    assert_eq!(
        decide(96.0, None, reset, None, true, &mut 0),
        SwitchDecision::SwitchTo("other".into())
    );
}

#[test]
fn at_most_one_switch_per_half_hour() {
    assert_eq!(
        decide(
            96.0,
            None,
            HashMap::new(),
            Some(NOW - 29.0 * 60.0),
            true,
            &mut 0
        ),
        SwitchDecision::Stay
    );
    assert_eq!(
        decide(
            96.0,
            None,
            HashMap::new(),
            Some(NOW - 30.0 * 60.0),
            true,
            &mut 0
        ),
        SwitchDecision::SwitchTo("other".into())
    );
}

#[test]
fn a_busy_provider_blocks_the_switch_and_is_asked_only_when_a_target_exists() {
    let mut calls = 0;
    assert_eq!(
        decide(96.0, None, HashMap::new(), None, false, &mut calls),
        SwitchDecision::Busy
    );
    assert_eq!(calls, 1);
    calls = 0;
    assert_eq!(
        decide(50.0, None, HashMap::new(), None, false, &mut calls),
        SwitchDecision::Stay
    );
    let full = vec![Candidate::from_report(
        "full",
        Some(&report(85.0, 10.0, NOW, vec![])),
        Some("Codex"),
        false,
    )];
    assert_eq!(
        decide(96.0, Some(full), HashMap::new(), None, false, &mut calls),
        SwitchDecision::NoRoom
    );
    assert_eq!(calls, 0);
}

#[test]
fn codex_auto_switch_waits_for_turns_and_the_app() {
    assert!(codex_idle_for_auto_switch(false, 0));
    assert!(!codex_idle_for_auto_switch(false, 1));
    assert!(!codex_idle_for_auto_switch(true, 0));
}

#[test]
fn the_lane_pool_cannot_take_the_main_logins_account() {
    let pool = lane_pool(
        vec![
            cand("main", NOW, Some(1.0), Some(1.0)),
            cand("free", NOW, Some(1.0), Some(1.0)),
        ],
        Some("main"),
    );
    assert_eq!(
        pick_target(&pool, None, NOW, &HashMap::new()).map(|c| c.id.as_str()),
        Some("free")
    );
}
