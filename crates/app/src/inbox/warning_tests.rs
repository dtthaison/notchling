use super::*;

#[test]
fn a_warn_with_notifications_denied_and_the_notch_hidden_shows_the_row() {
    assert!(next(false, true, true, false));
}

#[test]
fn the_row_stays_until_something_can_alert_again() {
    assert!(next(true, false, true, false));
}

#[test]
fn the_row_goes_when_notifications_are_allowed() {
    assert!(!next(true, false, false, false));
    assert!(!next(false, true, false, false));
}

#[test]
fn the_row_goes_when_the_notch_is_shown() {
    assert!(!next(true, false, true, true));
    assert!(!next(false, true, true, true));
}

#[test]
fn no_warn_and_nothing_showing_keeps_it_hidden() {
    assert!(!next(false, false, true, false));
}
