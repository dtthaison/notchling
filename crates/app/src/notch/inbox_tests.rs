use super::*;

#[test]
fn the_badge_says_nothing_for_zero_and_caps_at_nine() {
    assert_eq!(badge_text(0), None);
    assert_eq!(badge_text(1).as_deref(), Some("1"));
    assert_eq!(badge_text(9).as_deref(), Some("9"));
    assert_eq!(badge_text(10).as_deref(), Some("9+"));
    assert_eq!(badge_text(400).as_deref(), Some("9+"));
}

#[test]
fn the_notch_draws_again_only_when_the_unread_count_changes() {
    let mut model = NotchModel::default();
    let before = model.drawn();
    assert!(!model.set_inbox_unread(0));
    assert_eq!(model.drawn(), before);

    assert!(model.set_inbox_unread(2));
    assert_ne!(model.drawn(), before);

    let two = model.drawn();
    assert!(!model.set_inbox_unread(2));
    assert_eq!(model.drawn(), two);

    assert!(model.set_inbox_unread(0));
    assert_eq!(model.drawn(), before);
}
