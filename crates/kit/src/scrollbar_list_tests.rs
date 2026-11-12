//! The scroll bar's numbers for a GPUI list (`ui::scrollable_list`).

use super::*;

#[test]
fn a_following_list_is_scrolled_to_the_end() {
    // A list that follows its end reports an offset past the content (it counts every row), or
    // none at all: either way the thumb is at the bottom.
    assert_eq!(list_scrolled(true, 0.0, 800.0), 800.0);
    assert_eq!(list_scrolled(true, 5000.0, 800.0), 800.0);
    assert_eq!(list_scrolled(true, 123.0, 0.0), 0.0, "nothing to scroll");
    // At the end, the thumb sits at the bottom of the track.
    let (track, viewport, content) = (596.0, 600.0, 1400.0);
    let end = thumb(
        track,
        viewport,
        content,
        list_scrolled(true, 0.0, content - viewport),
    )
    .expect("the content overflows");
    assert!((end.end() - track).abs() < 0.001, "{end:?}");
}

#[test]
fn a_list_offset_is_clamped() {
    // Not following: the offset itself, kept inside the range the rows measured so far allow
    // (a row measured since can leave it past the end, an unmeasured one short of the start).
    assert_eq!(list_scrolled(false, 250.0, 800.0), 250.0);
    assert_eq!(list_scrolled(false, -30.0, 800.0), 0.0);
    assert_eq!(list_scrolled(false, 950.0, 800.0), 800.0);
    assert_eq!(list_scrolled(false, 10.0, 0.0), 0.0);
}
