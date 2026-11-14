use notchling_locale as t;
use serde_json::json;

use super::model::{time_label, ActivityModel, Entry, Kind, Load};

fn entry(source: &str, detail: Option<&str>) -> Entry {
    Entry::parse(&json!({
        "time_ms": 1, "source": source, "kind": "acted", "op": "session.reply",
        "who": "Pixel 9", "result": "ok", "detail": detail.unwrap_or("")
    }))
    .unwrap()
}

#[test]
fn filter_and_empty_texts_use_messages() {
    let labels = Kind::ALL.map(Kind::label);
    assert_eq!(
        labels,
        [
            t::settings::ACTIVITY_KIND_REFUSED.get(),
            t::settings::ACTIVITY_KIND_FAILED.get(),
            t::settings::ACTIVITY_KIND_ACTED.get(),
            t::settings::ACTIVITY_KIND_SETTING.get(),
            t::settings::ACTIVITY_KIND_INBOX.get(),
            t::settings::ACTIVITY_KIND_ACCOUNT_SWITCH.get(),
        ]
    );
    assert_eq!(
        labels,
        [
            "Refused",
            "Failed",
            "Acted",
            "Setting",
            "Inbox",
            "Account Switch"
        ]
    );

    let mut model = ActivityModel {
        load: Load::Loading,
        ..ActivityModel::default()
    };
    assert_eq!(
        model.empty_text().as_deref(),
        Some(t::settings::activity_loading())
    );
    model.load = Load::Done;
    assert_eq!(
        model.empty_text().as_deref(),
        Some(t::settings::activity_empty())
    );
    model.entries = vec![entry("audit", None)];
    model.search = "zzz".into();
    assert_eq!(
        model.empty_text().as_deref(),
        Some(t::settings::activity_no_match())
    );
}

#[test]
fn a_rows_second_line_is_one_message_with_its_parts() {
    assert_eq!(entry("audit", None).line(), "Pixel 9 · ok");
    assert_eq!(
        entry("audit", Some("why")).line(),
        t::settings::activity_line_detail("Pixel 9", "ok", "why")
    );
    assert_eq!(entry("audit", Some("why")).line(), "Pixel 9 · ok · why");
    assert_eq!(entry("phone", None).line(), "Pixel 9 (phone) · ok");
}

#[test]
fn the_times_come_from_messages_and_keep_their_english() {
    let now = 1_700_000_000_000;
    assert_eq!(time_label(now, now, 0), "10:13 PM");
    assert_eq!(time_label(now, now - 86_400_000, 0), "Nov 13, 10:13 PM");
    assert_eq!(
        time_label(now, now - 400 * 86_400_000, 0),
        "Oct 10, 2022, 10:13 PM"
    );
    assert_eq!(t::settings::ACTIVITY_MONTH_DEC.english(), "Dec");
    assert_eq!(t::settings::activity_time_am("9", "05"), "9:05 AM");
    assert_eq!(t::settings::activity_chip("Refused", 3), "Refused 3");
    assert_eq!(
        t::settings::activity_showing(300, 412),
        "Showing the newest 300 of 412 rows"
    );
}
