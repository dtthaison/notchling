use super::logic::Fired;
use super::usage::UsageWindow;
use crate::pages::clock::{civil_at, local_offset_secs};

#[derive(Clone, Copy)]
pub struct Zone(pub fn(f64) -> i64);

impl Zone {
    pub const LOCAL: Zone = Zone(local_offset_secs);
    pub const UTC: Zone = Zone(|_| 0);
}

const WEEKDAYS: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];

pub fn clock(at: f64, now: f64, zone: Zone) -> String {
    let offset = (zone.0)(at);
    let c = civil_at(at, offset);
    let today = civil_at(now, (zone.0)(now));
    let hour12 = match c.hour % 12 {
        0 => 12,
        h => h,
    };
    let time = format!(
        "{hour12}:{:02} {}",
        c.minute,
        if c.hour < 12 { "AM" } else { "PM" }
    );
    if (c.year, c.month, c.day) == (today.year, today.month, today.day) {
        return time;
    }
    let days = (at.floor() as i64 + offset).div_euclid(86_400);
    let weekday = (days + 4).rem_euclid(7) as usize;
    format!("{} {time}", WEEKDAYS[weekday])
}

pub fn resets(window: &UsageWindow, now: f64, zone: Zone) -> String {
    window
        .resets_at
        .map(|at| format!(", resets {}", clock(at, now, zone)))
        .unwrap_or_default()
}

pub fn climb(prefix: &str, who: &str, fired: &Fired, now: f64, zone: Zone) -> String {
    let pct = fired.window.percent.round() as i64;
    let tail = resets(&fired.window, now, zone);
    if fired.level >= 100 {
        format!(
            "{prefix}{who} has used its {} limit ({pct}%){tail}.",
            fired.name
        )
    } else {
        format!(
            "{prefix}{who} is at {pct}% of its {} limit{tail}.",
            fired.name
        )
    }
}
