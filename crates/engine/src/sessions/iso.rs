//! ISO 8601 timestamps from `state.json`, without a date crate.

/// Milliseconds since the epoch for `2026-11-02T08:21:06.159Z`, with or without a fraction, and with
/// `Z` or a `+hh:mm` offset. `None` for anything else.
pub fn parse_iso_ms(text: &str) -> Option<i64> {
    let (date, rest) = text.split_once('T')?;
    let mut d = date.split('-');
    let year: i64 = d.next()?.parse().ok()?;
    let month: i64 = d.next()?.parse().ok()?;
    let day: i64 = d.next()?.parse().ok()?;
    if d.next().is_some() || !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    let (clock, offset_ms) = split_zone(rest)?;
    let (hms, fraction) = match clock.split_once('.') {
        Some((h, f)) => (h, Some(f)),
        None => (clock, None),
    };
    let mut t = hms.split(':');
    let hour: i64 = t.next()?.parse().ok()?;
    let minute: i64 = t.next()?.parse().ok()?;
    let second: i64 = t.next()?.parse().ok()?;
    if t.next().is_some() || hour > 23 || minute > 59 || second > 60 {
        return None;
    }
    let millis = match fraction {
        None => 0,
        Some(f) if !f.is_empty() && f.bytes().all(|b| b.is_ascii_digit()) => {
            let three: String = f.chars().chain("000".chars()).take(3).collect();
            three.parse::<i64>().ok()?
        }
        Some(_) => return None,
    };
    let days = days_from_civil(year, month, day);
    Some(((days * 24 + hour) * 60 + minute) * 60_000 + second * 1000 + millis - offset_ms)
}

/// The clock part and the zone's offset in milliseconds (east of UTC positive).
fn split_zone(rest: &str) -> Option<(&str, i64)> {
    if let Some(clock) = rest.strip_suffix('Z') {
        return Some((clock, 0));
    }
    let at = rest.rfind(['+', '-'])?;
    let (clock, zone) = rest.split_at(at);
    let sign = if zone.starts_with('-') { -1 } else { 1 };
    let body = &zone[1..];
    let (h, m) = body.split_once(':').unwrap_or((body, "0"));
    let hours: i64 = h.parse().ok()?;
    let minutes: i64 = m.parse().ok()?;
    Some((clock, sign * (hours * 60 + minutes) * 60_000))
}

/// Days since 1970-01-01 of a proleptic Gregorian date (Howard Hinnant's algorithm).
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (month + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_zulu_with_fraction() {
        assert_eq!(parse_iso_ms("1970-01-01T00:00:00.000Z"), Some(0));
        assert_eq!(parse_iso_ms("2026-11-02T08:21:06.159Z"), Some(1_793_607_666_159));
    }

    #[test]
    fn fraction_is_optional_and_offsets_apply() {
        assert_eq!(parse_iso_ms("2026-11-02T08:21:06Z"), Some(1_793_607_666_000));
        assert_eq!(
            parse_iso_ms("2026-11-02T15:21:06.159+07:00"),
            parse_iso_ms("2026-11-02T08:21:06.159Z")
        );
        assert_eq!(parse_iso_ms("2026-11-02T08:21:06.5Z"), Some(1_793_607_666_500));
    }

    #[test]
    fn rejects_garbage() {
        assert_eq!(parse_iso_ms(""), None);
        assert_eq!(parse_iso_ms("yesterday"), None);
        assert_eq!(parse_iso_ms("2026-13-02T08:21:06Z"), None);
    }

    #[test]
    fn ordering_follows_time() {
        let a = parse_iso_ms("2026-11-02T08:21:06.159Z");
        let b = parse_iso_ms("2026-11-04T03:15:40.662Z");
        assert!(a < b);
    }
}
