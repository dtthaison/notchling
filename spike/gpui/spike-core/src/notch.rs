//! Notch geometry, from `NotchController.screensChanged` and `shapeRect`.
//! Coordinates are AppKit's: y grows upward from the bottom of the screen.

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScreenInfo {
    pub frame: Rect,
    pub safe_top: f64,
    pub aux_left_width: Option<f64>,
    pub aux_right_width: Option<f64>,
}

/// The camera housing: frame width minus the two menu bar areas beside it,
/// at the top of the frame. `None` on a display without a notch.
pub fn notch_rect(s: &ScreenInfo) -> Option<Rect> {
    let (left, right) = (s.aux_left_width?, s.aux_right_width?);
    let w = s.frame.w - left - right;
    if w <= 0. || s.safe_top <= 0. {
        return None;
    }
    Some(Rect {
        x: s.frame.x + left,
        y: s.frame.y + s.frame.h - s.safe_top,
        w,
        h: s.safe_top,
    })
}

/// Half-open on the right and top edges, so a boundary pixel belongs to one side
/// only; the shape grows downward by `slack_below`.
pub fn hit_test(shape: Rect, mouse: (f64, f64), slack_below: f64) -> bool {
    let (mx, my) = mouse;
    mx >= shape.x && mx < shape.x + shape.w && my >= shape.y - slack_below && my < shape.y + shape.h
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notch_rect_on_macbook() {
        let s = ScreenInfo {
            frame: Rect { x: 0., y: 0., w: 1512., h: 982. },
            safe_top: 32.,
            aux_left_width: Some(662.),
            aux_right_width: Some(662.),
        };
        assert_eq!(notch_rect(&s), Some(Rect { x: 662., y: 950., w: 188., h: 32. }));
    }

    #[test]
    fn notch_rect_none_without_notch() {
        let s = ScreenInfo {
            frame: Rect { x: 0., y: 0., w: 2560., h: 1440. },
            safe_top: 0.,
            aux_left_width: None,
            aux_right_width: None,
        };
        assert_eq!(notch_rect(&s), None);
    }

    #[test]
    fn hit_test_edges() {
        let shape = Rect { x: 100., y: 900., w: 200., h: 32. };
        assert!(hit_test(shape, (100., 900.), 0.)); // corner inside
        assert!(!hit_test(shape, (300., 916.), 0.)); // right edge is outside (half-open)
        assert!(hit_test(shape, (150., 897.), 4.)); // inside the 4 pt slack below
        assert!(!hit_test(shape, (150., 895.), 4.));
    }
}
