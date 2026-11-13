use std::f64::consts::PI;

use super::layout::{Size, FLARE};

pub const RADIUS: f64 = 8.0;
pub const TOP: f64 = 10.0;
pub const ROPE: f64 = 21.0;
pub const SPEED: f64 = 36.0;
pub const SPEED_NEEDS: f64 = 60.0;
pub const REST: f64 = 0.7;
pub const MAX_STEP: f64 = 0.1;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

impl Point {
    pub const fn new(x: f64, y: f64) -> Point {
        Point { x, y }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Track {
    left: f64,
    right: f64,
    bottom: f64,
    out: f64,
    side: f64,
    arc: f64,
    floor: f64,
}

const CORNER: f64 = 10.0;

impl Track {
    pub fn new(shape: Size) -> Track {
        let left = FLARE;
        let right = shape.w - FLARE;
        let bottom = shape.h;
        let out = RADIUS + 2.0;
        Track {
            left,
            right,
            bottom,
            out,
            side: (bottom - CORNER - TOP).max(0.0),
            arc: PI / 2.0 * (CORNER + out),
            floor: (right - left - 2.0 * CORNER).max(0.0),
        }
    }

    pub fn length(&self) -> f64 {
        2.0 * self.side + 2.0 * self.arc + self.floor
    }

    pub fn point(&self, d: f64) -> Point {
        let r = CORNER + self.out;
        let (side, arc, floor) = (self.side, self.arc, self.floor);
        if d < side {
            Point::new(self.left - self.out, TOP + d)
        } else if d < side + arc {
            let a = PI - (d - side) / r;
            Point::new(
                self.left + CORNER + a.cos() * r,
                self.bottom - CORNER + a.sin() * r,
            )
        } else if d < side + arc + floor {
            Point::new(
                self.left + CORNER + (d - side - arc),
                self.bottom + self.out,
            )
        } else if d < side + 2.0 * arc + floor {
            let a = PI / 2.0 - (d - side - arc - floor) / r;
            Point::new(
                self.right - CORNER + a.cos() * r,
                self.bottom - CORNER + a.sin() * r,
            )
        } else {
            Point::new(
                self.right + self.out,
                self.bottom - CORNER - (d - side - 2.0 * arc - floor),
            )
        }
    }

    pub fn outward(&self, d: f64) -> Point {
        let (a, b) = (self.point(d - 1.0), self.point(d + 1.0));
        let (tx, ty) = (b.x - a.x, b.y - a.y);
        let len = (tx * tx + ty * ty).sqrt().max(0.001);
        Point::new(-ty / len, tx / len)
    }

    fn bounds(&self) -> (f64, f64) {
        (ROPE, (self.length() - ROPE).max(ROPE))
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Turn {
    pub progress: f64,
    pub from: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Motion {
    pub d: f64,
    pub heading: f64,
    pub turn: Option<Turn>,
}

impl Default for Motion {
    fn default() -> Self {
        Motion {
            d: ROPE,
            heading: 1.0,
            turn: None,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Walk {
    pub motion: Motion,
    last: Option<f64>,
}

impl Walk {
    pub fn step(&mut self, t: f64, rolling: bool, needs: bool, shape: Size) -> Motion {
        let track = Track::new(shape);
        let (lo, hi) = track.bounds();
        let mut m = self.motion;
        m.d = m.d.clamp(lo, hi);
        let last = self.last;
        self.last = rolling.then_some(t);
        if let (true, Some(last)) = (rolling, last) {
            let dt = (t - last).clamp(0.0, MAX_STEP);
            advance(&mut m, dt, if needs { SPEED_NEEDS } else { SPEED }, lo, hi);
        }
        self.motion = m;
        m
    }
}

fn advance(m: &mut Motion, dt: f64, speed: f64, lo: f64, hi: f64) {
    if let Some(turn) = m.turn {
        let progress = turn.progress + dt / REST;
        m.turn = (progress < 1.0).then_some(Turn {
            progress,
            from: turn.from,
        });
        return;
    }
    m.d += m.heading * speed * dt;
    if m.d >= hi {
        m.d = hi;
        m.heading = -1.0;
        m.turn = Some(Turn {
            progress: 0.0,
            from: 1.0,
        });
    } else if m.d <= lo {
        m.d = lo;
        m.heading = 1.0;
        m.turn = Some(Turn {
            progress: 0.0,
            from: -1.0,
        });
    }
}

pub fn spin(m: &Motion) -> f64 {
    m.d / RADIUS
}

pub fn badge(m: &Motion, shape: Size) -> Point {
    let track = Track::new(shape);
    let Some(turn) = m.turn else {
        return track.point(m.d - m.heading * ROPE);
    };
    let pet = track.point(m.d);
    let from = track.point(m.d - turn.from * ROPE);
    let to = track.point(m.d - m.heading * ROPE);
    let n = track.outward(m.d);
    let eased = (1.0 - (turn.progress * PI).cos()) / 2.0;

    let start = (from.y - pet.y).atan2(from.x - pet.x);
    let end = (to.y - pet.y).atan2(to.x - pet.x);
    let mut sweep = (end - start).rem_euclid(2.0 * PI);
    let outward = n.y.atan2(n.x);
    let off = |a: f64| (a - outward).sin().atan2((a - outward).cos()).abs();
    if off(start + sweep / 2.0) > off(start + (sweep - 2.0 * PI) / 2.0) {
        sweep -= 2.0 * PI;
    }
    let angle = start + eased * sweep;
    let r_from = (from.x - pet.x).hypot(from.y - pet.y);
    let r_to = (to.x - pet.x).hypot(to.y - pet.y);
    let radius = r_from + eased * (r_to - r_from);
    Point::new(pet.x + angle.cos() * radius, pet.y + angle.sin() * radius)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SHAPE: Size = Size::new(201.0, 32.0);

    fn close(a: Point, b: Point) -> bool {
        (a.x - b.x).abs() < 1e-6 && (a.y - b.y).abs() < 1e-6
    }

    #[test]
    fn the_path_is_the_outline_pushed_out() {
        let t = Track::new(SHAPE);
        let arc = PI / 2.0 * 20.0;
        assert!((t.length() - (2.0 * 12.0 + 2.0 * arc + 165.0)).abs() < 1e-9);
        assert!(close(t.point(0.0), Point::new(8.0 - 10.0, TOP)));
        let mid = t.point(12.0 + arc + 82.5);
        assert!(close(mid, Point::new(8.0 + 10.0 + 82.5, 42.0)));
        assert!((mid.x - SHAPE.w / 2.0).abs() < 1e-9, "the floor's middle");
        let end = t.point(t.length());
        assert!(close(end, Point::new(SHAPE.w - 8.0 + 10.0, TOP)));
    }

    #[test]
    fn the_path_follows_the_shape_not_the_clock() {
        let small = Track::new(SHAPE);
        let wide = Track::new(Size::new(301.0, 32.0));
        assert!((wide.length() - small.length() - 100.0).abs() < 1e-9);
        let (mut a, mut b) = (Walk::default(), Walk::default());
        for t in [10.0, 10.05, 10.1, 10.15] {
            a.step(t, true, false, SHAPE);
            b.step(t, true, false, Size::new(301.0, 32.0));
        }
        assert_eq!(a.motion.d, b.motion.d);
        let d = small.length() - 5.0;
        assert!(!close(small.point(d), wide.point(d)));
    }

    #[test]
    fn outward_points_away_from_the_camera() {
        let t = Track::new(SHAPE);
        let left = t.outward(5.0);
        assert!(left.x < -0.99, "{left:?}");
        let floor = t.outward(100.0);
        assert!(floor.y > 0.99, "{floor:?}");
        let right = t.outward(t.length() - 5.0);
        assert!(right.x > 0.99, "{right:?}");
    }

    #[test]
    fn it_only_moves_while_rolling_and_never_leaps() {
        let mut walk = Walk::default();
        let start = walk.step(0.0, true, false, SHAPE);
        assert_eq!(start.d, ROPE, "the first frame only notes the time");
        let next = walk.step(0.05, true, false, SHAPE);
        assert!((next.d - (ROPE + SPEED * 0.05)).abs() < 1e-9);
        let stalled = walk.step(5.0, true, false, SHAPE);
        assert!((stalled.d - next.d - SPEED * MAX_STEP).abs() < 1e-9);
        let held = walk.step(9.0, false, false, SHAPE);
        assert_eq!(held.d, stalled.d);
        let again = walk.step(20.0, true, false, SHAPE);
        assert_eq!(again.d, stalled.d);
        let on = walk.step(20.05, true, true, SHAPE);
        assert!(
            (on.d - again.d - SPEED_NEEDS * 0.05).abs() < 1e-9,
            "faster when needed"
        );
    }

    #[test]
    fn it_rests_and_turns_round_at_each_end() {
        let mut walk = Walk::default();
        let hi = Track::new(SHAPE).length() - ROPE;
        let mut t = 0.0;
        walk.step(t, true, false, SHAPE);
        while walk.motion.turn.is_none() {
            t += 0.05;
            walk.step(t, true, false, SHAPE);
        }
        let m = walk.motion;
        assert_eq!((m.d, m.heading), (hi, -1.0));
        assert_eq!(m.turn.map(|tn| tn.from), Some(1.0));
        for _ in 0..13 {
            t += 0.05;
            let held = walk.step(t, true, false, SHAPE);
            assert_eq!(held.d, hi);
        }
        t += 0.1;
        walk.step(t, true, false, SHAPE);
        assert!(walk.motion.turn.is_none());
        t += 0.05;
        assert!(walk.step(t, true, false, SHAPE).d < hi, "heading back");
    }

    #[test]
    fn a_shape_that_shrinks_keeps_it_on_the_path() {
        let mut walk = Walk::default();
        walk.motion.d = 1000.0;
        let m = walk.step(0.0, false, false, SHAPE);
        assert_eq!(m.d, Track::new(SHAPE).length() - ROPE);
    }

    #[test]
    fn the_badge_trails_a_rope_behind_and_swings_round_the_outside() {
        let t = Track::new(SHAPE);
        let m = Motion {
            d: 100.0,
            heading: 1.0,
            turn: None,
        };
        assert!(close(badge(&m, SHAPE), t.point(100.0 - ROPE)));
        let back = Motion { heading: -1.0, ..m };
        assert!(close(badge(&back, SHAPE), t.point(100.0 + ROPE)));
        let turning = Motion {
            d: 100.0,
            heading: -1.0,
            turn: Some(Turn {
                progress: 0.5,
                from: 1.0,
            }),
        };
        let pet = t.point(100.0);
        let mid = badge(&turning, SHAPE);
        assert!(mid.y > pet.y + 10.0, "{mid:?} vs {pet:?}");
        let start = Motion {
            turn: Some(Turn {
                progress: 0.0,
                from: 1.0,
            }),
            ..turning
        };
        assert!(close(badge(&start, SHAPE), t.point(100.0 - ROPE)));
        let end = Motion {
            turn: Some(Turn {
                progress: 1.0,
                from: 1.0,
            }),
            ..turning
        };
        let b = badge(&end, SHAPE);
        let want = t.point(100.0 + ROPE);
        assert!((b.x - want.x).abs() < 1e-6 && (b.y - want.y).abs() < 1e-6);
    }

    #[test]
    fn reach_stays_within_the_canvas_margin() {
        use super::super::layout::PET_REACH;
        let (badge_w, badge_h) = (30.0, 16.0);
        let inside = |p: Point, half_w: f64, half_h: f64| {
            p.x - half_w >= -PET_REACH
                && p.x + half_w <= SHAPE.w + PET_REACH
                && p.y + half_h <= SHAPE.h + PET_REACH
                && p.y - half_h >= 0.0
        };
        let track = Track::new(SHAPE);
        let (lo, hi) = track.bounds();
        let mut d = lo;
        while d <= hi {
            for heading in [1.0, -1.0] {
                let rolling = Motion {
                    d,
                    heading,
                    turn: None,
                };
                assert!(inside(track.point(d), RADIUS, RADIUS), "pet at {d}");
                assert!(
                    inside(badge(&rolling, SHAPE), badge_w / 2.0, badge_h / 2.0),
                    "badge at {d} heading {heading}"
                );
                for step in 0..=10 {
                    let turning = Motion {
                        turn: Some(Turn {
                            progress: f64::from(step) / 10.0,
                            from: -heading,
                        }),
                        ..rolling
                    };
                    assert!(
                        inside(badge(&turning, SHAPE), badge_w / 2.0, badge_h / 2.0),
                        "badge turning at {d}, step {step}: {:?}",
                        badge(&turning, SHAPE)
                    );
                }
            }
            d += 0.5;
        }
    }

    #[test]
    fn it_spins_like_a_wheel() {
        let m = Motion {
            d: 16.0,
            ..Motion::default()
        };
        assert_eq!(spin(&m), 2.0);
    }
}
