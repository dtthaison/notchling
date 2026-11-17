use super::*;

#[test]
fn a_card_not_drawn_keeps_no_clock() {
    let state = State::default();
    assert!(
        !state.clock_due(10 * MINUTE_MS),
        "nothing drawn, nothing due"
    );
    state.drawn_minute.set(Some(5));
    assert!(!state.clock_due(5 * MINUTE_MS + 59_999), "same minute");
    assert!(state.clock_due(6 * MINUTE_MS), "next minute redraws");
    state.not_drawn();
    assert!(
        !state.clock_due(60 * MINUTE_MS),
        "a draw without cards stops the clock"
    );
}
