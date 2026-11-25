use super::exec::{execute, wake_failure, Markers, NoQueueLog, Outcome};
use super::exec_tests::{lines, Fake};
use super::fake::RULE;
use super::keys;
use super::plan;

const ELSEWHERE: &[&str] = &[
    "Waking session 5de74ffd…",
    "Couldn't wake 5de74ffd — Can't open — this session is running in another terminal",
];

const OTHER: &[&str] = &[
    "Waking session 5de74ffd…",
    "Couldn't wake 5de74ffd — the job has no conversation left",
];

fn why(screen: &[&str]) -> String {
    let mut pty = Fake::new(screen);
    execute(
        &plan::reply_waking("hi"),
        &mut pty,
        &Markers,
        &mut NoQueueLog,
    )
    .expect_err("a session that can't wake takes nothing")
    .to_string()
}

#[test]
fn a_conversation_open_elsewhere_says_so() {
    assert_eq!(
        why(ELSEWHERE),
        "This conversation is open somewhere else (a terminal or Claude Desktop), so the session can't wake. Close it there, then send again."
    );
}

#[test]
fn any_other_reason_is_passed_on_as_claude_code_wrote_it() {
    assert_eq!(
        why(OTHER),
        "The session couldn't wake: the job has no conversation left"
    );
}

#[test]
fn a_line_without_the_dash_is_the_reason_whole() {
    assert_eq!(
        why(&["Couldn't wake 5de74ffd"]),
        "The session couldn't wake: Couldn't wake 5de74ffd"
    );
}

const SAID: &str =
    "Couldn't wake 5de74ffd — Can't open — this session is running in another terminal";

#[test]
fn a_live_idle_screen_that_mentions_the_phrase_in_its_scrollback_is_not_a_failed_wake() {
    let mut pty = Fake::new(&[
        "Couldn't wake 5de74ffd — the job has no conversation left",
        "",
        RULE,
        "❯ ",
        RULE,
        "  ? for shortcuts",
    ])
    .reacting(|keys, screen| {
        let at = screen.len() - 3;
        if let Some(text) = keys
            .strip_prefix(keys::PASTE_START)
            .and_then(|t| t.strip_suffix(keys::PASTE_END))
        {
            screen[at] = format!("❯ {text}");
        } else if keys == keys::ENTER {
            screen[at] = "❯ ".to_string();
        }
    });
    let out = execute(
        &plan::reply_waking("hi"),
        &mut pty,
        &Markers,
        &mut NoQueueLog,
    );
    assert_eq!(out, Ok(Outcome { taken: false }));
    assert!(pty.writes().iter().any(|keys| keys.contains("hi")));
}

#[test]
fn the_phrase_counts_only_as_the_last_row_of_a_screen_with_no_prompt_box() {
    assert_eq!(
        wake_failure(&lines(&["Waking session 5de74ffd…", SAID, "", ""])),
        Some("Can't open — this session is running in another terminal".to_string())
    );
    assert_eq!(
        wake_failure(&lines(&[SAID, RULE, "❯ ", RULE, "  ? for shortcuts"])),
        None
    );
    assert_eq!(
        wake_failure(&lines(&["  see: Couldn't wake 5de74ffd — x"])),
        None,
        "inside a sentence"
    );
}

const ELSEWHERE_MESSAGE: &str = "This conversation is open somewhere else (a terminal or Claude Desktop), so the session can't wake. Close it there, then send again.";

#[test]
fn a_starting_box_with_the_line_printed_below_it_is_a_failed_wake() {
    let screen = [
        "⏺ ok",
        RULE,
        "❯",
        RULE,
        "  Session is starting — showing its transcript until it appears. Ctrl+Z to detach",
        SAID,
    ];
    assert_eq!(why(&screen), ELSEWHERE_MESSAGE);
    assert_eq!(
        wake_failure(&lines(&screen)),
        Some("Can't open — this session is running in another terminal".to_string())
    );
}

#[test]
fn a_line_that_wraps_is_joined_into_the_reason() {
    let head = "Couldn't wake 5de74ffd — Can't open — ";
    let filler = "x".repeat(200 - head.chars().count());
    let first = format!("{head}{filler}");
    let second = "this session is running in another terminal";
    assert_eq!(
        wake_failure(&lines(&["Waking session 5de74ffd…", &first, second])),
        Some(format!("Can't open — {filler}{second}"))
    );
    assert_eq!(
        why(&["Waking session 5de74ffd…", &first, second]),
        ELSEWHERE_MESSAGE
    );
}

#[test]
fn a_row_after_a_line_that_did_not_wrap_is_not_part_of_the_reason() {
    let hint = "  Run claude --resume 5de74ffd in a terminal to see why";
    assert_eq!(
        wake_failure(&lines(&["Waking session 5de74ffd…", SAID, hint])),
        Some("Can't open — this session is running in another terminal".to_string())
    );
    assert_eq!(
        why(&["Waking session 5de74ffd…", SAID, hint]),
        ELSEWHERE_MESSAGE
    );
}

#[test]
fn only_the_last_three_rows_are_looked_at() {
    let hint = "  a hint";
    assert!(wake_failure(&lines(&[SAID, hint, hint])).is_some());
    assert_eq!(wake_failure(&lines(&[SAID, hint, hint, hint])), None);
}
