use super::when::{bounds, parse_when, When};

const NOW: f64 = 1_794_000_000.0;

fn local_midnight(now: f64, offset: i64) -> f64 {
    let local = now as i64 + offset;
    (local - local.rem_euclid(86_400) - offset) as f64
}

#[test]
fn today_starts_at_local_midnight() {
    for offset in [7 * 3600, -5 * 3600, 0] {
        let (after, before) = bounds(When::Today, NOW, offset);
        assert_eq!(after, local_midnight(NOW, offset));
        assert_eq!(before, NOW);
    }
    let early = 1_794_000_000.0 - (1_794_000_000_i64.rem_euclid(86_400)) as f64 + 30.0 * 60.0;
    let (after, _) = bounds(When::Today, early, 7 * 3600);
    assert_eq!(after, local_midnight(early, 7 * 3600));
    assert!(early - after < 86_400.0);
}

#[test]
fn seven_days_is_seven_times_86400_back() {
    assert_eq!(
        bounds(When::Days(7), NOW, 3600),
        (NOW - 7.0 * 86_400.0, NOW)
    );
}

#[test]
fn a_month_is_its_first_to_its_last_local_second() {
    let offset = 7 * 3600;
    let (after, before) = bounds(
        When::Month {
            year: 2026,
            month: 10,
        },
        NOW,
        offset,
    );
    assert_eq!(after, 1_790_812_800.0 - 7.0 * 3600.0);
    assert_eq!(before - after, 31.0 * 86_400.0);
    let (feb_after, feb_before) = bounds(
        When::Month {
            year: 2024,
            month: 2,
        },
        NOW,
        0,
    );
    assert_eq!(feb_before - feb_after, 29.0 * 86_400.0);
    let (_, december) = bounds(
        When::Month {
            year: 2025,
            month: 12,
        },
        NOW,
        0,
    );
    assert_eq!(december, 1_767_225_600.0);
}

#[test]
fn the_current_month_ends_now() {
    let (_, before) = bounds(
        When::Month {
            year: 2026,
            month: 11,
        },
        1_794_000_000.0,
        0,
    );
    assert_eq!(before, 1_794_000_000.0);
}

#[test]
fn tokens_parse() {
    assert_eq!(parse_when("today"), Some(When::Today));
    assert_eq!(parse_when("TODAY"), Some(When::Today));
    assert_eq!(parse_when("7d"), Some(When::Days(7)));
    assert_eq!(parse_when("3650d"), Some(When::Days(3650)));
    assert_eq!(
        parse_when("2026-11"),
        Some(When::Month {
            year: 2026,
            month: 11
        })
    );
}

#[test]
fn junk_is_none() {
    for junk in [
        "7x", "2026-13", "0d", "3651d", "", "d", "-1d", "2026-00", "26-11",
    ] {
        assert_eq!(parse_when(junk), None, "{junk}");
    }
}
