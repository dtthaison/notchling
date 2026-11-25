use super::*;

#[test]
fn a_press_over_text_nothing_covers_takes_that_text() {
    let candidates = [("a", false, true), ("b", true, true), ("c", true, true)];
    assert_eq!(pick(&candidates), Hit::Text("b"));
}

#[test]
fn a_press_over_text_a_sheet_covers_takes_nothing_and_is_not_a_click_elsewhere() {
    let candidates = [("a", true, false), ("b", false, true)];
    assert_eq!(pick(&candidates), Hit::Covered);
}

#[test]
fn a_covered_text_never_hides_a_reachable_one_at_the_same_spot() {
    let candidates = [("under", true, false), ("over", true, true)];
    assert_eq!(pick(&candidates), Hit::Text("over"));
}

#[test]
fn a_press_over_no_text_is_a_click_elsewhere() {
    let candidates = [("a", false, true), ("b", false, false)];
    assert_eq!(pick(&candidates), Hit::Empty);
    assert_eq!(pick::<&str>(&[]), Hit::Empty);
}

#[test]
fn nothing_behind_an_open_sheet_is_copied() {
    let asked = std::cell::Cell::new(false);
    let behind = || {
        asked.set(true);
        Some("the conversation".to_owned())
    };
    assert_eq!(copy_text(true, behind), None);
    assert!(!asked.get(), "the conversation is not even read");
}

#[test]
fn with_no_sheet_the_selection_behind_is_what_copy_takes() {
    assert_eq!(
        copy_text(false, || Some("the conversation".to_owned())).as_deref(),
        Some("the conversation")
    );
    assert_eq!(copy_text(false, || None), None);
}
