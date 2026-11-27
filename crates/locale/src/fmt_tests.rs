use crate::fmt::{decimal_with, join};
use crate::format::{Cat, Numbers};
use crate::numbers::Icu;

#[test]
fn en_groups_with_commas() {
    let icu = Icu::new("en", None);
    assert_eq!(icu.int(1234), "1,234");
    assert_eq!(icu.int(-5), "-5");
    assert_eq!(icu.int(0), "0");
    assert_eq!(icu.int(1_234_567), "1,234,567");
}

#[test]
fn en_with_region_de_groups_with_dots() {
    let icu = Icu::new("en", Some("DE"));
    assert_eq!(icu.int(1234), "1.234");
}

#[test]
fn english_plural_categories() {
    let icu = Icu::new("en", None);
    assert_eq!(icu.category(1), Cat::One);
    assert_eq!(icu.category(0), Cat::Other);
    assert_eq!(icu.category(2), Cat::Other);
    assert_eq!(icu.category(21), Cat::Other);
}

#[test]
fn other_languages_have_their_own_categories() {
    let polish = Icu::new("pl", None);
    assert_eq!(polish.category(1), Cat::One);
    assert_eq!(polish.category(3), Cat::Few);
    assert_eq!(polish.category(5), Cat::Many);
    assert_eq!(Icu::new("ja", None).category(1), Cat::Other);
}

#[test]
fn unknown_tag_reads_as_english() {
    let icu = Icu::new("not a tag", None);
    assert_eq!(icu.int(1234), "1,234");
    assert_eq!(icu.category(1), Cat::One);
}

#[test]
fn decimal_rounds_to_digits() {
    let icu = Icu::new("en", None);
    assert_eq!(decimal_with(&icu, 12.34, 1), "12.3");
    assert_eq!(decimal_with(&icu, 0.05, 1), "0.1");
    assert_eq!(decimal_with(&icu, 12.0, 1), "12.0");
    assert_eq!(decimal_with(&icu, f64::NAN, 1), "0");
    assert_eq!(decimal_with(&icu, f64::INFINITY, 1), "0");
    assert_eq!(decimal_with(&icu, 1234.5, 0), "1,235");
    assert_eq!(decimal_with(&Icu::new("de", None), 12.3, 1), "12,3");
}

#[test]
fn list_joins_with_separator() {
    assert_eq!(join(&["a", "b"], ", "), "a, b");
    assert_eq!(join(&["a"], ", "), "a");
    assert_eq!(join::<&str>(&[], ", "), "");
    assert_eq!(crate::fmt::list(&["a", "b", "c"]), "a, b, c");
    assert_eq!(crate::fmt::list(&[String::from("x")]), "x");
}

#[test]
fn count_uses_english_before_start() {
    assert_eq!(crate::fmt::count(1234), "1,234");
    assert_eq!(crate::fmt::decimal(12.3, 1), "12.3");
}
