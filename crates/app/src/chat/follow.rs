//! Whether the conversation follows its newest message (the Swift app's `atBottom`): pinned to
//! the bottom while you are there, left alone once you scroll up, with a "↓ Latest" pill to get
//! back. Pure, so the rule is tested on every system; `ChatView` feeds it the scroll handle's
//! numbers before a reload is applied.

/// How far from the bottom still counts as at the bottom, in points.
pub const TOLERANCE: f32 = 24.0;

/// A scroll position as GPUI keeps it: `offset_y` is 0 at the top and goes negative as you scroll
/// down; `max_y` is how far the content can scroll (0 when it fits).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Scroll {
    pub offset_y: f32,
    pub max_y: f32,
}

impl Scroll {
    /// How far the bottom of the content is below the bottom of the view.
    pub fn from_bottom(self) -> f32 {
        (self.max_y + self.offset_y).max(0.0)
    }

    pub fn at_bottom(self) -> bool {
        self.from_bottom() <= TOLERANCE
    }
}

/// What to do with a reload.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Follow {
    /// Scroll to the end.
    pub scroll: bool,
    /// Show the "Latest" pill.
    pub pill: bool,
}

/// `prev` is the position before the reload, `new_max_y` the content's reach after it (`prev`'s
/// when not known yet). Sending a message and picking another session always go to the end; else
/// the end follows only if you were at the bottom, and a pill shows while you are not.
pub fn decide(prev: Scroll, new_max_y: f32, sent: bool, session_changed: bool) -> Follow {
    if sent || session_changed || prev.at_bottom() {
        return Follow {
            scroll: true,
            pill: false,
        };
    }
    let after = Scroll {
        max_y: new_max_y,
        ..prev
    };
    Follow {
        scroll: false,
        pill: !after.at_bottom(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(offset_y: f32, max_y: f32) -> Scroll {
        Scroll { offset_y, max_y }
    }

    #[test]
    fn content_that_fits_or_sits_at_the_bottom_follows() {
        assert_eq!(
            decide(at(0.0, 0.0), 300.0, false, false),
            Follow {
                scroll: true,
                pill: false
            }
        );
        // Exactly at the bottom, and within the tolerance of it.
        assert!(decide(at(-500.0, 500.0), 620.0, false, false).scroll);
        assert!(decide(at(-480.0, 500.0), 620.0, false, false).scroll);
        assert!(decide(at(-476.0, 500.0), 620.0, false, false).scroll);
    }

    #[test]
    fn scrolled_up_is_left_alone_with_a_pill() {
        let f = decide(at(-300.0, 500.0), 620.0, false, false);
        assert_eq!(
            f,
            Follow {
                scroll: false,
                pill: true
            }
        );
        // Just past the tolerance.
        assert!(!decide(at(-474.0, 500.0), 500.0, false, false).scroll);
    }

    #[test]
    fn sending_and_a_new_session_always_go_to_the_end() {
        let up = at(-100.0, 500.0);
        assert_eq!(
            decide(up, 500.0, true, false),
            Follow {
                scroll: true,
                pill: false
            }
        );
        assert!(decide(up, 500.0, false, true).scroll);
    }

    #[test]
    fn a_reload_that_shrinks_the_content_can_bring_you_back_to_the_bottom() {
        let f = decide(at(-300.0, 500.0), 310.0, false, false);
        assert_eq!(
            f,
            Follow {
                scroll: false,
                pill: false
            }
        );
    }

    #[test]
    fn the_pill_is_for_what_is_off_screen_below() {
        assert!(at(0.0, 0.0).at_bottom());
        assert!(at(-476.0, 500.0).at_bottom());
        assert!(!at(-475.0, 500.0).at_bottom());
    }
}
