//! The overlay scroll bar's numbers (the drawing is `ui::scrollbar`): where the thumb sits, how far a
//! drag or a press in the track scrolls, and how visible the bar is a moment after the last scroll.
//! Pure, in points, tested on every system.
//!
//! Positions are "scrolled": how far the content has moved up, 0 at the top, never negative, unlike
//! GPUI's scroll offset, which goes negative as you scroll down.

/// The shortest the thumb gets, however long the content is.
pub const MIN_THUMB: f32 = 24.0;
/// The thumb's width at rest, and under the pointer.
pub const THUMB_WIDTH: f32 = 7.0;
pub const THUMB_WIDTH_HOVER: f32 = 9.0;
/// The thumb's gap to the container's right edge, and the track's to its top and bottom.
pub const EDGE_INSET: f32 = 2.0;
/// The right padding a scrolling list keeps so its rows end before the bar's lane (the inset, the
/// widest thumb and a hair), and a row's fill or border never draws under the bar.
pub const GUTTER: f32 = EDGE_INSET + THUMB_WIDTH_HOVER + 2.0;
/// The strip along the right edge that counts as the bar: showing it, grabbing it.
pub const ZONE_WIDTH: f32 = 14.0;
/// The bar stays fully shown this long after the last scroll, then fades over `FADE`.
pub const HOLD_SECONDS: f32 = 0.7;
pub const FADE_SECONDS: f32 = 0.3;
/// A press in the track scrolls a page, less this much, so the last lines stay in view.
const PAGE_OVERLAP: f32 = 40.0;

/// The thumb on the track: `start` from the track's top, `len` along it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Thumb {
    pub start: f32,
    pub len: f32,
}

impl Thumb {
    pub fn end(self) -> f32 {
        self.start + self.len
    }
}

/// How far the content can scroll, 0 when it fits (a hair of rounding doesn't count).
pub fn max_scrolled(viewport: f32, content: f32) -> f32 {
    let max = content - viewport;
    if max < 0.5 {
        0.0
    } else {
        max
    }
}

/// The thumb for a track of `track` points, a viewport of `viewport` showing `content`, scrolled
/// `scrolled` down. `None` when the content fits (the bar is hidden).
pub fn thumb(track: f32, viewport: f32, content: f32, scrolled: f32) -> Option<Thumb> {
    let max = max_scrolled(viewport, content);
    if max <= 0.0 || track <= 0.0 || viewport <= 0.0 {
        return None;
    }
    let len = (track * viewport / content).clamp(MIN_THUMB.min(track), track);
    let along = (scrolled.clamp(0.0, max) / max) * (track - len);
    Some(Thumb { start: along, len })
}

/// The scroll position after the thumb is dragged `delta` points from where it was grabbed, when
/// the content was `start_scrolled` down. The thumb's travel (`track - len`) covers the whole
/// scroll range, so a drag moves the content faster than the pointer.
pub fn scrolled_for_drag(
    track: f32,
    viewport: f32,
    content: f32,
    start_scrolled: f32,
    delta: f32,
) -> f32 {
    let max = max_scrolled(viewport, content);
    let Some(thumb) = thumb(track, viewport, content, start_scrolled) else {
        return 0.0;
    };
    let travel = track - thumb.len;
    if travel <= 0.0 {
        return start_scrolled.clamp(0.0, max);
    }
    (start_scrolled + delta * max / travel).clamp(0.0, max)
}

/// The scroll position after a press in the track at `y` (from the track's top): a page toward it,
/// or `None` when the press is on the thumb.
pub fn scrolled_for_page(
    track: f32,
    viewport: f32,
    content: f32,
    scrolled: f32,
    y: f32,
) -> Option<f32> {
    let max = max_scrolled(viewport, content);
    let thumb = thumb(track, viewport, content, scrolled)?;
    let page = (viewport - PAGE_OVERLAP).max(viewport / 2.0);
    if y < thumb.start {
        Some((scrolled - page).clamp(0.0, max))
    } else if y > thumb.end() {
        Some((scrolled + page).clamp(0.0, max))
    } else {
        None
    }
}

/// How shown the bar is, 0 to 1, `since` seconds after the last scroll: fully for `HOLD_SECONDS`,
/// then fading to nothing over `FADE_SECONDS`.
pub fn visibility(since: f32) -> f32 {
    let total = HOLD_SECONDS + FADE_SECONDS;
    if since <= HOLD_SECONDS {
        1.0
    } else if since >= total {
        0.0
    } else {
        1.0 - (since - HOLD_SECONDS) / FADE_SECONDS
    }
}

/// Whether the bar is still changing (so the next frame is worth drawing) `since` seconds after the
/// last scroll: during the hold it only waits for the fade to start.
pub fn is_animating(since: f32) -> bool {
    since < HOLD_SECONDS + FADE_SECONDS
}

/// How often the fading bar is drawn again: 30 times a second, not every display frame, since a
/// frame draws the whole list the bar is over again.
pub const FADE_STEP_SECONDS: f32 = 1.0 / 30.0;

/// When the bar next looks different, `since` seconds after the last scroll: when the fade starts,
/// while it holds (nothing changes until then), then a step of the fade; `None` once it's hidden.
pub fn next_frame_in(since: f32) -> Option<f32> {
    if !is_animating(since) {
        return None;
    }
    if since < HOLD_SECONDS {
        return Some(HOLD_SECONDS - since);
    }
    Some(
        FADE_STEP_SECONDS
            .min(HOLD_SECONDS + FADE_SECONDS - since)
            .max(0.001),
    )
}

/// Whether the pointer holds the bar on screen: only one scrolling already showed (`shown`), or
/// that it already holds, so it can be grabbed. Passing over a hidden bar shows nothing.
pub fn holds(over: bool, shown: bool, hovered: bool) -> bool {
    over && (shown || hovered)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_pointer_never_shows_a_hidden_bar() {
        // Over a bar nothing scrolled: it stays hidden.
        assert!(!holds(true, false, false));
        // Over a bar scrolling showed: it holds, so it can be grabbed, and keeps holding.
        assert!(holds(true, true, false));
        assert!(holds(true, false, true));
        // Away from the bar: nothing to hold.
        assert!(!holds(false, true, true));
    }

    #[test]
    fn hidden_when_the_content_fits() {
        assert_eq!(thumb(300.0, 300.0, 300.0, 0.0), None);
        assert_eq!(thumb(300.0, 300.0, 200.0, 0.0), None);
        // Rounding noise from layout is not overflow.
        assert_eq!(thumb(300.0, 300.0, 300.3, 0.0), None);
        assert_eq!(max_scrolled(300.0, 300.4), 0.0);
        assert_eq!(max_scrolled(300.0, 450.0), 150.0);
    }

    #[test]
    fn thumb_length_follows_the_visible_share() {
        // Half the content visible: half the track.
        let t = thumb(200.0, 300.0, 600.0, 0.0);
        assert_eq!(
            t,
            Some(Thumb {
                start: 0.0,
                len: 100.0
            })
        );
        // A quarter visible.
        let t = thumb(400.0, 100.0, 400.0, 0.0);
        assert_eq!(
            t,
            Some(Thumb {
                start: 0.0,
                len: 100.0
            })
        );
    }

    #[test]
    fn thumb_is_never_shorter_than_the_minimum() {
        let t = thumb(300.0, 300.0, 300_000.0, 0.0);
        assert_eq!(t.map(|t| t.len), Some(MIN_THUMB));
        // And never longer than a track shorter than it.
        let t = thumb(10.0, 300.0, 300_000.0, 0.0);
        assert_eq!(t.map(|t| t.len), Some(10.0));
    }

    #[test]
    fn thumb_travels_the_track_with_the_scroll() {
        let (track, viewport, content) = (200.0, 300.0, 600.0);
        let top = thumb(track, viewport, content, 0.0);
        let middle = thumb(track, viewport, content, 150.0);
        let bottom = thumb(track, viewport, content, 300.0);
        assert_eq!(top.map(|t| t.start), Some(0.0));
        assert_eq!(middle.map(|t| t.start), Some(50.0));
        assert_eq!(bottom.map(|t| t.end()), Some(track));
        // Overscroll (a bounce) and a stale offset stay on the track.
        assert_eq!(thumb(track, viewport, content, -20.0), top);
        assert_eq!(thumb(track, viewport, content, 999.0), bottom);
    }

    #[test]
    fn dragging_the_thumb_scrolls_the_whole_range() {
        let (track, viewport, content) = (200.0, 300.0, 600.0);
        // Travel is 100 pt for 300 pt of scroll: 3 to 1.
        assert_eq!(scrolled_for_drag(track, viewport, content, 0.0, 10.0), 30.0);
        assert_eq!(
            scrolled_for_drag(track, viewport, content, 60.0, -10.0),
            30.0
        );
        // To the ends, and no further.
        assert_eq!(
            scrolled_for_drag(track, viewport, content, 0.0, 500.0),
            300.0
        );
        assert_eq!(
            scrolled_for_drag(track, viewport, content, 100.0, -500.0),
            0.0
        );
        // A drag of nothing keeps the place.
        assert_eq!(
            scrolled_for_drag(track, viewport, content, 120.0, 0.0),
            120.0
        );
    }

    #[test]
    fn dragging_when_nothing_scrolls_stays_at_the_top() {
        assert_eq!(scrolled_for_drag(200.0, 300.0, 300.0, 0.0, 40.0), 0.0);
    }

    #[test]
    fn dragging_the_whole_travel_reaches_the_end() {
        let (track, viewport, content) = (180.0, 240.0, 5000.0);
        let t = thumb(track, viewport, content, 0.0).unwrap_or(Thumb {
            start: 0.0,
            len: 0.0,
        });
        let end = scrolled_for_drag(track, viewport, content, 0.0, track - t.len);
        assert!((end - max_scrolled(viewport, content)).abs() < 0.01);
    }

    #[test]
    fn a_press_in_the_track_pages_toward_it() {
        let (track, viewport, content) = (200.0, 300.0, 1200.0);
        // Thumb 50 long at the top: a press below it pages down.
        assert_eq!(
            scrolled_for_page(track, viewport, content, 0.0, 150.0),
            Some(260.0)
        );
        // Mid-way, above the thumb pages up.
        let scrolled = 450.0;
        let t = thumb(track, viewport, content, scrolled).map(|t| t.start);
        assert_eq!(t, Some(75.0));
        assert_eq!(
            scrolled_for_page(track, viewport, content, scrolled, 10.0),
            Some(190.0)
        );
        // On the thumb: not a page.
        assert_eq!(
            scrolled_for_page(track, viewport, content, scrolled, 100.0),
            None
        );
    }

    #[test]
    fn paging_stops_at_the_ends() {
        let (track, viewport, content) = (200.0, 300.0, 400.0);
        assert_eq!(
            scrolled_for_page(track, viewport, content, 0.0, 199.0),
            Some(100.0)
        );
        assert_eq!(
            scrolled_for_page(track, viewport, content, 100.0, 0.0),
            Some(0.0)
        );
        assert_eq!(scrolled_for_page(track, 300.0, 300.0, 0.0, 10.0), None);
    }

    #[test]
    fn visible_while_scrolling_then_fades_out_in_about_a_second() {
        assert_eq!(visibility(0.0), 1.0);
        assert_eq!(visibility(HOLD_SECONDS), 1.0);
        let half = visibility(HOLD_SECONDS + FADE_SECONDS / 2.0);
        assert!((half - 0.5).abs() < 1e-5);
        assert_eq!(visibility(HOLD_SECONDS + FADE_SECONDS), 0.0);
        assert_eq!(visibility(5.0), 0.0);
        // "About a second" after scrolling stops.
        let total = HOLD_SECONDS + FADE_SECONDS;
        assert!((total - 1.0).abs() < 0.05, "{total}");
    }

    #[test]
    fn a_held_bar_waits_for_its_fade_and_the_fade_steps_thirty_times_a_second() {
        assert!((next_frame_in(0.0).unwrap() - HOLD_SECONDS).abs() < 1e-6);
        assert!((next_frame_in(0.5).unwrap() - (HOLD_SECONDS - 0.5)).abs() < 1e-6);
        assert_eq!(next_frame_in(HOLD_SECONDS), Some(FADE_STEP_SECONDS));
        let near_end = HOLD_SECONDS + FADE_SECONDS - 0.01;
        assert!((next_frame_in(near_end).unwrap() - 0.01).abs() < 1e-4);
        assert_eq!(next_frame_in(HOLD_SECONDS + FADE_SECONDS), None);
        assert_eq!(next_frame_in(5.0), None);
    }

    #[test]
    fn animates_only_until_hidden() {
        assert!(is_animating(0.0));
        assert!(is_animating(0.9));
        assert!(!is_animating(HOLD_SECONDS + FADE_SECONDS));
    }
}
