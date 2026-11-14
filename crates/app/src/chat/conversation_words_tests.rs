use notchling_engine::transcript::model::ToolKind;
use notchling_locale as t;

use super::message_meta::{full_time, relative_time};
use super::rows::{tool_icon, Output, ToolRow};
use super::steps::{detail, failed_text, output_text, summary, Detail};

const DAY: f64 = 86_400.0;
const NOW: f64 = 20_766.0 * DAY + 23.0 * 3600.0 + 10.0 * 60.0;

fn step(id: &str, kind: ToolKind, input: &str) -> ToolRow {
    ToolRow {
        id: id.into(),
        kind,
        icon: tool_icon(kind),
        title: "step".into(),
        input: input.into(),
        output: None,
        diff: Vec::new(),
        added: 0,
        removed: 0,
        is_error: false,
        finished: true,
    }
}

#[test]
fn message_times_are_messages() {
    assert_eq!(relative_time(NOW, NOW - 30.0, 0), t::units::just_now());
    assert_eq!(relative_time(NOW, NOW - 60.0, 0), t::units::minutes_ago(1));
    assert_eq!(
        relative_time(NOW, NOW - 5.0 * 60.0, 0),
        t::units::minutes_ago(5)
    );
    assert_eq!(relative_time(NOW, NOW - 3600.0, 0), t::units::hours_ago(1));
    assert_eq!(
        relative_time(NOW, NOW - 2.0 * 3600.0, 0),
        t::units::hours_ago(2)
    );
    assert_eq!(
        relative_time(NOW, NOW - DAY, 0),
        t::units::yesterday(&t::units::clock(11, "10", t::units::PM.get()))
    );
    assert_eq!(
        relative_time(NOW, NOW - 3.0 * DAY, 0),
        t::units::date_short(t::units::MONTH_NOV.get(), "6")
    );
    assert_eq!(
        relative_time(NOW, NOW - 400.0 * DAY, 0),
        t::units::date_year(t::units::MONTH_OCT.get(), "5", "2025")
    );
    assert_eq!(
        full_time(NOW, 0),
        t::units::date_full(
            t::units::WEEKDAY_LONG_MONDAY.get(),
            t::units::MONTH_LONG_NOV.get(),
            "9",
            "2026",
            "11:10 PM"
        )
    );
}

#[test]
fn message_times_read_as_they_did() {
    assert_eq!(relative_time(NOW, NOW - 30.0, 0), "just now");
    assert_eq!(relative_time(NOW, NOW - 60.0, 0), "1 minute ago");
    assert_eq!(relative_time(NOW, NOW - 5.0 * 60.0, 0), "5 minutes ago");
    assert_eq!(relative_time(NOW, NOW - 3600.0, 0), "1 hour ago");
    assert_eq!(relative_time(NOW, NOW - 2.0 * 3600.0, 0), "2 hours ago");
    assert_eq!(relative_time(NOW, NOW - DAY, 0), "Yesterday 11:10 PM");
    assert_eq!(relative_time(NOW, NOW - 3.0 * DAY, 0), "Nov 6");
    assert_eq!(relative_time(NOW, NOW - 400.0 * DAY, 0), "Oct 5, 2025");
    assert_eq!(full_time(NOW, 0), "Monday, November 9, 2026 at 11:10 PM");
    let midnight = 20_766.0 * DAY;
    assert_eq!(
        full_time(midnight, 0),
        "Monday, November 9, 2026 at 12:00 AM"
    );
    assert_eq!(
        full_time(midnight + 12.0 * 3600.0 + 5.0 * 60.0, 0),
        "Monday, November 9, 2026 at 12:05 PM"
    );
}

#[test]
fn every_month_and_weekday_reads_as_it_did() {
    let long: Vec<&str> = [
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
    ]
    .iter()
    .map(|m| m.get())
    .collect();
    assert_eq!(
        long,
        [
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
            "December"
        ]
    );
    let short: Vec<&str> = [
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
    ]
    .iter()
    .map(|m| m.get())
    .collect();
    assert_eq!(
        short,
        ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"]
    );
    let days: Vec<&str> = [
        t::units::WEEKDAY_LONG_SUNDAY,
        t::units::WEEKDAY_LONG_MONDAY,
        t::units::WEEKDAY_LONG_TUESDAY,
        t::units::WEEKDAY_LONG_WEDNESDAY,
        t::units::WEEKDAY_LONG_THURSDAY,
        t::units::WEEKDAY_LONG_FRIDAY,
        t::units::WEEKDAY_LONG_SATURDAY,
    ]
    .iter()
    .map(|d| d.get())
    .collect();
    assert_eq!(
        days,
        [
            "Sunday",
            "Monday",
            "Tuesday",
            "Wednesday",
            "Thursday",
            "Friday",
            "Saturday"
        ]
    );
}

#[test]
fn a_tool_summary_is_one_message_per_kind() {
    let kinds: [(ToolKind, fn(usize) -> String, [&str; 2]); 11] = [
        (
            ToolKind::Shell,
            t::chat::steps_ran_commands,
            ["Ran 1 command", "Ran 2 commands"],
        ),
        (
            ToolKind::Read,
            t::chat::steps_read_files,
            ["Read 1 file", "Read 2 files"],
        ),
        (
            ToolKind::Edit,
            t::chat::steps_edited_files,
            ["Edited 1 file", "Edited 2 files"],
        ),
        (
            ToolKind::Write,
            t::chat::steps_wrote_files,
            ["Wrote 1 file", "Wrote 2 files"],
        ),
        (
            ToolKind::Search,
            t::chat::steps_searched_times,
            ["Searched 1 time", "Searched 2 times"],
        ),
        (
            ToolKind::Web,
            t::chat::steps_browsed_pages,
            ["Browsed 1 page", "Browsed 2 pages"],
        ),
        (
            ToolKind::Agent,
            t::chat::steps_ran_agents,
            ["Ran 1 agent", "Ran 2 agents"],
        ),
        (
            ToolKind::Task,
            t::chat::steps_updated_tasks,
            ["Updated 1 task", "Updated 2 tasks"],
        ),
        (
            ToolKind::Ask,
            t::chat::steps_asked_questions,
            ["Asked 1 question", "Asked 2 questions"],
        ),
        (
            ToolKind::Skill,
            t::chat::steps_used_skills,
            ["Used 1 skill", "Used 2 skills"],
        ),
        (
            ToolKind::Other,
            t::chat::steps_used_tools,
            ["Used 1 tool", "Used 2 tools"],
        ),
    ];
    for (kind, message, english) in kinds {
        for (i, n) in [1usize, 2].into_iter().enumerate() {
            if kind == ToolKind::Other && n == 1 {
                continue;
            }
            let steps: Vec<ToolRow> = (0..n)
                .map(|k| step(&format!("s{k}"), kind, &format!("/x/{k}")))
                .collect();
            assert_eq!(
                summary(&steps),
                notchling_kit::sentence(&message(n)),
                "{kind:?} {n}"
            );
            assert_eq!(summary(&steps), english[i], "{kind:?} {n}");
        }
    }
    let mixed = [
        step("a", ToolKind::Shell, "ls"),
        step("b", ToolKind::Read, "/x/a.rs"),
    ];
    assert_eq!(summary(&mixed), "Ran 1 command, read 1 file");
}

#[test]
fn step_titles_and_cut_output_are_messages() {
    let shell = step("a", ToolKind::Shell, "ls");
    assert!(matches!(
        detail(&shell),
        Detail::Text {
            input_title: Some(title),
            ..
        } if title == t::chat::step_command()
    ));
    let other = step("b", ToolKind::Web, "https://x.test");
    assert!(matches!(
        detail(&other),
        Detail::Text {
            input_title: Some(title),
            ..
        } if title == t::chat::step_input()
    ));
    assert_eq!(t::chat::step_command(), "Command");
    assert_eq!(t::chat::step_input(), "Input");
    let mut cut = step("c", ToolKind::Shell, "ls");
    cut.output = Some(Output {
        text: "a\nb".into(),
        more: 1200,
    });
    assert_eq!(output_text(&cut).as_deref(), Some("a\nb\n… +1200 lines"));
    let mut failed = step("d", ToolKind::Shell, "ls");
    failed.is_error = true;
    assert_eq!(
        failed_text(&[failed.clone()]),
        Some(t::chat::steps_failed(1))
    );
    assert_eq!(
        failed_text(&[failed.clone(), failed]).as_deref(),
        Some("2 failed")
    );
    assert_eq!(failed_text(&[cut]), None);
}

#[test]
fn the_conversations_words_are_what_they_were() {
    assert_eq!(t::chat::thought_for("12s"), "Thought for 12s");
    assert_eq!(t::chat::thought_briefly(), "Thought briefly");
    assert_eq!(t::chat::worked_for("4m 05s"), "Worked for 4m 05s");
    assert_eq!(
        t::chat::worked_for_model("4m 05s", "Opus 5.5"),
        "Worked for 4m 05s · Opus 5.5"
    );
    assert_eq!(t::chat::images(1), "1 image");
    assert_eq!(t::chat::images(3), "3 images");
    assert_eq!(t::chat::no_reply_yet(), "· No reply yet");
    assert_eq!(t::chat::sending_note(), "Sending…");
    assert_eq!(t::chat::loading(), "Loading…");
    assert_eq!(t::chat::agy_loading(), "Loading from Antigravity…");
    assert_eq!(
        t::chat::agy_through_daemon(),
        "Antigravity sessions load through the daemon while acting is on."
    );
    assert_eq!(t::chat::reply_placeholder(), "Reply…");
    assert_eq!(t::chat::message_codex_placeholder(), "Message Codex…");
    assert_eq!(
        t::chat::message_antigravity_placeholder(),
        "Message Antigravity…"
    );
    assert_eq!(
        t::chat::codex_account_tip(),
        "The Codex account. Click to switch or add one."
    );
    assert_eq!(
        t::chat::menu_opens("/model"),
        "/model opens a menu: run it in this session's own terminal"
    );
    assert_eq!(t::chat::this_command(), "This command");
    assert_eq!(t::chat::account_lane("night"), "night · lane");
    assert_eq!(t::chat::run_tip(), "Runs in a terminal (it asks for input)");
    assert_eq!(t::chat::select_first(), "Select some text first.");
    assert_eq!(t::chat::code(), "code");
    assert_eq!(t::chat::activity_shells("2 shells"), "· 2 shells");
}
