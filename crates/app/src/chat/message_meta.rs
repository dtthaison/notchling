//! What the row under a message says and offers (the Swift app's hover bar on `AssistantMessage` and
//! `UserBubble`): the relative time from the transcript line's timestamp, its full local date for
//! the tooltip, the delivered mark, and which buttons a message gets. Pure, so it is tested on
//! every system; `chat::view::message_actions` draws it.
//!
//! Only what really works here is offered: Copy. Rewind is Claude Code's terminal menu, Fork would
//! launch a new process (`claude --resume --fork-session`), and nothing in the Swift app or the
//! daemon's ops does either from Chat, so neither has a button.

use notchling_engine::pages::clock::{civil_at, days_from_civil, local_offset_secs};

/// How long Copy shows its check after a click.
pub const COPIED_MS: i64 = 1500;

/// Who wrote the message.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    User,
    Assistant,
}

/// What a message's row holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Plan {
    /// The double check: your message is in the transcript, not waiting in the queue.
    pub delivered: bool,
    /// Copy puts the message's text on the clipboard.
    pub copy: bool,
    /// The time, when the transcript line had one.
    pub time: bool,
}

impl Plan {
    /// Whether there is a row at all.
    pub fn any(self) -> bool {
        self.delivered || self.copy || self.time
    }
}

/// What a message gets. A queued message has no row: it says "Queued" and isn't in the transcript
/// yet, so it has no time and isn't delivered. Copy needs text (a message of only images has none).
pub fn plan(role: Role, queued: bool, has_date: bool, text: &str) -> Plan {
    if queued {
        return Plan {
            delivered: false,
            copy: false,
            time: false,
        };
    }
    Plan {
        delivered: role == Role::User,
        copy: !text.trim().is_empty(),
        time: has_date,
    }
}

/// How old a drawn relative time may get before a hovered row redraws it, in ms.
const STALE_MS: i64 = 30_000;

/// Whether the times drawn at `drawn_ms` need drawing again at `now_ms`.
pub fn stale(drawn_ms: i64, now_ms: i64) -> bool {
    now_ms - drawn_ms >= STALE_MS
}

/// Whether a click at `copied_at_ms` still shows the check at `now_ms`.
pub fn copied_shows(copied_at_ms: i64, now_ms: i64) -> bool {
    (0..COPIED_MS).contains(&(now_ms - copied_at_ms))
}

const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];
const MONTHS_LONG: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];
/// 1970-01-01 was a Thursday, so day 0 is index 4 of a Sunday-first week.
const WEEKDAYS: [&str; 7] = [
    "Sunday",
    "Monday",
    "Tuesday",
    "Wednesday",
    "Thursday",
    "Friday",
    "Saturday",
];

/// "3:12 PM".
fn clock_text(hour: u32, minute: u32) -> String {
    let h = match hour % 12 {
        0 => 12,
        h => h,
    };
    let ampm = if hour < 12 { "AM" } else { "PM" };
    format!("{h}:{minute:02} {ampm}")
}

fn local_day(ts: f64, offset: i64) -> i64 {
    (ts.floor() as i64 + offset).div_euclid(86_400)
}

fn plural(n: i64, unit: &str) -> String {
    if n == 1 {
        format!("1 {unit} ago")
    } else {
        format!("{n} {unit}s ago")
    }
}

/// "just now", "4 minutes ago", "2 hours ago", "Yesterday 3:12 PM", else the date ("Nov 3", and
/// the year when it isn't this one). Seconds since the epoch; `offset` is the local zone's lead on
/// UTC. A time ahead of `now` (a skewed clock) reads as just now.
pub fn relative_time(now: f64, at: f64, offset: i64) -> String {
    let seconds = (now - at).floor() as i64;
    if seconds < 60 {
        return "just now".to_owned();
    }
    if seconds < 3600 {
        return plural(seconds / 60, "minute");
    }
    let days_ago = local_day(now, offset) - local_day(at, offset);
    if days_ago <= 0 {
        return plural(seconds / 3600, "hour");
    }
    let then = civil_at(at, offset);
    if days_ago == 1 {
        return format!("Yesterday {}", clock_text(then.hour, then.minute));
    }
    let month = MONTHS[(then.month as usize - 1) % 12];
    if civil_at(now, offset).year == then.year {
        format!("{month} {}", then.day)
    } else {
        format!("{month} {}, {}", then.day, then.year)
    }
}

/// "Monday, November 9, 2026 at 11:06 PM", the tooltip.
pub fn full_time(at: f64, offset: i64) -> String {
    let c = civil_at(at, offset);
    let day = days_from_civil(c.year, i64::from(c.month), i64::from(c.day));
    let weekday = WEEKDAYS[(day + 4).rem_euclid(7) as usize];
    format!(
        "{weekday}, {} {}, {} at {}",
        MONTHS_LONG[(c.month as usize - 1) % 12],
        c.day,
        c.year,
        clock_text(c.hour, c.minute)
    )
}

/// [`relative_time`] in the local zone.
pub fn relative_local(now: f64, at: f64) -> String {
    relative_time(now, at, local_offset_secs(at))
}

/// [`full_time`] in the local zone.
pub fn full_local(at: f64) -> String {
    full_time(at, local_offset_secs(at))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 2026-11-09 23:10:00 UTC (a Monday).
    const NOW: f64 = 20_766.0 * 86_400.0 + 23.0 * 3600.0 + 10.0 * 60.0;

    #[test]
    fn a_queued_message_has_no_row() {
        let plan = plan(Role::User, true, true, "next");
        assert!(!plan.any());
    }

    #[test]
    fn your_sent_message_is_delivered_and_can_be_copied() {
        let p = plan(Role::User, false, true, "hi");
        assert_eq!(
            p,
            Plan {
                delivered: true,
                copy: true,
                time: true
            }
        );
    }

    #[test]
    fn a_reply_has_no_delivered_mark() {
        let p = plan(Role::Assistant, false, true, "Done.");
        assert_eq!(
            p,
            Plan {
                delivered: false,
                copy: true,
                time: true
            }
        );
    }

    #[test]
    fn no_text_means_no_copy_and_no_date_means_no_time() {
        let images = plan(Role::User, false, true, "  ");
        assert!(!images.copy && images.time && images.delivered);
        let undated = plan(Role::Assistant, false, false, "x");
        assert!(undated.copy && !undated.time);
        // A reply with neither has no row.
        assert!(!plan(Role::Assistant, false, false, "").any());
        // A message of only images, undated, still shows its delivered mark.
        assert!(plan(Role::User, false, false, "").any());
    }

    #[test]
    fn drawn_times_go_stale() {
        assert!(!stale(1000, 1000 + STALE_MS - 1));
        assert!(stale(1000, 1000 + STALE_MS));
    }

    #[test]
    fn the_check_shows_for_a_moment() {
        assert!(copied_shows(1000, 1000));
        assert!(copied_shows(1000, 1000 + COPIED_MS - 1));
        assert!(!copied_shows(1000, 1000 + COPIED_MS));
        assert!(!copied_shows(1000, 999), "a click from the future");
    }

    #[test]
    fn minutes_and_hours() {
        assert_eq!(relative_time(NOW, NOW - 5.0, 0), "just now");
        assert_eq!(relative_time(NOW, NOW + 30.0, 0), "just now");
        assert_eq!(relative_time(NOW, NOW - 59.0, 0), "just now");
        assert_eq!(relative_time(NOW, NOW - 60.0, 0), "1 minute ago");
        assert_eq!(
            relative_time(NOW, NOW - 4.0 * 60.0 - 10.0, 0),
            "4 minutes ago"
        );
        assert_eq!(relative_time(NOW, NOW - 3599.0, 0), "59 minutes ago");
        assert_eq!(relative_time(NOW, NOW - 3600.0, 0), "1 hour ago");
        assert_eq!(
            relative_time(NOW, NOW - 2.0 * 3600.0 - 5.0, 0),
            "2 hours ago"
        );
        assert_eq!(relative_time(NOW, NOW - 23.0 * 3600.0, 0), "23 hours ago");
    }

    #[test]
    fn yesterday_dates_and_years() {
        // 2026-11-08 15:12 UTC.
        let yesterday = 20_765.0 * 86_400.0 + 15.0 * 3600.0 + 12.0 * 60.0;
        assert_eq!(relative_time(NOW, yesterday, 0), "Yesterday 3:12 PM");
        // Less than 24 hours but over midnight is yesterday, not hours ago.
        let late = 20_765.0 * 86_400.0 + 23.5 * 3600.0;
        assert_eq!(relative_time(NOW, late, 0), "Yesterday 11:30 PM");
        // 2026-11-03 00:05 UTC.
        let earlier = 20_760.0 * 86_400.0 + 5.0 * 60.0;
        assert_eq!(relative_time(NOW, earlier, 0), "Nov 3");
        // 2025-12-25 noon UTC.
        let last_year = days_from_civil(2025, 12, 25) as f64 * 86_400.0 + 12.0 * 3600.0;
        assert_eq!(relative_time(NOW, last_year, 0), "Dec 25, 2025");
    }

    #[test]
    fn the_zone_moves_the_day() {
        // 23:10 UTC is 06:10 the next day at +7; a message at 22:00 UTC the day before is
        // yesterday there at 5:00 AM.
        let at = 20_765.0 * 86_400.0 + 22.0 * 3600.0;
        assert_eq!(relative_time(NOW, at, 7 * 3600), "Yesterday 5:00 AM");
        // Three hours earlier is the same local day.
        let near = NOW - 3.0 * 3600.0;
        assert_eq!(relative_time(NOW, near, 0), "3 hours ago");
    }

    #[test]
    fn the_tooltip_has_the_weekday_date_and_time() {
        let at = 20_766.0 * 86_400.0 + 23.0 * 3600.0 + 6.0 * 60.0;
        assert_eq!(full_time(at, 0), "Monday, November 9, 2026 at 11:06 PM");
        assert_eq!(full_time(0.0, 0), "Thursday, January 1, 1970 at 12:00 AM");
        assert_eq!(
            full_time(12.0 * 3600.0 + 5.0 * 60.0, 0),
            "Thursday, January 1, 1970 at 12:05 PM"
        );
    }
}
