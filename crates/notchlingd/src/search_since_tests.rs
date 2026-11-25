use super::*;
use notchling_engine::pages::clock::local_text;

const NOW: f64 = 1_790_000_000.0;

#[test]
fn durations_count_back_from_now() {
    assert_eq!(parse("30m", NOW), Some(NOW - 1800.0));
    assert_eq!(parse("24h", NOW), Some(NOW - 86_400.0));
    assert_eq!(parse("7d", NOW), Some(NOW - 7.0 * 86_400.0));
    assert_eq!(parse("90s", NOW), Some(NOW - 90.0));
    assert_eq!(parse("2w", NOW), Some(NOW - 14.0 * 86_400.0));
    assert_eq!(parse("0d", NOW), Some(NOW));
}

#[test]
fn a_duration_longer_than_the_clock_stops_at_zero() {
    assert_eq!(parse("99999999d", NOW), Some(0.0));
}

#[test]
fn a_date_is_local_midnight() {
    let at = parse("2026-11-01", NOW).unwrap();
    assert_eq!(&local_text(at)[..], "2026-11-01 00:00:00");
    let leap = parse("2028-02-29", NOW).unwrap();
    assert_eq!(&local_text(leap)[..10], "2028-02-29");
}

#[test]
fn epoch_seconds_pass_through_as_a_float() {
    assert_eq!(parse("1763900000", NOW), Some(1_763_900_000.0));
    assert_eq!(parse("0", NOW), Some(0.0));
    assert_eq!(parse("12.5", NOW), Some(12.5));
}

#[test]
fn anything_else_is_refused() {
    for bad in [
        "7x",
        "d",
        "-1",
        "-5m",
        "1.5h",
        "NaN",
        "inf",
        "-inf",
        "soon",
        "",
        "2026-13-01",
        "2026-02-30",
        "2027-02-29",
        "2026-1-1",
        "26-11-01",
        "2026/11/01",
        "2026-11-01T10:00",
        "m",
        "7 d",
    ] {
        assert_eq!(parse(bad, NOW), None, "{bad:?}");
    }
}
