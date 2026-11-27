use super::*;
use crate::menu::{Dots, TrayGlyph, TrayLook};

fn look(dots: Dots, pulse: bool, glyph: TrayGlyph) -> TrayLook {
    TrayLook {
        glyph,
        dots,
        pulse,
        count: 1,
    }
}

#[test]
fn idle_draws_nothing_because_the_template_image_stays() {
    assert!(frames_of(&look(Dots::Idle, false, TrayGlyph::Outline)).is_empty());
}

#[test]
fn a_steady_look_has_one_frame_and_a_pulse_has_eight() {
    assert_eq!(
        frames_of(&look(Dots::Working, false, TrayGlyph::Outline)).len(),
        1
    );
    assert_eq!(
        frames_of(&look(Dots::Waiting, false, TrayGlyph::Outline)).len(),
        1
    );
    assert_eq!(
        frames_of(&look(Dots::Blocked, false, TrayGlyph::Outline)).len(),
        1
    );
    assert_eq!(
        frames_of(&look(Dots::Blocked, true, TrayGlyph::Outline)).len(),
        8
    );
}

#[test]
fn the_three_states_use_their_colors_and_shapes() {
    let working = &frames_of(&look(Dots::Working, false, TrayGlyph::Outline))[0];
    assert_eq!(working.tint, Tint::Green);
    assert!(working.marks.iter().all(|m| matches!(m, Mark::Bead { .. })));
    let waiting = &frames_of(&look(Dots::Waiting, false, TrayGlyph::Outline))[0];
    assert_eq!(waiting.tint, Tint::Orange);
    assert!(waiting.marks.iter().all(|m| matches!(m, Mark::Ring { .. })));
    let blocked = &frames_of(&look(Dots::Blocked, false, TrayGlyph::Outline))[0];
    assert_eq!(blocked.tint, Tint::Red);
    assert!(blocked.marks.iter().all(|m| m.radius() == 2.1));
    assert!(blocked.marks.iter().all(|m| m.radius() > IDLE_RADIUS));
}

#[test]
fn the_dots_sit_where_the_glyph_has_them() {
    let blocked = &frames_of(&look(Dots::Blocked, false, TrayGlyph::Outline))[0];
    let centers: Vec<(f64, f64)> = blocked.marks.iter().map(Mark::center).collect();
    assert_eq!(centers, [(5.0, 12.0), (11.0, 8.0), (17.0, 4.0)]);
}

#[test]
fn the_filled_glyph_fills_the_pills() {
    assert!(frames_of(&look(Dots::Working, false, TrayGlyph::Filled))[0].filled);
    assert!(!frames_of(&look(Dots::Working, false, TrayGlyph::Outline))[0].filled);
}

#[test]
fn the_pulse_goes_from_full_to_0_35_and_back() {
    let frames = frames_of(&look(Dots::Blocked, true, TrayGlyph::Outline));
    let alphas: Vec<f64> = frames.iter().map(|f| f.alpha).collect();
    assert_eq!(alphas[0], 1.0);
    assert!((alphas[4] - 0.35).abs() < 1e-9);
    assert!(alphas.iter().all(|a| (0.35..=1.0).contains(a)));
    assert!(alphas[1] < alphas[0] && alphas[2] < alphas[1] && alphas[3] < alphas[2]);
    assert!(alphas[5] > alphas[4] && alphas[7] > alphas[6]);
}

#[test]
fn the_pulse_runs_ten_seconds_at_about_eight_frames_a_second_then_stops() {
    assert!((pulse_step_seconds() - 0.25).abs() < 1e-9);
    assert_eq!(pulse_steps(), 40);
    assert_eq!(pulse_frame_at(0), Some(0));
    assert_eq!(pulse_frame_at(9), Some(1));
    assert_eq!(pulse_frame_at(39), Some(7));
    assert_eq!(pulse_frame_at(40), None);
}

#[test]
fn the_pill_outlines_stay_inside_the_glyph() {
    for index in 0..3 {
        let points = pill_polygon(index, 8);
        assert_eq!(points.len(), 18);
        for (x, y) in points {
            assert!(
                (0.0..=WIDTH).contains(&x) && (0.0..=HEIGHT).contains(&y),
                "{x} {y}"
            );
        }
    }
}

#[test]
fn the_middle_pill_leans_the_way_the_template_does() {
    let points = pill_polygon(2, 8);
    let (top, bottom) = (points[4], points[13]);
    assert!(top.0 < bottom.0 && top.1 < bottom.1, "{top:?} {bottom:?}");
}
