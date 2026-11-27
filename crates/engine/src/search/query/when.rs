use crate::pages::clock::{civil_at, days_from_civil};

const DAY: i64 = 86_400;
const MAX_DAYS: u32 = 3650;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum When {
    Today,
    Days(u32),
    Month { year: i32, month: u32 },
}

pub fn parse_when(token: &str) -> Option<When> {
    let token = token.to_ascii_lowercase();
    if token == "today" {
        return Some(When::Today);
    }
    if let Some(n) = token.strip_suffix('d') {
        if n.is_empty() || !n.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        let n: u32 = n.parse().ok()?;
        return (1..=MAX_DAYS).contains(&n).then_some(When::Days(n));
    }
    let (year, month) = token.split_once('-')?;
    if year.len() != 4 || month.len() != 2 {
        return None;
    }
    let year: i32 = year.parse().ok()?;
    let month: u32 = month.parse().ok()?;
    (1..=12)
        .contains(&month)
        .then_some(When::Month { year, month })
}

fn month_start(year: i32, month: u32, offset_secs: i64) -> f64 {
    (days_from_civil(i64::from(year), i64::from(month), 1) * DAY - offset_secs) as f64
}

pub fn bounds(when: When, now: f64, offset_secs: i64) -> (f64, f64) {
    match when {
        When::Today => {
            let c = civil_at(now, offset_secs);
            let midnight = days_from_civil(c.year, i64::from(c.month), i64::from(c.day)) * DAY;
            ((midnight - offset_secs) as f64, now)
        }
        When::Days(n) => (now - f64::from(n) * DAY as f64, now),
        When::Month { year, month } => {
            let (next_year, next_month) = if month == 12 {
                (year + 1, 1)
            } else {
                (year, month + 1)
            };
            let after = month_start(year, month, offset_secs);
            let end = month_start(next_year, next_month, offset_secs);
            (after, end.min(now))
        }
    }
}
