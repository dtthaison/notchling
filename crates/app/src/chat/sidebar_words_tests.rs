use notchling_engine::limits::report::{Bar, BarKind, Group, UsageReport};
use notchling_engine::sessions::AgentGroup;
use notchling_locale as t;

use super::data::Tool;
use super::limit_ring::{antigravity_limits, card, reset_clock, reset_label, Kind, Limit};
use super::sidebar::sections;
use super::sidebar::tests::session;
use crate::accounts::limits::Zone;

const NOW: f64 = 1_800_000_000.0;

fn limit(name: &str, kind: Kind, resets_at: Option<f64>) -> Limit {
    Limit {
        name: name.into(),
        kind,
        percent: 10.0,
        resets_at,
    }
}

#[test]
fn section_titles_are_messages() {
    let all = [
        session("a", AgentGroup::NeedsInput, None, Tool::Claude),
        session("b", AgentGroup::Working, None, Tool::Claude),
        session("c", AgentGroup::Completed, None, Tool::Claude),
    ];
    let titles: Vec<&str> = sections(&all, 0).iter().map(|s| s.title).collect();
    assert_eq!(
        titles,
        [t::chat::needs_input(), t::chat::working(), t::chat::done()]
    );
    assert_eq!(titles, ["Needs input", "Working", "Done"]);
}

#[test]
fn the_limit_card_is_made_of_messages() {
    let limits = [
        limit("5-hour", Kind::Session, Some(NOW + 3_600.0)),
        limit("weekly", Kind::Week, None),
        limit("Sonnet", Kind::Other, None),
        limit("Opus weekly", Kind::Other, None),
    ];
    let c = card(&limits, Some("Team"), NOW, Zone::UTC).unwrap();
    assert_eq!(c.title, t::chat::limits_title_plan("Team"));
    assert_eq!(c.title, "Your usage limits · Team");
    let labels: Vec<&str> = c.rows.iter().map(|r| r.label.as_str()).collect();
    assert_eq!(
        labels,
        [
            t::chat::limit_five_hour(),
            t::chat::limit_weekly(),
            t::chat::limit_named("Sonnet").as_str(),
            "Opus weekly",
        ]
    );
    assert_eq!(
        labels,
        [
            "5-hour limit",
            "Weekly · all models",
            "Sonnet limit",
            "Opus weekly"
        ]
    );
    let plain = card(&limits, None, NOW, Zone::UTC).unwrap();
    assert_eq!(plain.title, t::chat::limits_title());
    assert_eq!(plain.title, "Your usage limits");
}

#[test]
fn a_reset_is_a_message_over_a_clock() {
    let friday_nine = NOW + 3_600.0;
    assert_eq!(
        reset_clock(friday_nine, Zone::UTC),
        t::units::reset_clock(t::units::WEEKDAY_FRI.get(), 9, "00", t::units::AM.get())
    );
    assert_eq!(reset_clock(friday_nine, Zone::UTC), "Fri 9:00 AM");
    assert_eq!(
        reset_label(friday_nine, NOW, Zone::UTC),
        t::chat::limit_resets("Fri 9:00 AM")
    );
    assert_eq!(
        reset_label(friday_nine, NOW, Zone::UTC),
        "Resets Fri 9:00 AM"
    );
    assert_eq!(
        reset_label(NOW - 1.0, NOW, Zone::UTC),
        t::chat::limit_resetting()
    );
    assert_eq!(reset_label(NOW - 1.0, NOW, Zone::UTC), "Resetting");
}

#[test]
fn every_weekday_and_both_halves_of_the_day_read_as_they_did() {
    let days = [
        t::units::WEEKDAY_SUN,
        t::units::WEEKDAY_MON,
        t::units::WEEKDAY_TUE,
        t::units::WEEKDAY_WED,
        t::units::WEEKDAY_THU,
        t::units::WEEKDAY_FRI,
        t::units::WEEKDAY_SAT,
    ];
    let words: Vec<&str> = days.iter().map(|d| d.get()).collect();
    assert_eq!(words, ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"]);
    assert_eq!(t::units::AM.get(), "AM");
    assert_eq!(t::units::PM.get(), "PM");
}

#[test]
fn another_providers_windows_are_named_by_message() {
    let report = UsageReport {
        groups: vec![Group {
            name: Some("Gemini".into()),
            bars: vec![
                Bar {
                    label: "Pro".into(),
                    kind: BarKind::Week,
                    percent: 5.0,
                    resets_at: None,
                },
                Bar {
                    label: "Pro".into(),
                    kind: BarKind::Session,
                    percent: 5.0,
                    resets_at: None,
                },
                Bar {
                    label: "Flash".into(),
                    kind: BarKind::Other,
                    percent: 5.0,
                    resets_at: None,
                },
            ],
        }],
        fetched: NOW,
    };
    let names: Vec<String> = antigravity_limits(&report, Some("gemini-3-pro"))
        .into_iter()
        .map(|l| l.name)
        .collect();
    assert_eq!(
        names,
        [
            t::chat::limit_window("Gemini", t::chat::limit_five_hour()),
            t::chat::limit_window("Gemini", t::chat::limit_weekly_short()),
            t::chat::limit_window("Gemini", "Flash"),
        ]
    );
    assert_eq!(
        names,
        [
            "Gemini · 5-hour limit",
            "Gemini · weekly limit",
            "Gemini · Flash"
        ]
    );
}

#[test]
fn a_delegated_job_is_titled_by_its_tool() {
    assert_eq!(t::chat::delegated_codex(), "Codex");
    assert_eq!(t::chat::delegated_antigravity(), "Antigravity CLI");
}

#[test]
fn the_menus_words_are_what_they_were() {
    assert_eq!(t::chat::copy_session_id(), "Copy Session ID");
    assert_eq!(t::chat::close_all_codex(), "Close All Codex Sessions");
    assert_eq!(
        t::chat::close_all_antigravity(),
        "Close All Antigravity Sessions"
    );
    assert_eq!(t::chat::remove_session(), "Remove Session…");
    assert_eq!(t::chat::remove_title("night"), "Remove “night”?");
    assert_eq!(
        t::chat::remove_note(),
        "Claude Code stops it and removes it from its list. This can't be undone."
    );
    assert_eq!(t::chat::codex_accounts(), "Codex Accounts…");
    assert_eq!(t::chat::move_failed("no"), "Couldn't move it: no");
    assert_eq!(
        t::chat::save_used_failed("no"),
        "Couldn't save the sessions used in Chat: no"
    );
    assert_eq!(
        t::chat::save_closed_failed("no"),
        "Couldn't save the closed sessions: no"
    );
}

#[test]
fn the_phone_buttons_tips_are_what_they_were() {
    assert_eq!(
        t::chat::phone_tip_on(),
        "Phone: the bridge is on. Pair or manage phones"
    );
    assert_eq!(
        t::chat::phone_tip_off(),
        "Phone: pair a phone with this Mac"
    );
}
