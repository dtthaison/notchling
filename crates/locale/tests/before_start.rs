use notchling_locale as t;

#[test]
fn get_before_start_is_english() {
    assert_eq!(t::units::list_separator(), ", ");
    assert_eq!(t::active(), "en");
    assert_eq!(t::fmt::count(1234), "1,234");
    assert_eq!(t::fmt::list(&["a", "b"]), "a, b");
}
