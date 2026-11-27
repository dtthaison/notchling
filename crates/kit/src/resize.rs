pub const EDGE: f32 = 4.0;
pub const CORNER: f32 = 10.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Zone {
    Left,
    Right,
    Top,
    Bottom,
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Axis {
    Horizontal,
    Vertical,
    NorthWestSouthEast,
    NorthEastSouthWest,
}

impl Zone {
    pub const ALL: [Zone; 8] = [
        Zone::Left,
        Zone::Right,
        Zone::Top,
        Zone::Bottom,
        Zone::TopLeft,
        Zone::TopRight,
        Zone::BottomLeft,
        Zone::BottomRight,
    ];

    pub fn axis(self) -> Axis {
        match self {
            Zone::Left | Zone::Right => Axis::Horizontal,
            Zone::Top | Zone::Bottom => Axis::Vertical,
            Zone::TopLeft | Zone::BottomRight => Axis::NorthWestSouthEast,
            Zone::TopRight | Zone::BottomLeft => Axis::NorthEastSouthWest,
        }
    }

    fn on_top(self) -> bool {
        matches!(self, Zone::Top | Zone::TopLeft | Zone::TopRight)
    }

    pub fn frame(self, width: f32, height: f32, top_resizes: bool) -> Option<(f32, f32, f32, f32)> {
        if self.on_top() && !top_resizes {
            return None;
        }
        let y0 = if top_resizes { CORNER } else { 0.0 };
        let side = (height - y0 - CORNER).max(0.0);
        let run = (width - 2.0 * CORNER).max(0.0);
        Some(match self {
            Zone::Left => (0.0, y0, EDGE, side),
            Zone::Right => (width - EDGE, y0, EDGE, side),
            Zone::Top => (CORNER, 0.0, run, EDGE),
            Zone::Bottom => (CORNER, height - EDGE, run, EDGE),
            Zone::TopLeft => (0.0, 0.0, CORNER, CORNER),
            Zone::TopRight => (width - CORNER, 0.0, CORNER, CORNER),
            Zone::BottomLeft => (0.0, height - CORNER, CORNER, CORNER),
            Zone::BottomRight => (width - CORNER, height - CORNER, CORNER, CORNER),
        })
    }
}

pub fn zone_at(x: f32, y: f32, width: f32, height: f32, top_resizes: bool) -> Option<Zone> {
    Zone::ALL.into_iter().find(|zone| {
        zone.frame(width, height, top_resizes)
            .is_some_and(|(fx, fy, fw, fh)| x >= fx && x < fx + fw && y >= fy && y < fy + fh)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const W: f32 = 800.0;
    const H: f32 = 500.0;

    #[test]
    fn corners_and_edges_of_a_window_with_a_top_border() {
        let at = |x, y| zone_at(x, y, W, H, true);
        assert_eq!(at(0.0, 0.0), Some(Zone::TopLeft));
        assert_eq!(at(9.9, 9.9), Some(Zone::TopLeft));
        assert_eq!(at(W - 1.0, 1.0), Some(Zone::TopRight));
        assert_eq!(at(1.0, H - 1.0), Some(Zone::BottomLeft));
        assert_eq!(at(W - 1.0, H - 1.0), Some(Zone::BottomRight));
        assert_eq!(at(1.0, 200.0), Some(Zone::Left));
        assert_eq!(at(W - 1.0, 200.0), Some(Zone::Right));
        assert_eq!(at(400.0, 1.0), Some(Zone::Top));
        assert_eq!(at(400.0, H - 1.0), Some(Zone::Bottom));
    }

    #[test]
    fn the_inside_belongs_to_no_zone() {
        assert_eq!(zone_at(EDGE, 200.0, W, H, true), None);
        assert_eq!(zone_at(400.0, EDGE, W, H, true), None);
        assert_eq!(zone_at(W / 2.0, H / 2.0, W, H, true), None);
        assert_eq!(zone_at(CORNER, 1.0, W, H, true), Some(Zone::Top));
        assert_eq!(zone_at(1.0, CORNER, W, H, true), Some(Zone::Left));
        assert_eq!(zone_at(8.0, 8.0, W, H, true), Some(Zone::TopLeft));
    }

    #[test]
    fn outside_the_window_is_no_zone() {
        assert_eq!(zone_at(-1.0, 100.0, W, H, true), None);
        assert_eq!(zone_at(W, 100.0, W, H, true), None);
        assert_eq!(zone_at(100.0, H, W, H, true), None);
    }

    #[test]
    fn a_titled_window_has_no_top_zones_and_its_sides_run_to_the_top() {
        let at = |x, y| zone_at(x, y, W, H, false);
        assert_eq!(at(0.0, 0.0), Some(Zone::Left));
        assert_eq!(at(W - 1.0, 0.0), Some(Zone::Right));
        assert_eq!(at(400.0, 0.0), None);
        assert_eq!(at(1.0, H - 1.0), Some(Zone::BottomLeft));
        assert_eq!(at(400.0, H - 1.0), Some(Zone::Bottom));
        assert_eq!(Zone::Top.frame(W, H, false), None);
        assert_eq!(Zone::TopLeft.frame(W, H, false), None);
    }

    #[test]
    fn zones_never_overlap() {
        for top in [true, false] {
            for zone in Zone::ALL {
                let Some((x, y, w, h)) = zone.frame(W, H, top) else {
                    continue;
                };
                for (px, py) in [
                    (x, y),
                    (x + w - 0.5, y + h - 0.5),
                    (x + w / 2.0, y + h / 2.0),
                ] {
                    let claims: Vec<Zone> = Zone::ALL
                        .into_iter()
                        .filter(|z| {
                            z.frame(W, H, top).is_some_and(|(fx, fy, fw, fh)| {
                                px >= fx && px < fx + fw && py >= fy && py < fy + fh
                            })
                        })
                        .collect();
                    assert_eq!(claims, vec![zone], "{zone:?} at {px},{py} (top {top})");
                    assert_eq!(zone_at(px, py, W, H, top), Some(zone));
                }
            }
        }
    }

    #[test]
    fn a_tiny_window_keeps_frames_non_negative() {
        for zone in Zone::ALL {
            let (_, _, w, h) = zone.frame(12.0, 12.0, true).unwrap_or((0.0, 0.0, 0.0, 0.0));
            assert!(w >= 0.0 && h >= 0.0, "{zone:?}");
        }
    }

    #[test]
    fn axes_pair_the_opposite_zones() {
        assert_eq!(Zone::Left.axis(), Axis::Horizontal);
        assert_eq!(Zone::Right.axis(), Axis::Horizontal);
        assert_eq!(Zone::Top.axis(), Axis::Vertical);
        assert_eq!(Zone::Bottom.axis(), Axis::Vertical);
        assert_eq!(Zone::TopLeft.axis(), Axis::NorthWestSouthEast);
        assert_eq!(Zone::BottomRight.axis(), Axis::NorthWestSouthEast);
        assert_eq!(Zone::TopRight.axis(), Axis::NorthEastSouthWest);
        assert_eq!(Zone::BottomLeft.axis(), Axis::NorthEastSouthWest);
    }
}
