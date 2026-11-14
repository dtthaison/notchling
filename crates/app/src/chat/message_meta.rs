use notchling_engine::pages::clock::{civil_at, days_from_civil, local_offset_secs};
use notchling_locale as t;

pub const COPIED_MS: i64 = 1500;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    User,
    Assistant,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Plan {
    pub delivered: bool,
    pub copy: bool,
    pub time: bool,
}

impl Plan {
    pub fn any(self) -> bool {
        self.delivered || self.copy || self.time
    }
}

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

const STALE_MS: i64 = 30_000;

pub fn stale(drawn_ms: i64, now_ms: i64) -> bool {
    now_ms - drawn_ms >= STALE_MS
}

pub fn copied_shows(copied_at_ms: i64, now_ms: i64) -> bool {
    (0..COPIED_MS).contains(&(now_ms - copied_at_ms))
}

const MONTHS: [t::Text; 12] = [
    t::units::MONTH_JAN,
    t::units::MONTH_FEB,
    t::units::MONTH_MAR,
    t::units::MONTH_APR,
    t::units::MONTH_MAY,
    t::units::MONTH_JUN,
    t::units::MONTH_JUL,
    t::units::MONTH_AUG,
    t::units::MONTH_SEP,
    t::units::MONTH_OCT,
    t::units::MONTH_NOV,
    t::units::MONTH_DEC,
];
const MONTHS_LONG: [t::Text; 12] = [
    t::units::MONTH_LONG_JAN,
    t::units::MONTH_LONG_FEB,
    t::units::MONTH_LONG_MAR,
    t::units::MONTH_LONG_APR,
    t::units::MONTH_LONG_MAY,
    t::units::MONTH_LONG_JUN,
    t::units::MONTH_LONG_JUL,
    t::units::MONTH_LONG_AUG,
    t::units::MONTH_LONG_SEP,
    t::units::MONTH_LONG_OCT,
    t::units::MONTH_LONG_NOV,
    t::units::MONTH_LONG_DEC,
];
const WEEKDAYS: [t::Text; 7] = [
    t::units::WEEKDAY_LONG_SUNDAY,
    t::units::WEEKDAY_LONG_MONDAY,
    t::units::WEEKDAY_LONG_TUESDAY,
    t::units::WEEKDAY_LONG_WEDNESDAY,
    t::units::WEEKDAY_LONG_THURSDAY,
    t::units::WEEKDAY_LONG_FRIDAY,
    t::units::WEEKDAY_LONG_SATURDAY,
];

fn clock_text(hour: u32, minute: u32) -> String {
    let h = match hour % 12 {
        0 => 12,
        h => h,
    };
    let suffix = if hour < 12 {
        t::units::AM
    } else {
        t::units::PM
    };
    t::units::clock(h, &format!("{minute:02}"), suffix.get())
}

fn local_day(ts: f64, offset: i64) -> i64 {
    (ts.floor() as i64 + offset).div_euclid(86_400)
}

pub fn relative_time(now: f64, at: f64, offset: i64) -> String {
    let seconds = (now - at).floor() as i64;
    if seconds < 60 {
        return t::units::just_now().to_owned();
    }
    if seconds < 3600 {
        return t::units::minutes_ago(seconds / 60);
    }
    let days_ago = local_day(now, offset) - local_day(at, offset);
    if days_ago <= 0 {
        return t::units::hours_ago(seconds / 3600);
    }
    let then = civil_at(at, offset);
    if days_ago == 1 {
        return t::units::yesterday(&clock_text(then.hour, then.minute));
    }
    let month = MONTHS[(then.month as usize - 1) % 12].get();
    if civil_at(now, offset).year == then.year {
        t::units::date_short(month, &then.day.to_string())
    } else {
        t::units::date_year(month, &then.day.to_string(), &then.year.to_string())
    }
}

pub fn full_time(at: f64, offset: i64) -> String {
    let c = civil_at(at, offset);
    let day = days_from_civil(c.year, i64::from(c.month), i64::from(c.day));
    let weekday = WEEKDAYS[(day + 4).rem_euclid(7) as usize].get();
    t::units::date_full(
        weekday,
        MONTHS_LONG[(c.month as usize - 1) % 12].get(),
        &c.day.to_string(),
        &c.year.to_string(),
        &clock_text(c.hour, c.minute),
    )
}

pub fn relative_local(now: f64, at: f64) -> String {
    relative_time(now, at, local_offset_secs(at))
}

pub fn full_local(at: f64) -> String {
    full_time(at, local_offset_secs(at))
}

#[cfg(test)]
mod tests {
    use super::*;

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
        assert!(!plan(Role::Assistant, false, false, "").any());
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
        let yesterday = 20_765.0 * 86_400.0 + 15.0 * 3600.0 + 12.0 * 60.0;
        assert_eq!(relative_time(NOW, yesterday, 0), "Yesterday 3:12 PM");
        let late = 20_765.0 * 86_400.0 + 23.5 * 3600.0;
        assert_eq!(relative_time(NOW, late, 0), "Yesterday 11:30 PM");
        let earlier = 20_760.0 * 86_400.0 + 5.0 * 60.0;
        assert_eq!(relative_time(NOW, earlier, 0), "Nov 3");
        let last_year = days_from_civil(2025, 12, 25) as f64 * 86_400.0 + 12.0 * 3600.0;
        assert_eq!(relative_time(NOW, last_year, 0), "Dec 25, 2025");
    }

    #[test]
    fn the_zone_moves_the_day() {
        let at = 20_765.0 * 86_400.0 + 22.0 * 3600.0;
        assert_eq!(relative_time(NOW, at, 7 * 3600), "Yesterday 5:00 AM");
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
