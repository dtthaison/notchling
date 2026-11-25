use super::exec::{execute_clearing, Clearing, Failure, Markers, NoQueueLog, Outcome, TRAIL_MAX};
use super::fake::FakePrompt;
use super::keys;
use super::plan;

fn typed(text: &str) -> Vec<String> {
    vec![
        keys::clear_input(),
        keys::bracketed(text),
        keys::ENTER.to_string(),
    ]
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

fn knew(text: &str) -> Clearing {
    Clearing {
        earlier: vec![text.to_string()],
        any: false,
    }
}

#[test]
fn a_message_whole_in_the_box_is_cleared_and_typed_once_without_any_earlier_try() {
    for text in ["/clear", "is it right", "first the build\nthen the tests"] {
        let mut pty = FakePrompt::idle().holding(text);
        let (out, trail) = send(&mut pty, text, &Clearing::default());
        assert_eq!(out, Ok(Outcome { taken: false }), "{trail:#?}");
        assert_eq!(pty.writes, typed(text));
        assert_eq!(pty.sent, [text]);
    }
}

#[test]
fn a_box_that_keeps_the_text_after_the_clear_keys_ends_not_cleared_whatever_is_known_of_earlier_tries(
) {
    for clearing in [Clearing::default(), knew("/clear")] {
        let mut pty = FakePrompt::idle().stuck().holding("/clear");
        let (out, trail) = send(&mut pty, "/clear", &clearing);
        assert_eq!(out, Err(Failure::NotCleared), "{trail:#?}");
        assert_eq!(
            pty.writes,
            [keys::clear_input()],
            "the clear keys, then stop"
        );
        assert!(pty.sent.is_empty());
        assert!(trail.len() <= TRAIL_MAX);
        assert!(
            trail.iter().any(|note| note.contains("ms await empty: ")),
            "it got as far as waiting for the box to empty: {trail:#?}"
        );
    }
}

#[test]
fn the_trail_says_the_clear_keys_did_not_take_when_the_box_keeps_its_text() {
    let mut pty = FakePrompt::idle().stuck().holding("/clear");
    let (out, trail) = send(&mut pty, "/clear", &knew("/clear"));
    assert_eq!(out, Err(Failure::NotCleared), "{trail:#?}");
    let last = trail.last().cloned().unwrap_or_default();
    assert!(
        last.contains("ms await empty: the clear keys did not take"),
        "the last note says why the run stopped: {trail:#?}"
    );
}
