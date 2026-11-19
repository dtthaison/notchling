use super::Guarded;
use notchling_engine::reply::exec::{execute, Markers, NoQueueLog, Outcome, Pty};
use notchling_engine::reply::fake::FakePrompt;
use notchling_engine::reply::plan;
use notchling_engine::terminal::input_box;
use std::sync::atomic::{AtomicBool, Ordering};

const HINT: &str = "Try \"refactor parser.rs\"";

#[test]
fn the_gate_hands_on_what_the_session_drew_so_a_hint_still_reads_as_one() {
    let guard = || true;
    let (refused, typed) = (AtomicBool::new(false), AtomicBool::new(false));
    let mut gated = Guarded {
        inner: Box::new(FakePrompt::idle().hinting(HINT).drawing_its_cursor()),
        guard: &guard,
        refused: &refused,
        typed: &typed,
        entered: None,
        checked: false,
    };
    let lines = gated.screen();
    let shades = gated.shades();
    assert_eq!(gated.cursor(), None);
    assert!(input_box::hint_shown(&lines, &shades));
    assert!(input_box::cursor_drawn(&lines, &shades));
}

#[test]
fn a_reply_through_the_gate_goes_in_over_a_hint_the_session_drew_itself() {
    let guard = || true;
    let (refused, typed, entered) = (
        AtomicBool::new(false),
        AtomicBool::new(false),
        AtomicBool::new(false),
    );
    let mut gated = Guarded {
        inner: Box::new(FakePrompt::idle().hinting(HINT).drawing_its_cursor()),
        guard: &guard,
        refused: &refused,
        typed: &typed,
        entered: Some(&entered),
        checked: false,
    };
    let out = execute(&plan::reply("hi"), &mut gated, &Markers, &mut NoQueueLog);
    assert_eq!(out, Ok(Outcome { taken: false }));
    assert!(typed.load(Ordering::SeqCst) && entered.load(Ordering::SeqCst));
    assert!(!refused.load(Ordering::SeqCst));
}
