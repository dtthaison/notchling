use serde_json::json;

use super::renders::{spend, Shown};
use super::spend::{percent, Compare, Span, SpendModel};
use super::wire;

fn side(days: u64, calls: u64, cost: Option<f64>, unpriced: u64) -> serde_json::Value {
    json!({
        "from": "2026-11-02", "to": "2026-11-08", "days": days, "calls": calls,
        "tokens": 1_500_000, "cost": cost, "unpriced_tokens": unpriced,
    })
}

#[test]
fn the_switch_asks_the_report_for_seven_or_thirty_days() {
    assert_eq!(wire::spend(Span::Week.days()).args["days"], 7);
    assert_eq!(wire::spend(Span::Month.days()).args["days"], 30);
    assert_eq!(wire::spend(7).op, "cost.report");
    assert_eq!(Span::from_key("month"), Some(Span::Month));
    assert_eq!(Span::from_key("year"), None);
}

#[test]
fn week_compares_spend_a_day_and_says_how_it_changed() {
    let model = spend(Shown::Spend);
    let view = model.view();
    assert_eq!(view.columns.len(), 2);
    assert_eq!(view.columns[0].title, "Before");
    assert_eq!(view.columns[0].headline, "$30.34 a day");
    assert_eq!(view.columns[0].detail.as_deref(), Some("$212 in all"));
    assert_eq!(view.columns[1].headline, "$16.94 a day");
    assert_eq!(view.change.as_deref(), Some("Spend a day changed by -44%."));
    assert!(view.note.is_none());
    assert!(view.intro.as_deref().unwrap().contains("7 days"));
}

#[test]
fn a_set_up_newer_than_the_month_says_so_and_compares_per_day() {
    let view = spend(Shown::SpendMonth).view();
    assert_eq!(
        view.note.as_deref(),
        Some(
            "Only 13 days since Set Up are counted in After, so the figures per day are compared."
        )
    );
    assert!(view.intro.as_deref().unwrap().contains("30 days"));
}

#[test]
fn tokens_of_a_model_with_no_price_are_shown_beside_the_spend() {
    let view = spend(Shown::Spend).view();
    assert_eq!(
        view.columns[1].unpriced.as_deref(),
        Some("5.3m tokens in models with no price are left out of this spend.")
    );
    assert!(view.columns[0].unpriced.is_none());
}

#[test]
fn a_period_with_only_unpriced_models_shows_tokens_instead_of_zero() {
    let reply = json!({ "compare": {
        "window": 7, "applied": "2026-11-09",
        "before": side(7, 10, None, 1_500_000),
        "after": side(7, 0, None, 0),
    }});
    let mut model = SpendModel::default();
    model.show(Ok(reply));
    let view = model.view();
    assert_eq!(view.columns[0].headline, "No price · 1.5m tokens");
    assert_eq!(view.columns[1].headline, "No usage recorded");
    assert!(view.change.is_none());
}

#[test]
fn without_a_compare_the_tab_says_set_up_has_not_run() {
    let mut model = SpendModel::default();
    assert_eq!(model.view().empty, Some("Reading usage…"));
    model.show(Ok(json!({ "report": {}, "compare": null })));
    assert_eq!(
        model.view().empty,
        Some("Set Up hasn't been applied yet, so there is nothing to compare.")
    );
    assert!(Compare::parse(&json!({})).is_none());
}

#[test]
fn a_failed_read_keeps_its_message_and_no_columns() {
    let mut model = SpendModel::default();
    model.show(Err("the usage ledger is not available".into()));
    assert!(model.view().columns.is_empty());
    assert_eq!(
        model.message.as_deref(),
        Some("the usage ledger is not available")
    );
}

#[test]
fn a_reply_for_the_span_no_longer_shown_is_dropped() {
    let mut model = SpendModel {
        span: Span::Month,
        ..SpendModel::default()
    };
    let reply = json!({"compare": null});
    assert!(!model.show_for(Span::Week, Ok(reply.clone())));
    assert!(!model.loaded);
    assert!(model.show_for(Span::Month, Ok(reply)));
    assert!(model.loaded);
}

#[test]
fn a_change_that_rounds_to_zero_prints_no_sign() {
    assert_eq!(percent(-0.004), "0%");
    assert_eq!(percent(0.004), "0%");
    assert_eq!(percent(0.0), "0%");
    assert_eq!(percent(-0.44), "-44%");
    assert_eq!(percent(0.126), "+13%");
}
