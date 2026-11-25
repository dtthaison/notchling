use crate::part::PartState;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Labels {
    pub on: bool,
}

impl Labels {
    pub fn toggle(&mut self) -> bool {
        self.on = !self.on;
        self.on
    }
}

pub fn button_state(on: bool) -> PartState {
    if on {
        PartState::Pressed
    } else {
        PartState::Rest
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Corner {
    #[default]
    TopLeading,
    Top,
    TopTrailing,
    Leading,
    Trailing,
    BottomLeading,
    Bottom,
    BottomTrailing,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Anchor {
    Start(f32),
    Center,
    End(f32),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BadgePlace {
    pub x: Anchor,
    pub y: Anchor,
}

pub const ALONG: f32 = 6.0;
pub const OUTSIDE: f32 = -8.0;
pub const INSIDE: f32 = 6.0;

pub const OUTLINE_WIDTH: f32 = 1.0;
pub const OUTLINE_RADIUS: f32 = 4.0;
pub const OUTLINE_ALPHA: f32 = 0.7;
pub const BADGE_HEIGHT: f32 = 16.0;
pub const BADGE_TEXT: f32 = 10.0;
pub const BADGE_PADDING: f32 = 5.0;

pub fn badge_place(corner: Corner, inside: bool) -> BadgePlace {
    use Corner::*;
    let x = match corner {
        TopLeading | Leading | BottomLeading => Anchor::Start(ALONG),
        Top | Bottom => Anchor::Center,
        TopTrailing | Trailing | BottomTrailing => Anchor::End(ALONG),
    };
    let across = if inside { INSIDE } else { OUTSIDE };
    let y = match corner {
        TopLeading | Top | TopTrailing => Anchor::Start(across),
        Leading | Trailing => Anchor::Center,
        BottomLeading | Bottom | BottomTrailing => Anchor::End(across),
    };
    BadgePlace { x, y }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_start_off_and_toggle() {
        let mut labels = Labels::default();
        assert!(!labels.on);
        assert!(labels.toggle());
        assert!(labels.on);
        assert!(!labels.toggle());
        assert!(!labels.on);
    }

    #[test]
    fn the_button_is_pressed_while_on() {
        assert_eq!(button_state(true), PartState::Pressed);
        assert_eq!(button_state(false), PartState::Rest);
    }

    #[test]
    fn a_badge_straddles_the_edge_six_in_from_the_corner() {
        let p = badge_place(Corner::TopLeading, false);
        assert_eq!(p.x, Anchor::Start(6.0));
        assert_eq!(p.y, Anchor::Start(-8.0));
        let p = badge_place(Corner::BottomTrailing, false);
        assert_eq!(p.x, Anchor::End(6.0));
        assert_eq!(p.y, Anchor::End(-8.0));
        assert_eq!(OUTSIDE, -BADGE_HEIGHT / 2.0);
    }

    #[test]
    fn edge_corners_centre_the_other_axis() {
        assert_eq!(
            badge_place(Corner::Top, false),
            BadgePlace {
                x: Anchor::Center,
                y: Anchor::Start(-8.0)
            }
        );
        assert_eq!(
            badge_place(Corner::Bottom, false),
            BadgePlace {
                x: Anchor::Center,
                y: Anchor::End(-8.0)
            }
        );
        assert_eq!(
            badge_place(Corner::Leading, false),
            BadgePlace {
                x: Anchor::Start(6.0),
                y: Anchor::Center
            }
        );
        assert_eq!(
            badge_place(Corner::Trailing, true),
            BadgePlace {
                x: Anchor::End(6.0),
                y: Anchor::Center
            }
        );
    }

    #[test]
    fn inside_keeps_the_badge_within_a_clipped_edge() {
        assert_eq!(badge_place(Corner::Top, true).y, Anchor::Start(6.0));
        assert_eq!(badge_place(Corner::BottomLeading, true).y, Anchor::End(6.0));
        assert_eq!(badge_place(Corner::TopTrailing, true).x, Anchor::End(6.0));
    }

    #[test]
    fn the_default_corner_is_the_top_left() {
        assert_eq!(Corner::default(), Corner::TopLeading);
    }
}
