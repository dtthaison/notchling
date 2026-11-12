//! The trajectory judge over made-up frames.

use super::*;

/// A frame at `at_ms` after `injected` points of wheel, with rows `a`, `b`, `c` at `ys` (`None`: not
/// painted).
fn frame(at_ms: f64, injected: f64, ys: [Option<f32>; 3]) -> Frame {
    let keys = ["a", "b", "c"];
    Frame {
        at_ms,
        injected,
        rows: ys
            .iter()
            .zip(keys)
            .filter_map(|(y, k)| y.map(|y| (k.to_owned(), y)))
            .collect(),
        ..Frame::default()
    }
}

fn at(y: f32) -> [Option<f32>; 3] {
    [Some(y), Some(y + 100.0), Some(y + 200.0)]
}

#[test]
fn a_move_by_what_the_wheel_asked_is_fine() {
    let (a, b) = (frame(0.0, 0.0, at(0.0)), frame(16.0, 12.0, at(12.0)));
    assert_eq!(judge(&a, &b), None);
    // Sub-point rounding is not a jump.
    let c = frame(32.0, 24.0, at(24.8));
    assert_eq!(judge(&b, &c), None);
}

#[test]
fn a_wheel_step_that_moved_nothing_is_a_stall() {
    let (a, b) = (frame(0.0, 0.0, at(0.0)), frame(16.0, 5.0, at(0.0)));
    assert_eq!(judge(&a, &b), Some(Event::Stall { asked: 5.0 }));
}

#[test]
fn a_move_by_another_amount_is_a_jump_and_the_other_way_a_reversal() {
    let a = frame(0.0, 0.0, at(0.0));
    let b = frame(16.0, 10.0, at(250.0));
    assert_eq!(
        judge(&a, &b),
        Some(Event::Jump {
            asked: 10.0,
            moved: 250.0
        })
    );
    let c = frame(16.0, 10.0, at(-40.0));
    assert_eq!(
        judge(&a, &c),
        Some(Event::Reversal {
            asked: 10.0,
            moved: -40.0
        })
    );
    // Less than asked (a list that can't go past the rows it has measured) is a jump too.
    let d = frame(16.0, 600.0, at(100.0));
    assert!(matches!(judge(&a, &d), Some(Event::Jump { .. })));
}

#[test]
fn frames_with_no_row_in_common_are_lost() {
    let a = frame(0.0, 0.0, [Some(0.0), None, None]);
    let b = frame(16.0, 900.0, [None, None, Some(30.0)]);
    assert_eq!(judge(&a, &b), Some(Event::Lost { asked: 900.0 }));
}

#[test]
fn content_that_moves_with_no_wheel_is_a_drift() {
    let (a, b) = (frame(0.0, 0.0, at(0.0)), frame(16.0, 0.0, at(30.0)));
    assert_eq!(judge(&a, &b), Some(Event::Drift { moved: 30.0 }));
    // New rows pushing a list that follows its end up are not.
    let (mut a, mut b) = (a, b);
    a.at_end = true;
    b.at_end = true;
    assert_eq!(judge(&a, &b), None);
}

#[test]
fn scrolling_down_at_the_end_has_nowhere_to_go() {
    let mut a = frame(0.0, 0.0, at(0.0));
    a.at_end = true;
    let b = frame(16.0, -20.0, at(0.0));
    assert_eq!(judge(&a, &b), None);
    // Down with less than asked left to the end: it stops there, which is no jump.
    let near = frame(0.0, 0.0, at(0.0));
    let mut end = frame(16.0, -50.0, at(-10.0));
    end.at_end = true;
    assert_eq!(judge(&near, &end), None);
    // Up from the end must move.
    let c = frame(16.0, 20.0, at(0.0));
    assert_eq!(judge(&a, &c), Some(Event::Stall { asked: 20.0 }));
}

#[test]
fn the_median_of_the_rows_is_how_far_the_content_moved() {
    let a = frame(0.0, 0.0, at(0.0));
    // One row grew under the others: the median still says 10, the spread says a row changed.
    let b = frame(16.0, 10.0, [Some(10.0), Some(110.0), Some(240.0)]);
    assert_eq!(moved(&a, &b), Some((10.0, 30.0)));
}

#[test]
fn a_report_counts_each_kind_and_keeps_the_first_few() {
    let mut frames = vec![frame(0.0, 0.0, at(0.0))];
    let mut y = 0.0;
    let mut wheel = 0.0;
    // Ten good steps, two stalls, one jump, then a step shown 60 ms late.
    for n in 1..=10 {
        wheel += 10.0;
        y += 10.0;
        frames.push(frame(f64::from(n) * 16.0, wheel, at(y)));
    }
    for n in 11..=12 {
        wheel += 10.0;
        frames.push(frame(f64::from(n) * 16.0, wheel, at(y)));
    }
    wheel += 10.0;
    y += 300.0;
    frames.push(frame(13.0 * 16.0, wheel, at(y)));
    wheel += 10.0;
    y += 10.0;
    frames.push(frame(13.0 * 16.0 + 60.0, wheel, at(y)));
    // Each step was made 1 ms before the draw that showed it, the last one 60 ms before.
    let mut steps: Vec<(f64, f64)> = frames
        .iter()
        .skip(1)
        .map(|f| (f.at_ms - 1.0, f.injected))
        .collect();
    if let Some(last) = steps.last_mut() {
        last.0 -= 59.0;
    }
    let r = report(&frames, &steps, 700.0);
    assert_eq!(r.frames, 15);
    assert_eq!((r.stalls, r.jumps, r.reversals, r.lost), (2, 1, 0, 0));
    assert_eq!(r.slow_frames, 1);
    assert_eq!(r.max_jump, 290.0);
    assert_eq!(r.first_events.len(), 3);
    assert_eq!(r.first_events[0].0, 11);
}

#[test]
fn a_step_waits_for_the_draw_that_shows_it() {
    let frames = vec![
        frame(0.0, 0.0, at(0.0)),
        frame(20.0, 10.0, at(10.0)),
        frame(36.0, 20.0, at(20.0)),
    ];
    // Injected at 5 ms (total 10): shown at 20 ms; at 25 ms (total 20): shown at 36 ms.
    let r = report(&frames, &[(5.0, 10.0), (25.0, 20.0)], 700.0);
    assert_eq!(r.latency_max_ms, 15.0);
    assert!(r.latency_p50_ms >= 11.0);
}

#[test]
fn the_json_names_what_a_test_asserts_on() {
    let r = report(&[], &[], 700.0);
    let v = r.json();
    for key in [
        "stalls",
        "jumps",
        "reversals",
        "lost",
        "drifts",
        "max_jump",
        "slow_frames",
        "latency_p99_ms",
    ] {
        assert!(v.get(key).is_some(), "{key}");
    }
}

fn draw(cpu_ms: f64, rows: u64, changes: u64) -> Draw {
    Draw {
        cpu_ms,
        wall_ms: cpu_ms * 2.0,
        rows,
        changes,
        ..Draw::default()
    }
}

#[test]
fn draws_are_counted_against_the_frame_budget() {
    let mut draws: Vec<Draw> = (0..96).map(|_| draw(3.0, 0, 0)).collect();
    draws.extend([draw(9.0, 1, 0), draw(20.0, 4, 0), draw(40.0, 2, 1)]);
    let r = draw_report(&draws);
    assert_eq!(r.draws, 99);
    assert_eq!((r.over_half, r.over_budget, r.over_two), (3, 2, 1));
    assert_eq!(r.cpu_max_ms, 40.0);
    assert_eq!(r.worst.len(), 6);
    assert_eq!(r.worst[0].cpu_ms, 40.0);
    assert_eq!(r.worst[1].cpu_ms, 20.0);
}

#[test]
fn a_dear_draw_names_what_it_built() {
    assert_eq!(draw(5.0, 3, 0).cause(), "none");
    assert_eq!(draw(30.0, 4, 0).cause(), "4 rows built");
    assert_eq!(draw(30.0, 4, 1).cause(), "list changed (4 rows built)");
    assert_eq!(draw(30.0, 0, 0).cause(), "rows already laid out");
}

#[test]
fn no_draws_report_nothing() {
    let r = draw_report(&[]);
    assert_eq!(r, DrawReport::default());
    assert!(r.json().get("over_budget").is_some());
}

#[test]
fn up_at_the_top_has_nowhere_to_go() {
    let at_top = |f: Frame| Frame { at_top: true, ..f };
    // Asked 30 up; the list is at its top and stays: not a stall.
    let a = at_top(frame(0.0, 0.0, at(0.0)));
    let b = at_top(frame(16.0, 30.0, at(0.0)));
    assert_eq!(judge(&a, &b), None);
    // Reaching the top with less to go than asked is fine too.
    let c = frame(0.0, 0.0, at(0.0));
    let d = at_top(frame(16.0, 30.0, at(10.0)));
    assert_eq!(judge(&c, &d), None);
    // Away from the top, no move is a stall.
    let e = frame(32.0, 60.0, at(10.0));
    let f = frame(48.0, 90.0, at(10.0));
    assert_eq!(judge(&e, &f), Some(Event::Stall { asked: 30.0 }));
}

#[test]
fn a_step_of_a_viewport_or_more_is_not_a_lost_frame() {
    // Two flings: 900 points with a 700 point list (no row can be in both), and 300 that lost its rows.
    let frames = [
        frame(0.0, 0.0, at(0.0)),
        frame(16.0, 900.0, [None, None, None]),
        frame(32.0, 1200.0, [None, None, None]),
    ];
    let r = report(&frames, &[], 700.0);
    assert_eq!((r.lost, r.max_jump), (1, 700.0));
}

#[test]
fn a_big_step_may_be_off_by_a_share_of_itself() {
    let (a, b) = (frame(0.0, 0.0, at(0.0)), frame(16.0, 620.0, at(612.0)));
    assert_eq!(judge(&a, &b), None);
    let c = frame(16.0, 620.0, at(560.0));
    assert!(matches!(judge(&a, &c), Some(Event::Jump { .. })));
}
