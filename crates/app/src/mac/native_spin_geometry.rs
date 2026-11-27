use gpui::{Bounds, Pixels};
use notchling_kit::ui::SpinSlot;
use notchling_kit::Color;
use objc2_foundation::{NSPoint, NSRect, NSSize};

pub const BOX: f64 = 24.0;
pub const RADIUS: f64 = 9.0;
pub const STROKE: f64 = 3.0;
const SWEEP: f64 = 288.0 * std::f64::consts::PI / 180.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Placed {
    pub ring: Rect,
    pub clip: Rect,
    pub ink: Color,
}

fn rect_of(b: Bounds<Pixels>) -> Rect {
    Rect {
        x: f32::from(b.origin.x),
        y: f32::from(b.origin.y),
        w: f32::from(b.size.width),
        h: f32::from(b.size.height),
    }
}

pub fn placed(slot: &SpinSlot) -> Option<Placed> {
    let shown = slot.bounds.intersect(&slot.mask);
    let (clip, ring) = (rect_of(shown), rect_of(slot.bounds));
    (clip.w > 0.0 && clip.h > 0.0 && ring.w > 0.0).then_some(Placed {
        ring,
        clip,
        ink: slot.color,
    })
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Frame {
    pub height: f32,
    pub scale: f32,
    pub flipped: bool,
}

pub fn layer_y(y: f32, h: f32, frame: Frame) -> f64 {
    if frame.flipped {
        f64::from(y)
    } else {
        f64::from(frame.height - y - h)
    }
}

pub fn container_frame(clip: Rect, frame: Frame) -> NSRect {
    NSRect::new(
        NSPoint::new(f64::from(clip.x), layer_y(clip.y, clip.h, frame)),
        NSSize::new(f64::from(clip.w), f64::from(clip.h)),
    )
}

pub fn ring_centre(p: &Placed, frame: Frame) -> NSPoint {
    let (cx, cy) = (
        p.ring.x + p.ring.w / 2.0 - p.clip.x,
        p.ring.y + p.ring.h / 2.0 - p.clip.y,
    );
    let y = if frame.flipped { cy } else { p.clip.h - cy };
    NSPoint::new(f64::from(cx), f64::from(y))
}

pub fn arc(flipped: bool) -> (f64, f64, bool) {
    if flipped {
        (0.0, SWEEP, false)
    } else {
        (0.0, -SWEEP, true)
    }
}

pub fn turn_to(flipped: bool) -> f64 {
    if flipped {
        std::f64::consts::TAU
    } else {
        -std::f64::consts::TAU
    }
}

#[cfg(test)]
mod tests {
    use gpui::{point, px, size, Bounds};
    use notchling_kit::{Appearance, Ink};

    use super::*;

    fn slot(x: f32, y: f32, mask: Bounds<gpui::Pixels>) -> SpinSlot {
        SpinSlot {
            key: 1,
            bounds: Bounds::new(point(px(x), px(y)), size(px(12.0), px(12.0))),
            mask,
            color: Ink::Working.color(Appearance::Dark),
        }
    }

    fn window_mask() -> Bounds<gpui::Pixels> {
        Bounds::new(point(px(0.0), px(0.0)), size(px(800.0), px(600.0)))
    }

    const FRAME: Frame = Frame {
        height: 600.0,
        scale: 2.0,
        flipped: false,
    };

    #[test]
    fn a_slot_inside_its_clip_is_placed_whole() {
        let p = placed(&slot(100.0, 40.0, window_mask())).expect("shown");
        assert_eq!(p.clip, p.ring);
        assert_eq!(
            p.ring,
            Rect {
                x: 100.0,
                y: 40.0,
                w: 12.0,
                h: 12.0
            }
        );
    }

    #[test]
    fn a_slot_half_under_a_header_is_clipped_to_what_shows() {
        let mask = Bounds::new(point(px(0.0), px(44.0)), size(px(800.0), px(556.0)));
        let p = placed(&slot(100.0, 38.0, mask)).expect("half shows");
        assert_eq!(
            p.clip,
            Rect {
                x: 100.0,
                y: 44.0,
                w: 12.0,
                h: 6.0
            }
        );
        assert_eq!(
            p.ring.y, 38.0,
            "the ring keeps its place; only the container is cut"
        );
    }

    #[test]
    fn a_slot_wholly_outside_its_clip_has_no_layer() {
        let mask = Bounds::new(point(px(0.0), px(44.0)), size(px(800.0), px(556.0)));
        assert_eq!(placed(&slot(100.0, 20.0, mask)), None);
        assert_eq!(placed(&slot(100.0, 700.0, mask)), None);
        assert_eq!(placed(&slot(900.0, 100.0, mask)), None, "right of the clip");
    }

    #[test]
    fn window_y_down_becomes_layer_y_up() {
        assert_eq!(
            layer_y(0.0, 12.0, FRAME),
            588.0,
            "the top edge is the top of the layer"
        );
        assert_eq!(layer_y(588.0, 12.0, FRAME), 0.0);
        let flipped = Frame {
            flipped: true,
            ..FRAME
        };
        assert_eq!(layer_y(40.0, 12.0, flipped), 40.0);
    }

    #[test]
    fn the_container_is_the_clip_and_the_ring_sits_in_its_middle() {
        let mask = Bounds::new(point(px(0.0), px(44.0)), size(px(800.0), px(556.0)));
        let p = placed(&slot(100.0, 38.0, mask)).expect("shown");
        let c = container_frame(p.clip, FRAME);
        assert_eq!(
            (c.origin.x, c.origin.y),
            (100.0, 550.0),
            "6 tall, its top at 44"
        );
        assert_eq!((c.size.width, c.size.height), (12.0, 6.0));
        let centre = ring_centre(&p, FRAME);
        assert_eq!((centre.x, centre.y), (6.0, 6.0));
        let window_y =
            f64::from(FRAME.height) - c.origin.y - c.size.height + (c.size.height - centre.y);
        assert_eq!(window_y, 44.0);
    }

    #[test]
    fn the_same_place_at_one_and_two_x_is_the_same_layer_geometry() {
        let p = placed(&slot(10.0, 20.0, window_mask())).expect("shown");
        let one = container_frame(
            p.clip,
            Frame {
                scale: 1.0,
                ..FRAME
            },
        );
        let two = container_frame(p.clip, FRAME);
        assert_eq!((one.origin.x, one.origin.y), (two.origin.x, two.origin.y));
    }

    #[test]
    fn the_arc_and_the_turn_go_clockwise_whichever_way_y_runs() {
        let (s, e, cw) = arc(false);
        assert!(
            s == 0.0 && e < 0.0 && cw,
            "y up: clockwise is a decreasing angle"
        );
        assert!(turn_to(false) < 0.0);
        let (s, e, cw) = arc(true);
        assert!(
            s == 0.0 && e > 0.0 && !cw,
            "y down: clockwise is an increasing angle"
        );
        assert!(turn_to(true) > 0.0);
        assert!((arc(false).1.abs() - 288f64.to_radians()).abs() < 1e-9);
    }
}
