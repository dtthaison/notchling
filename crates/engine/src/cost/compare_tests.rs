use super::compare::{compare, window_start};
use super::ledger::Rollup;
use super::summary::day_key;

const TODAY: i64 = 20_000;

fn row(day: i64, calls: u64, cost: f64, unpriced: u64) -> Rollup {
    Rollup {
        day: day_key(day),
        tool: "claude".into(),
        sub: false,
        model: "m".into(),
        calls,
        input: 100 * calls,
        output: 10 * calls,
        cache_read: 1000 * calls,
        write_5m: 0,
        write_1h: 0,
        cost,
        unpriced,
    }
}

#[test]
fn the_week_before_and_the_week_after_are_cut_on_the_day_set_up_ran() {
    let applied = TODAY - 10;
    let rows = vec![
        row(applied - 8, 1, 100.0, 0),
        row(applied - 7, 2, 4.0, 0),
        row(applied - 1, 3, 6.0, 0),
        row(applied, 4, 1.0, 0),
        row(TODAY - 6, 5, 2.0, 0),
        row(TODAY, 6, 3.0, 0),
    ];
    let c = compare(&rows, TODAY, applied, 7);
    assert_eq!((c.before.calls, c.before.cost), (5, Some(10.0)));
    assert_eq!(c.before.days, 7);
    assert_eq!(c.before.from, day_key(applied - 7));
    assert_eq!(c.before.to, day_key(applied - 1));
    assert_eq!((c.after.calls, c.after.cost), (11, Some(5.0)));
    assert_eq!(c.after.days, 7);
    assert_eq!(c.applied, day_key(applied));
}

#[test]
fn a_set_up_newer_than_the_window_counts_only_the_days_since() {
    let applied = TODAY - 2;
    let rows = vec![
        row(applied - 3, 1, 8.0, 0),
        row(applied, 2, 1.0, 0),
        row(TODAY, 3, 2.0, 0),
    ];
    let c = compare(&rows, TODAY, applied, 30);
    assert_eq!(c.after.days, 3);
    assert_eq!(c.after.cost, Some(3.0));
    assert_eq!(c.before.days, 30);
    assert_eq!(c.before.cost, Some(8.0));
}

#[test]
fn a_model_with_no_price_shows_its_tokens_and_adds_no_cost() {
    let applied = TODAY - 3;
    let rows = vec![row(TODAY, 2, 0.0, 2), row(TODAY - 1, 2, 5.0, 1)];
    let c = compare(&rows, TODAY, applied, 7);
    assert_eq!(c.after.cost, Some(5.0));
    assert_eq!(c.after.unpriced_calls, 3);
    assert_eq!(c.after.unpriced_tokens, 3330);

    let only = compare(&[row(TODAY, 2, 0.0, 2)], TODAY, applied, 7);
    assert_eq!(only.after.cost, None);
    assert_eq!(only.after.tokens, 2220);
    assert_eq!(only.before.calls, 0);
}

#[test]
fn a_set_up_day_in_the_future_is_taken_as_today() {
    let c = compare(&[row(TODAY, 1, 1.0, 0)], TODAY, TODAY + 5, 7);
    assert_eq!(c.applied, day_key(TODAY));
    assert_eq!(c.after.days, 1);
}

#[test]
fn the_rollup_read_starts_a_window_before_the_applied_day_never_before_today() {
    assert_eq!(window_start(TODAY, TODAY - 3, 7), TODAY - 10);
    assert_eq!(window_start(TODAY, TODAY + 5, 7), TODAY - 7);
    assert_eq!(window_start(TODAY, TODAY, 0), TODAY - 1);
}
