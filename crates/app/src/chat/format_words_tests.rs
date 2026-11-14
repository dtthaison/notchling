use notchling_locale as t;

use super::format::{age, duration, tokens};

const NOW: i64 = 1_000_000_000_000;

#[test]
fn ages_and_durations_unchanged() {
    let ages = [
        (0, "now"),
        (59, "now"),
        (60, "1m"),
        (3_599, "59m"),
        (3_600, "1h"),
        (86_399, "23h"),
        (86_400, "1d"),
        (7 * 86_400, "7d"),
        (999 * 86_400, "999d"),
    ];
    for (seconds, want) in ages {
        assert_eq!(age(NOW, NOW - seconds * 1000), want, "{seconds} s");
    }
    assert_eq!(age(NOW, 0), "");
    let durations = [
        (0.0, "1s"),
        (0.4, "1s"),
        (59.0, "59s"),
        (60.0, "1m 00s"),
        (65.0, "1m 05s"),
        (3_599.0, "59m 59s"),
        (3_600.0, "1h 0m"),
        (4_320.0, "1h 12m"),
        (86_399.0, "23h 59m"),
    ];
    for (seconds, want) in durations {
        assert_eq!(duration(seconds), want, "{seconds} s");
    }
    let counts = [
        (0, "0"),
        (999, "999"),
        (1_000, "1k"),
        (1_049, "1k"),
        (12_345, "12.3k"),
        (400_000, "400k"),
        (999_949, "999.9k"),
        (999_999, "1000k"),
        (1_000_000, "1M"),
        (1_234_567, "1.2M"),
    ];
    for (n, want) in counts {
        assert_eq!(tokens(n), want, "{n} tokens");
    }
}

#[test]
fn uses_unit_messages() {
    assert_eq!(
        tokens(12_345),
        t::units::thousands(&t::fmt::decimal(12.3, 1))
    );
    assert_eq!(
        tokens(1_200_000),
        t::units::millions(&t::fmt::decimal(1.2, 1))
    );
    assert_eq!(tokens(400_000), t::units::thousands("400"));
    assert_eq!(age(NOW, NOW - 30_000), t::units::age_now());
    assert_eq!(age(NOW, NOW - 5 * 60_000), t::units::age_minutes(5));
    assert_eq!(age(NOW, NOW - 2 * 3_600_000), t::units::age_hours(2));
    assert_eq!(age(NOW, NOW - 3 * 86_400_000), t::units::age_days(3));
    assert_eq!(duration(12.0), t::units::duration_seconds(12));
    assert_eq!(duration(245.0), t::units::duration_minutes(4, "05"));
    assert_eq!(duration(4_320.0), t::units::duration_hours(1, 12));
}
