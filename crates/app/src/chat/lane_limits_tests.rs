use super::*;
use crate::accounts::wire::{ClaudeAccount, ClaudeList, ClaudeUsage, Lane};
use crate::chat::limit_ring::arc;
use notchling_engine::limits::report::{Bar, Group};

fn bar(label: &str, kind: BarKind, percent: f64) -> Bar {
    Bar {
        label: label.into(),
        kind,
        percent,
        resets_at: Some(1_800_000_000.0),
    }
}

fn report(session: f64) -> UsageReport {
    UsageReport {
        groups: vec![Group {
            name: None,
            bars: vec![
                bar("5 hours", BarKind::Session, session),
                bar("Week", BarKind::Week, 20.0),
            ],
        }],
        fetched: 1_800_000_000.0,
    }
}

fn account(id: &str, plan: &str, lane: Option<&str>) -> ClaudeAccount {
    ClaudeAccount {
        id: id.into(),
        email: format!("{id}@example.com"),
        plan: Some(plan.into()),
        needs_login: false,
        lane: lane.map(str::to_owned),
    }
}

fn snapshot() -> Snapshot {
    let mut s = Snapshot {
        claude: ClaudeList {
            on: true,
            active: Some("main".into()),
            accounts: vec![
                account("main", "Max", None),
                account("night", "Team", Some("night")),
                account("loose", "Pro", None),
            ],
        },
        lanes: vec![
            Lane {
                name: "night".into(),
                path: "/lanes/night".into(),
                account_id: Some("night".into()),
                email: None,
            },
            Lane {
                name: "bare".into(),
                path: "/lanes/bare".into(),
                account_id: None,
                email: None,
            },
        ],
        ..Snapshot::default()
    };
    s.claude_usage = ClaudeUsage::default();
    s.claude_usage.reports.insert("night".into(), report(42.0));
    s.claude_usage.reports.insert("main".into(), report(7.0));
    s.claude_usage.reports.insert("loose".into(), report(99.0));
    s
}

#[test]
fn a_lane_with_a_report_rings_its_own_five_hour_limit() {
    let l = limits_for(&snapshot(), "/lanes/night").unwrap();
    assert_eq!(l.plan.as_deref(), Some("Team"));
    assert_eq!(l.limits[0].kind, Kind::Session);
    assert_eq!(l.limits[0].percent, 42.0);
    assert_eq!(l.limits[1].kind, Kind::Week);
    let arc = arc(&l.limits).unwrap();
    assert!((arc.fraction - 0.42).abs() < 1e-6);
}

#[test]
fn a_lane_with_no_report_yet_has_no_ring() {
    let mut s = snapshot();
    s.claude_usage.reports.remove("night");
    assert_eq!(limits_for(&s, "/lanes/night"), None);
    s.claude_usage.reports.insert(
        "night".into(),
        UsageReport {
            groups: vec![],
            fetched: 0.0,
        },
    );
    assert_eq!(limits_for(&s, "/lanes/night"), None);
}

#[test]
fn a_lane_without_an_account_id_finds_the_account_that_says_it_is_live_there() {
    let mut s = snapshot();
    s.lanes[0].account_id = None;
    let l = limits_for(&s, "/lanes/night").unwrap();
    assert_eq!(l.limits[0].percent, 42.0);
}

#[test]
fn a_lane_nobody_runs_in_has_no_ring() {
    assert_eq!(limits_for(&snapshot(), "/lanes/bare"), None);
}

#[test]
fn a_folder_no_lane_names_matches_no_account() {
    assert_eq!(limits_for(&snapshot(), "/elsewhere"), None);
    assert_eq!(limits_for(&snapshot(), "/lanes/main"), None);
    assert_eq!(limits_for(&snapshot(), ""), None);
}
