use super::usage::*;
use serde_json::json;

#[test]
fn fixed_keys() {
    let usage = parse_usage(
        &json!({
            "five_hour": {"utilization": 42.0, "resets_at": "2026-11-01T15:00:00Z"},
            "seven_day": {"utilization": 10.0},
            "seven_day_opus": {"utilization": 3.0},
        }),
        7.0,
    );
    assert_eq!(usage.session.map(|w| w.percent), Some(42.0));
    assert!(usage.session.and_then(|w| w.resets_at).is_some());
    assert_eq!(usage.week.map(|w| w.percent), Some(10.0));
    let names: Vec<_> = usage.models.iter().map(|m| m.name.as_str()).collect();
    assert_eq!(names, ["Opus"]);
    assert_eq!(usage.fetched, 7.0);
}

#[test]
fn limits_list_fills_a_missing_session() {
    let usage = parse_usage(
        &json!({
            "five_hour": null,
            "limits": [
                {"kind": "five_hour", "group": "session", "percent": 61.0, "resets_at": "2026-11-01T15:00:00Z"},
                {"kind": "seven_day", "group": "weekly", "percent": 30.0},
                {"kind": "seven_day", "group": "weekly", "percent": 12.0},
                {"kind": "model_week", "group": "weekly", "percent": 70.0,
                 "scope": {"model": {"display_name": "Opus"}}},
                {"kind": "daily_requests", "group": "daily", "percent": 5.0},
                {"kind": "no_percent", "group": "weekly"},
            ],
        }),
        0.0,
    );
    assert_eq!(usage.session.map(|w| w.percent), Some(61.0));
    assert_eq!(
        usage.week.map(|w| w.percent),
        Some(30.0),
        "the higher of two weekly limits wins"
    );
    assert_eq!(usage.models.len(), 1);
    assert_eq!(usage.models[0].name, "Opus");
    assert_eq!(usage.models[0].window.percent, 70.0);
    let other: Vec<_> = usage.other.iter().map(|m| m.name.as_str()).collect();
    assert_eq!(other, ["Daily Requests"]);
}

#[test]
fn a_scoped_limit_replaces_the_fixed_model_key() {
    let usage = parse_usage(
        &json!({
            "seven_day_opus": {"utilization": 3.0},
            "limits": [{"kind": "x", "percent": 50.0, "scope": {"model": {"display_name": "Opus"}}}],
        }),
        0.0,
    );
    assert_eq!(usage.models.len(), 1);
    assert_eq!(usage.models[0].window.percent, 50.0);
}

#[test]
fn a_surface_scope_names_the_limit_and_integers_count() {
    let usage = parse_usage(
        &json!({"limits": [{"kind": "app_week", "percent": 8, "scope": {"surface": {"display_name": "Claude Code"}}}]}),
        0.0,
    );
    assert_eq!(usage.models[0].name, "Claude Code");
    assert_eq!(usage.models[0].window.percent, 8.0);
}

#[test]
fn an_empty_response_has_nothing() {
    let usage = parse_usage(&json!({}), 0.0);
    assert!(usage.session.is_none() && usage.week.is_none());
    assert!(usage.models.is_empty() && usage.other.is_empty());
    assert!(parse_usage(&json!("nonsense"), 0.0)
        .stopping_windows()
        .is_empty());
}

#[test]
fn stopping_windows_are_session_week_and_unknown_account_wide() {
    let usage = parse_usage(
        &json!({
            "five_hour": {"utilization": 1.0},
            "seven_day": {"utilization": 2.0},
            "seven_day_opus": {"utilization": 99.0},
            "limits": [{"kind": "monthly", "group": "monthly", "percent": 3.0}],
        }),
        0.0,
    );
    let percents: Vec<_> = usage.stopping_windows().iter().map(|w| w.percent).collect();
    assert_eq!(
        percents,
        [1.0, 2.0, 3.0],
        "a model's weekly limit doesn't stop the account"
    );
}
