use crate::pages::clock::{civil_at, days_from_civil, local_offset_secs};

pub const RENEW_SOON_DAYS: i64 = 7;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Expired,
    Today,
    InDays(i64),
}

fn ymd(text: &str) -> Option<(i64, i64, i64)> {
    let b = text.as_bytes();
    let shaped = b.len() == 10
        && b[4] == b'-'
        && b[7] == b'-'
        && b.iter()
            .enumerate()
            .all(|(i, c)| i == 4 || i == 7 || c.is_ascii_digit());
    if !shaped {
        return None;
    }
    let year: i64 = text.get(0..4)?.parse().ok()?;
    let month: i64 = text.get(5..7)?.parse().ok()?;
    let day: i64 = text.get(8..10)?.parse().ok()?;
    let leap = (year % 4 == 0 && year % 100 != 0) || year % 400 == 0;
    let last = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        2 => 28,
        _ => return None,
    };
    (1..=last).contains(&day).then_some((year, month, day))
}

pub fn parse(text: &str) -> Option<String> {
    let text = text.trim();
    ymd(text).map(|_| text.to_string())
}

pub fn day_number(day: &str) -> Option<i64> {
    let (y, m, d) = ymd(day.trim())?;
    Some(days_from_civil(y, m, d))
}

pub fn day_text(days: i64) -> String {
    let c = civil_at(days as f64 * 86_400.0, 0);
    format!("{:04}-{:02}-{:02}", c.year, c.month, c.day)
}

pub fn today_local(now_secs: f64) -> i64 {
    let offset = local_offset_secs(now_secs);
    (now_secs.floor() as i64 + offset).div_euclid(86_400)
}

pub fn status(day: &str, today: i64) -> Option<Status> {
    let days = day_number(day)? - today;
    Some(match days {
        d if d < 0 => Status::Expired,
        0 => Status::Today,
        d => Status::InDays(d),
    })
}

pub fn phrase(day: &str, today: i64) -> Option<String> {
    match status(day, today)? {
        Status::InDays(1) => Some("in 1 day".into()),
        Status::InDays(n) => Some(format!("in {n} days")),
        Status::Today => Some("today".into()),
        Status::Expired => None,
    }
}

pub fn line(day: &str, today: i64) -> String {
    let status = status(day, today);
    if status == Some(Status::Expired) {
        return format!("EXPIRED {day}");
    }
    let Some(phrase) = phrase(day, today) else {
        return format!("expires {day}");
    };
    let soon = match status {
        Some(Status::InDays(n)) => n <= RENEW_SOON_DAYS,
        Some(Status::Today) => true,
        _ => false,
    };
    format!(
        "expires {day} ({phrase}{})",
        if soon { ", renew soon" } else { "" }
    )
}
