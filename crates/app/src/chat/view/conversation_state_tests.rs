//! `ConversationState`'s own rules, with no window or `ChatView`.

use super::*;

#[test]
fn toggle_opens_then_closes() {
    let mut s = ConversationState::new();
    assert!(!s.is_open("g1"));
    s.toggle("g1");
    assert!(s.is_open("g1"));
    assert!(!s.is_open("g2"));
    s.toggle("g1");
    assert!(!s.is_open("g1"));
}

#[test]
fn expand_all_opens_everything() {
    let mut s = ConversationState::new();
    s.expand_all = true;
    assert!(s.is_open("anything"));
    // The Tasks bar's own key is the preview's to show, not expand-all's: it reads `expanded`.
    assert!(!s.expanded.contains("anything"));
}

#[test]
fn hover_asks_for_a_redraw_only_when_times_are_stale() {
    let mut s = ConversationState::new();
    s.drawn_ms = now_ms();
    assert!(!s.hover("m1", true), "drawn just now: the times are fresh");
    assert_eq!(s.hovered_message.as_deref(), Some("m1"));
    s.drawn_ms = 0;
    assert!(s.hover("m2", true), "drawn long ago: draw the times again");
    assert_eq!(s.hovered_message.as_deref(), Some("m2"));
    // Leaving another message than the hovered one changes nothing; leaving it clears.
    assert!(!s.hover("m1", false));
    assert_eq!(s.hovered_message.as_deref(), Some("m2"));
    assert!(!s.hover("m2", false));
    assert_eq!(s.hovered_message, None);
}

#[test]
fn the_pill_shows_when_scrolled_up_and_not_while_loading() {
    // A scroll handle nothing laid out reads as at the bottom: no pill.
    let mut s = ConversationState::new();
    assert!(!s.latest_pill(false));
    // The render preview forces it, but a conversation on its way in has none.
    s.latest_preview = true;
    assert!(s.latest_pill(false));
    assert!(!s.latest_pill(true));
    // The pill's click puts the preview away and follows the end again.
    s.jump_to_latest();
    assert!(!s.latest_preview);
    assert!(!s.latest_pill(false));
}
