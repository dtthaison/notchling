use super::exec::{
    execute_clearing, Clearing, Failure, Markers, NoQueueLog, Outcome, Pty, TRAIL_MAX,
};
use super::fake::FakePrompt;
use super::keys::{self, timing::*};
use super::plan;

const EARLIER: &str = "Please run the whole suite again and tell me what failed";
const WITH_SHOT: &str = "i want click refresh it will refresh is it ok?\n\n'/Users/me/Library/Application Support/Notchling/drops/1794723941465/Screenshot 2026-11-15 at 1.25.35 PM.png'";
const CHIPPED: &str = "[Image #47]i want click refresh it will refresh is it ok?";

fn typed(text: &str) -> Vec<String> {
    vec![
        keys::clear_input(),
        keys::bracketed(text),
        keys::ENTER.to_string(),
    ]
}

fn earlier() -> Clearing {
    Clearing {
        earlier: vec![EARLIER.to_string()],
        any: false,
    }
}

fn asked() -> Clearing {
    Clearing {
        earlier: Vec::new(),
        any: true,
    }
}

fn send(
    pty: &mut FakePrompt,
    text: &str,
    clearing: &Clearing,
) -> (Result<Outcome, Failure>, Vec<String>) {
    let mut trail = Vec::new();
    let out = execute_clearing(
        &plan::reply(text),
        pty,
        &Markers,
        &mut NoQueueLog,
        &mut trail,
        clearing,
    );
    (out, trail)
}

#[test]
fn what_an_earlier_try_left_in_the_box_is_cleared_and_the_new_message_sent_once() {
    let mut pty = FakePrompt::idle().holding(EARLIER);
    let (out, trail) = send(&mut pty, "now fix it", &earlier());
    assert_eq!(out, Ok(Outcome { taken: false }), "{trail:#?}");
    assert_eq!(pty.sent, ["now fix it"]);
    assert_eq!(
        pty.writes,
        typed("now fix it"),
        "one clear, one paste, one Enter"
    );
}

#[test]
fn a_start_or_an_end_of_an_earlier_message_is_ours_too() {
    for left in [
        "Please run the whole suite",
        "and tell me what failed",
        "Please  run the whole\nsuite again!",
    ] {
        let mut pty = FakePrompt::idle().holding(left);
        let (out, trail) = send(&mut pty, "now fix it", &earlier());
        assert_eq!(out, Ok(Outcome { taken: false }), "{left}: {trail:#?}");
        assert_eq!(pty.sent, ["now fix it"], "{left}");
    }
}

#[test]
fn words_that_only_look_like_a_bit_of_an_earlier_message_are_not_ours() {
    for left in [
        "Please",
        "Please run",
        "what",
        "and tell me what failed today",
        "a draft of mine",
    ] {
        let mut pty = FakePrompt::idle().holding(left);
        let (out, _) = send(&mut pty, "now fix it", &earlier());
        assert_eq!(out, Err(Failure::NotCleared), "{left}");
        assert!(
            pty.writes.is_empty(),
            "{left}: not a key, not even the clear"
        );
        assert_eq!(pty.held(), Some(left));
    }
}

#[test]
fn a_draft_nobody_sent_is_refused_with_nothing_typed_and_stays_in_the_box() {
    let mut pty = FakePrompt::idle().holding("my own draft, half written");
    let (out, _) = send(&mut pty, "now fix it", &Clearing::default());
    assert_eq!(out, Err(Failure::NotCleared));
    assert!(pty.writes.is_empty());
    assert!(pty.sent.is_empty());
    assert_eq!(pty.held(), Some("my own draft, half written"));
}

#[test]
fn a_draft_in_the_box_is_cleared_when_the_person_asked_for_it() {
    let mut pty = FakePrompt::idle().holding("my own draft, half written\nover two lines");
    let (out, trail) = send(&mut pty, "now fix it", &asked());
    assert_eq!(out, Ok(Outcome { taken: false }), "{trail:#?}");
    assert_eq!(pty.sent, ["now fix it"]);
    assert_eq!(pty.writes, typed("now fix it"));
    assert_eq!(pty.held(), Some(""));
}

#[test]
fn asking_to_clear_an_empty_box_just_sends() {
    let mut pty = FakePrompt::idle();
    let (out, _) = send(&mut pty, "now fix it", &asked());
    assert_eq!(out, Ok(Outcome { taken: false }));
    assert_eq!(pty.writes, typed("now fix it"));
}

#[test]
fn a_clear_that_does_not_take_sends_nothing_and_never_presses_enter() {
    let mut pty = FakePrompt::idle().stuck().holding("my own draft");
    let (out, _) = send(&mut pty, "now fix it", &asked());
    assert_eq!(out, Err(Failure::NotCleared));
    assert_eq!(
        pty.writes,
        [keys::clear_input()],
        "the clear keys, then stop"
    );
    assert!(pty.sent.is_empty());
    assert_eq!(pty.held(), Some("my own draft"));
    assert!(
        pty.now_ms() >= CLEAR_WAIT_MS,
        "it waited for the box to empty"
    );

    let mut pty = FakePrompt::idle().stuck().holding(EARLIER);
    let (out, _) = send(&mut pty, "now fix it", &earlier());
    assert_eq!(out, Err(Failure::NotCleared));
    assert_eq!(pty.writes, [keys::clear_input()]);
    assert!(pty.sent.is_empty());
}

#[test]
fn a_person_typing_while_the_box_is_cleared_stops_the_message_before_the_paste() {
    let mut pty = FakePrompt::idle()
        .holding("my own draft")
        .typing(ATTACH_QUIET_MS + 50, "x");
    let (out, _) = send(&mut pty, "now fix it", &asked());
    assert_eq!(out, Err(Failure::NotCleared));
    assert_eq!(pty.writes, [keys::clear_input()], "no paste, no Enter");
    assert!(pty.sent.is_empty());
}

#[test]
fn a_folded_paste_is_never_taken_for_ours_but_is_cleared_when_asked() {
    let long = "word ".repeat(300);
    let theirs = Clearing {
        earlier: vec![long.clone()],
        any: false,
    };
    let mut pty = FakePrompt::idle().holding(&long).folding(100);
    let (out, _) = send(&mut pty, "now fix it", &theirs);
    assert_eq!(out, Err(Failure::NotCleared));
    assert!(pty.writes.is_empty());

    let mut pty = FakePrompt::idle().holding(&long).folding(100);
    let (out, trail) = send(&mut pty, "now fix it", &asked());
    assert_eq!(out, Ok(Outcome { taken: false }), "{trail:#?}");
    assert_eq!(pty.sent, ["now fix it"]);
}

#[test]
fn the_trail_says_a_clear_was_made_and_why_but_never_what_the_box_held() {
    let mut pty = FakePrompt::idle().holding(EARLIER);
    let (_, trail) = send(&mut pty, "now fix it", &earlier());
    assert!(
        trail.iter().any(|l| l.contains("an earlier try of ours")),
        "{trail:#?}"
    );
    let mut pty = FakePrompt::idle().holding("secret draft words");
    let (_, trail) = send(&mut pty, "now fix it", &asked());
    assert!(trail.iter().any(|l| l.contains("as asked")), "{trail:#?}");
    for line in &trail {
        assert!(
            !line.contains("secret") && !line.contains("fix it") && !line.contains("suite"),
            "{line}"
        );
    }
    assert!(trail.len() <= TRAIL_MAX);
}

#[test]
fn a_box_that_holds_exactly_the_message_is_still_cleared_without_any_asking() {
    let mut pty = FakePrompt::idle().holding("now fix it");
    let (out, _) = send(&mut pty, "now fix it", &Clearing::default());
    assert_eq!(out, Ok(Outcome { taken: false }));
    assert_eq!(pty.sent, ["now fix it"]);
    assert_eq!(pty.writes, typed("now fix it"));
}

#[test]
fn our_message_left_in_the_box_as_a_chip_and_words_is_cleared_and_sent_once() {
    let mut pty = FakePrompt::idle().reading_pictures().holding(CHIPPED);
    let (out, trail) = send(&mut pty, WITH_SHOT, &Clearing::default());
    assert_eq!(out, Ok(Outcome { taken: false }), "{trail:#?}");
    assert_eq!(
        pty.sent,
        ["[Image #1]i want click refresh it will refresh is it ok?"]
    );
    assert_eq!(
        pty.writes,
        typed(WITH_SHOT),
        "one clear, one paste, one Enter"
    );
}

#[test]
fn an_earlier_try_with_a_picture_is_ours_even_when_its_words_are_short() {
    let earlier = Clearing {
        earlier: vec!["ok\n\n'/tmp/shots/a b.png'".to_string()],
        any: false,
    };
    let mut pty = FakePrompt::idle().holding("[Image #47]ok");
    let (out, trail) = send(&mut pty, "now fix it", &earlier);
    assert_eq!(out, Ok(Outcome { taken: false }), "{trail:#?}");
    assert_eq!(pty.sent, ["now fix it"]);
    assert!(
        trail.iter().any(|l| l.contains("an earlier try of ours")),
        "{trail:#?}"
    );

    let mut pty = FakePrompt::idle().holding("[Image #47]ok");
    let (out, _) = send(&mut pty, "now fix it", &Clearing::default());
    assert_eq!(out, Err(Failure::NotCleared), "nothing says it is ours");
    assert!(pty.writes.is_empty());
}
