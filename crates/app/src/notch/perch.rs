use super::layout::Rect;
use super::pet::Point;

pub const LOWEST: f64 = 190.0;
pub const BODY_HEIGHT: f64 = 37.5;
pub const LEG_HANG: f64 = 40.0 / 240.0 * BODY_HEIGHT;
const EDGE_INSET: f64 = 14.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AxFrame {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Perch {
    pub x: f64,
    pub y: f64,
    pub w: f64,
}

impl Perch {
    pub fn mid_x(&self) -> f64 {
        self.x + self.w / 2.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PetMode {
    Hidden,
    Walking,
    Idle,
}

pub fn mode(show_pet: bool, working: usize, needs: usize) -> PetMode {
    match (show_pet, working + needs) {
        (false, _) => PetMode::Hidden,
        (true, 0) => PetMode::Idle,
        (true, _) => PetMode::Walking,
    }
}

pub fn find(
    window: AxFrame,
    full_screen: bool,
    screen: Rect,
    center_x: f64,
    primary_top: f64,
) -> Option<Perch> {
    let screen_top = primary_top - screen.max_y();
    let (mid_x, top) = (window.x + window.w / 2.0, window.y + 1.0);
    let on_screen = mid_x >= screen.x
        && mid_x < screen.max_x()
        && top >= screen_top
        && top < screen_top + screen.h;
    if !on_screen || full_screen || (window.w >= screen.w && window.h >= screen.h) {
        return None;
    }
    let seat_x = (window.x + 92.0).min(window.x + window.w - 20.0);
    let perch = Perch {
        x: seat_x - 60.0 - center_x,
        y: window.y - screen_top,
        w: 120.0,
    };
    (perch.y <= LOWEST).then_some(perch)
}

pub fn edge(span_left: f64, span_right: f64, notch_h: f64) -> (f64, f64, f64) {
    let lo = EDGE_INSET - span_left;
    let hi = (span_right - EDGE_INSET).max(lo + 1.0);
    (lo, hi, notch_h)
}

pub fn seat(perch: Option<Perch>, notch_w: f64, edge: (f64, f64, f64)) -> Point {
    let (lo, hi, y) = edge;
    match perch {
        Some(p) => Point::new(p.mid_x().clamp(lo, hi), p.y + LEG_HANG),
        None => Point::new(notch_w / 2.0 + 30.0, y + LEG_HANG),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCREEN: Rect = Rect::new(0.0, 0.0, 1512.0, 982.0);

    fn window(x: f64, y: f64, w: f64, h: f64) -> AxFrame {
        AxFrame { x, y, w, h }
    }

    #[test]
    fn it_sits_just_right_of_the_traffic_lights() {
        let p = find(
            window(100.0, 40.0, 900.0, 600.0),
            false,
            SCREEN,
            756.0,
            982.0,
        )
        .unwrap();
        assert_eq!(
            p,
            Perch {
                x: 132.0 - 756.0,
                y: 40.0,
                w: 120.0
            }
        );
        assert_eq!(p.mid_x(), 192.0 - 756.0);
    }

    #[test]
    fn a_narrow_window_keeps_the_seat_inside() {
        let p = find(
            window(500.0, 30.0, 80.0, 300.0),
            false,
            SCREEN,
            756.0,
            982.0,
        )
        .unwrap();
        assert_eq!(p.x + 60.0 + 756.0, 560.0, "20 in from its right edge");
    }

    #[test]
    fn nothing_to_sit_on() {
        let w = window(100.0, 40.0, 900.0, 600.0);
        assert_eq!(find(w, true, SCREEN, 756.0, 982.0), None, "full screen");
        let filling = window(0.0, 0.0, 1512.0, 982.0);
        assert_eq!(
            find(filling, false, SCREEN, 756.0, 982.0),
            None,
            "as big as the screen"
        );
        let low = window(100.0, 191.0, 900.0, 600.0);
        assert_eq!(find(low, false, SCREEN, 756.0, 982.0), None, "too low");
        let at_lowest = window(100.0, 190.0, 900.0, 600.0);
        assert!(find(at_lowest, false, SCREEN, 756.0, 982.0).is_some());
        let elsewhere = window(2000.0, 40.0, 900.0, 600.0);
        assert_eq!(
            find(elsewhere, false, SCREEN, 756.0, 982.0),
            None,
            "another screen"
        );
    }

    #[test]
    fn a_second_screen_counts_from_its_own_top() {
        let second = Rect::new(1512.0, 0.0, 1920.0, 1082.0);
        let w = window(1600.0, -100.0 + 50.0, 800.0, 500.0);
        let p = find(w, false, second, 2472.0, 982.0).unwrap();
        assert_eq!(p.y, 50.0);
        assert_eq!(p.x, 1600.0 + 92.0 - 60.0 - 2472.0);
    }

    #[test]
    fn the_seat_is_on_the_perch_or_beside_the_camera() {
        let e = edge(756.0, 756.0, 32.0);
        assert_eq!(e, (14.0 - 756.0, 742.0, 32.0));
        let perch = Perch {
            x: -600.0,
            y: 40.0,
            w: 120.0,
        };
        assert_eq!(
            seat(Some(perch), 185.0, e),
            Point::new(-540.0, 40.0 + LEG_HANG)
        );
        let off = Perch {
            x: -900.0,
            y: 40.0,
            w: 120.0,
        };
        assert_eq!(seat(Some(off), 185.0, e).x, e.0, "kept inside the screen");
        assert_eq!(
            seat(None, 185.0, e),
            Point::new(92.5 + 30.0, 32.0 + LEG_HANG)
        );
        assert_eq!(LEG_HANG, 6.25);
        assert_eq!(edge(10.0, 5.0, 24.0).1, 14.0 - 10.0 + 1.0, "never inverted");
    }

    #[test]
    fn it_walks_while_anything_works_or_waits() {
        assert_eq!(mode(true, 1, 0), PetMode::Walking);
        assert_eq!(mode(true, 0, 2), PetMode::Walking);
        assert_eq!(mode(true, 0, 0), PetMode::Idle);
        assert_eq!(mode(false, 3, 0), PetMode::Hidden);
    }
}
