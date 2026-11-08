//! Local dates without a date crate: the list page and `describe` show local times, as Swift's
//! formatters do.

/// Seconds the local time zone is ahead of UTC at `ts` (seconds since 1970). Zero where the system
/// can't say, and on Windows (not built yet).
// `tm_gmtoff` is an `i64` on 64-bit systems and an `i32` on 32-bit ones.
#[cfg(unix)]
#[allow(clippy::useless_conversion)]
pub fn local_offset_secs(ts: f64) -> i64 {
    let t = ts as libc::time_t;
    // SAFETY: an all-zero `tm` is a valid value for localtime_r to overwrite, and both pointers
    // are to live locals.
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    let got = unsafe { libc::localtime_r(&t, &mut tm) };
    if got.is_null() {
        0
    } else {
        i64::from(tm.tm_gmtoff)
    }
}

#[cfg(not(unix))]
pub fn local_offset_secs(_ts: f64) -> i64 {
    0
}

/// A civil date and time of day.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Civil {
    pub year: i64,
    pub month: u32,
    pub day: u32,
    pub hour: u32,
    pub minute: u32,
    pub second: u32,
}

/// Days since 1970-01-01 to a civil date (Howard Hinnant's algorithm).
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let year = yoe + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

/// The civil time of `ts` shifted by `offset_secs`.
pub fn civil_at(ts: f64, offset_secs: i64) -> Civil {
    let secs = ts.floor() as i64 + offset_secs;
    let (year, month, day) = civil_from_days(secs.div_euclid(86_400));
    let in_day = secs.rem_euclid(86_400) as u32;
    Civil {
        year,
        month,
        day,
        hour: in_day / 3600,
        minute: in_day % 3600 / 60,
        second: in_day % 60,
    }
}

/// `2026-11-08 15:04:05` in the local zone.
pub fn local_text(ts: f64) -> String {
    text(civil_at(ts, local_offset_secs(ts)), true)
}

/// `2026-11-08 15:04` in the local zone.
pub fn local_text_minutes(ts: f64) -> String {
    text(civil_at(ts, local_offset_secs(ts)), false)
}

/// The date and time as `yyyy-MM-dd HH:mm[:ss]`.
pub fn text(c: Civil, seconds: bool) -> String {
    let base = format!(
        "{:04}-{:02}-{:02} {:02}:{:02}",
        c.year, c.month, c.day, c.hour, c.minute
    );
    if seconds {
        format!("{base}:{:02}", c.second)
    } else {
        base
    }
}

/// Days since 1970-01-01 of a civil date, for comparing folder names like `2026/11/08` with a time.
pub fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let mp = if month > 2 { month - 3 } else { month + 9 };
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn civil_dates_round_trip() {
        // 2026-11-08 15:04:05 UTC
        let ts = 20_765.0 * 86_400.0 + 15.0 * 3600.0 + 4.0 * 60.0 + 5.0;
        let c = civil_at(ts, 0);
        assert_eq!(text(c, true), "2026-11-08 15:04:05");
        assert_eq!(days_from_civil(2026, 11, 8), 20_765);
        assert_eq!(
            civil_at(0.0, 0),
            Civil {
                year: 1970,
                month: 1,
                day: 1,
                hour: 0,
                minute: 0,
                second: 0
            }
        );
        // A zone ahead of UTC moves the date over midnight.
        assert_eq!(text(civil_at(86_399.0, 3600), false), "1970-01-02 00:59");
        // Before 1970.
        assert_eq!(text(civil_at(-1.0, 0), true), "1969-12-31 23:59:59");
    }
}
