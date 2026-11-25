use notchling_locale as t;

#[test]
fn the_first_apply_wins_and_sets_the_region() {
    let choice = t::Choice {
        tag: "en",
        pseudo: false,
        region: Some("DE".to_string()),
        mark_fallback: false,
    };
    assert!(t::apply(&choice));
    assert!(!t::apply(&choice));
    assert_eq!(t::fmt::count(1234), "1.234");
    assert_eq!(t::active(), "en");
}
