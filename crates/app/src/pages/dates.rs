//! The dates the pages list shows (the Swift `PageDates`): "Nov 8" in a row, "Nov 8, 2026 14:05"
//! in the detail. In UTC: the app has no time zone reader yet, and a date off by a few hours in a
//! row is better than a crate for it; say so where the text is read as local time.

/// The civil date of a day count since 1970-01-01 (Howard Hinnant's algorithm).
pub fn civil_from_days(days: i64) -> (i64, u32, u32) {
    todo!()
}

/// "Nov 8", from seconds since 1970.
pub fn short(secs: f64) -> String {
    todo!()
}

/// "Nov 8, 2026 14:05", from seconds since 1970.
pub fn long(secs: f64) -> String {
    todo!()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn days_become_civil_dates() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(-1), (1969, 12, 31));
        assert_eq!(civil_from_days(10_957), (2000, 1, 1));
        assert_eq!(civil_from_days(20_833), (2027, 1, 14));
        // 1_800_000_000 s = 2027-01-15 08:00:00 UTC.
        assert_eq!(civil_from_days(1_800_000_000 / 86_400), (2027, 1, 15));
    }

    #[test]
    fn the_short_and_long_forms_read_like_the_swift_ones() {
        assert_eq!(short(1_800_000_000.0), "Jan 15");
        assert_eq!(long(1_800_000_000.0), "Jan 15, 2027 08:00");
        assert_eq!(long(1_800_000_000.0 + 5.0 * 3_600.0 + 7.0 * 60.0 + 9.0), "Jan 15, 2027 13:07");
        assert_eq!(short(0.0), "Jan 1");
        assert_eq!(long(0.0), "Jan 1, 1970 00:00");
        // A fraction of a second, or a time before 1970, still reads.
        assert_eq!(short(951_782_400.5), "Feb 29");
        assert_eq!(long(-1.0), "Dec 31, 1969 23:59");
    }
}
