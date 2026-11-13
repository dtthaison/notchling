use super::*;
use crate::testutil::Scratch;

fn widths(sidebar: f32, details: f32) -> Widths {
    Widths { sidebar, details }
}

#[test]
fn a_ratio_gives_the_width_back_inside_the_limits() {
    let r = ratio(300.0, 1500.0);
    assert!((r - 0.2).abs() < 1e-6);
    assert!((width_from(Side::Sidebar, r, 1500.0) - 300.0).abs() < 0.01);
    assert_eq!(width_from(Side::Sidebar, 0.9, 1500.0), SIDEBAR_MAX);
    assert_eq!(width_from(Side::Sidebar, 0.01, 1500.0), SIDEBAR_MIN);
    assert_eq!(width_from(Side::Details, 0.9, 1500.0), DETAILS_MAX);
    assert_eq!(width_from(Side::Details, 0.01, 1500.0), DETAILS_MIN);
    assert_eq!(width_from(Side::Details, f32::NAN, 1500.0), DETAILS_DEFAULT);
    assert_eq!(width_from(Side::Sidebar, 0.2, 0.0), SIDEBAR_DEFAULT);
    assert_eq!(ratio(300.0, 0.0), 0.0);
}

#[test]
fn a_bigger_window_restores_a_wider_panel_up_to_the_limit() {
    let saved = Saved::of(widths(300.0, 260.0), false, false, 1500.0);
    let at_small = saved.widths(1000.0).sidebar;
    let at_big = saved.widths(1800.0).sidebar;
    assert!(at_small < 300.0 && at_big > 300.0);
    assert_eq!(saved.widths(5000.0).sidebar, SIDEBAR_MAX);
    assert_eq!(saved.widths(300.0).sidebar, SIDEBAR_MIN);
    assert_eq!(saved.widths(1500.0).details, DETAILS_DEFAULT);
}

#[test]
fn a_panel_at_its_default_saves_no_ratio() {
    let saved = Saved::of(Widths::default(), true, false, 1400.0);
    assert_eq!(saved.sidebar, None);
    assert_eq!(saved.details, None);
    assert!(saved.sidebar_hidden && !saved.details_hidden);
}

#[test]
fn the_limits_hold_when_dragging() {
    assert_eq!(dragged(Side::Sidebar, 100.0, 1400.0, 260.0), SIDEBAR_MIN);
    assert_eq!(dragged(Side::Sidebar, 900.0, 1400.0, 260.0), SIDEBAR_MAX);
    assert_eq!(dragged(Side::Sidebar, 310.0, 1400.0, 260.0), 310.0);
    assert_eq!(dragged(Side::Details, 1300.0, 1400.0, 272.0), DETAILS_MIN);
    assert_eq!(dragged(Side::Details, 200.0, 1400.0, 272.0), DETAILS_MAX);
    assert_eq!(dragged(Side::Details, 1000.0, 1400.0, 272.0), 400.0);
}

#[test]
fn a_drag_leaves_the_column_its_minimum_when_the_window_allows() {
    let window = 1000.0;
    let other = 260.0;
    let most = window - COLUMN_MIN - other;
    assert_eq!(dragged(Side::Sidebar, 900.0, window, other), most);
    assert_eq!(dragged(Side::Sidebar, 100.0, 700.0, 400.0), SIDEBAR_MIN);
}

#[test]
fn a_window_with_room_keeps_the_wanted_widths() {
    let w = shown(widths(300.0, 300.0), true, true, 1600.0);
    assert_eq!(w, widths(300.0, 300.0));
}

#[test]
fn a_hidden_panel_takes_no_room() {
    let w = shown(widths(300.0, 300.0), false, true, 1600.0);
    assert_eq!(w, widths(0.0, 300.0));
    let w = shown(widths(300.0, 300.0), true, false, 1600.0);
    assert_eq!(w, widths(300.0, 0.0));
}

#[test]
fn a_squeeze_takes_from_both_panels_down_to_their_minimums() {
    let w = shown(widths(400.0, 400.0), true, true, 1000.0);
    assert!((w.sidebar + w.details + COLUMN_MIN - 1000.0).abs() < 0.01);
    assert!(w.sidebar >= SIDEBAR_MIN && w.details >= DETAILS_MIN);
    assert!(w.sidebar < 400.0 && w.details < 400.0);
    let w = shown(widths(400.0, 400.0), true, true, 700.0);
    assert_eq!(w, widths(SIDEBAR_MIN, DETAILS_MIN));
}

#[test]
fn a_squeeze_never_pushes_a_panel_under_its_minimum() {
    let w = shown(widths(SIDEBAR_MIN, 500.0), true, true, 900.0);
    assert_eq!(w.sidebar, SIDEBAR_MIN);
    assert_eq!(w.details, 900.0 - COLUMN_MIN - SIDEBAR_MIN);
}

#[test]
fn a_saved_value_reads_back_and_a_bad_one_is_ignored() {
    let saved = Saved {
        sidebar: Some(0.21),
        details: None,
        sidebar_hidden: false,
        details_hidden: true,
    };
    assert_eq!(Saved::from_value(Some(&saved.to_value())), Some(saved));
    assert_eq!(Saved::from_value(None), None);
    assert_eq!(Saved::from_value(Some(&json!("x"))), None);
    let odd = Saved::from_value(Some(&json!({"sidebar": -1, "details": 7})));
    assert_eq!(odd.map(|s| (s.sidebar, s.details)), Some((None, None)));
}

#[test]
fn the_panels_land_in_the_store_and_read_back() {
    let dir = Scratch::new("panels-save");
    let file = dir.path().join("settings.json");
    let saved = Saved::of(widths(320.0, 300.0), false, true, 1400.0);
    save(&file, &saved).expect("the panels are saved");
    assert_eq!(load(&file), Some(saved));
}
