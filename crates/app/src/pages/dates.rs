const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

pub fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m as u32, d as u32)
}

fn split(secs: f64) -> (i64, i64) {
    let whole = secs.floor() as i64;
    (whole.div_euclid(86_400), whole.rem_euclid(86_400))
}

pub fn short(secs: f64) -> String {
    let (days, _) = split(secs);
    let (_, m, d) = civil_from_days(days);
    format!("{} {d}", MONTHS[(m - 1) as usize])
}

pub fn long(secs: f64) -> String {
    let (days, rest) = split(secs);
    let (y, m, d) = civil_from_days(days);
    format!(
        "{} {d}, {y} {:02}:{:02}",
        MONTHS[(m - 1) as usize],
        rest / 3600,
        (rest % 3600) / 60
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn days_become_civil_dates() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(-1), (1969, 12, 31));
        assert_eq!(civil_from_days(10_957), (2000, 1, 1));
        assert_eq!(civil_from_days(20_832), (2027, 1, 14));
        assert_eq!(civil_from_days(1_800_000_000 / 86_400), (2027, 1, 15));
    }

    #[test]
    fn the_short_and_long_forms_read_like_the_swift_ones() {
        assert_eq!(short(1_800_000_000.0), "Jan 15");
        assert_eq!(long(1_800_000_000.0), "Jan 15, 2027 08:00");
        assert_eq!(
            long(1_800_000_000.0 + 5.0 * 3_600.0 + 7.0 * 60.0 + 9.0),
            "Jan 15, 2027 13:07"
        );
        assert_eq!(short(0.0), "Jan 1");
        assert_eq!(long(0.0), "Jan 1, 1970 00:00");
        assert_eq!(short(951_782_400.5), "Feb 29");
        assert_eq!(long(-1.0), "Dec 31, 1969 23:59");
    }
}
