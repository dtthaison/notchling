use notchling_engine::pages::clock::{days_from_civil, local_offset_secs};

pub const USAGE: &str =
    "--since takes a duration (30m, 24h, 7d), a date (2026-11-01) or epoch seconds";

fn duration(text: &str) -> Option<f64> {
    let unit = text.chars().last()?;
    let per = match unit {
        's' => 1.0,
        'm' => 60.0,
        'h' => 3600.0,
        'd' => 86_400.0,
        'w' => 604_800.0,
        _ => return None,
    };
    let digits = &text[..text.len() - 1];
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    digits.parse::<f64>().ok().map(|n| n * per)
}

fn days_in(year: i64, month: i64) -> i64 {
    match month {
        2 if year % 4 == 0 && (year % 100 != 0 || year % 400 == 0) => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

fn date(text: &str) -> Option<f64> {
    let b = text.as_bytes();
    if b.len() != 10 || b[4] != b'-' || b[7] != b'-' {
        return None;
    }
    let number = |part: &str| -> Option<i64> {
        part.bytes()
            .all(|c| c.is_ascii_digit())
            .then(|| part.parse().ok())
            .flatten()
    };
    let (year, month, day) = (
        number(&text[0..4])?,
        number(&text[5..7])?,
        number(&text[8..10])?,
    );
    if !(1..=12).contains(&month) || day < 1 || day > days_in(year, month) {
        return None;
    }
    let utc = days_from_civil(year, month, day) * 86_400;
    let guess = utc - local_offset_secs(utc as f64);
    let local = utc - local_offset_secs(guess as f64);
    (local >= 0).then_some(local as f64)
}

pub fn parse(text: &str, now: f64) -> Option<f64> {
    if let Some(back) = duration(text) {
        return Some((now - back).max(0.0));
    }
    if let Some(midnight) = date(text) {
        return Some(midnight);
    }
    text.parse::<f64>()
        .ok()
        .filter(|s| s.is_finite() && *s >= 0.0)
}

#[cfg(test)]
#[path = "search_since_tests.rs"]
mod tests;
