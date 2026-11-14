use crate::format::{format_body, Body, Cat, Numbers, Part};
use crate::{lookup, units, Arg, Num, Text};

struct English;

impl Numbers for English {
    fn int(&self, n: i64) -> String {
        n.to_string()
    }

    fn category(&self, n: i64) -> Cat {
        if n == 1 {
            Cat::One
        } else {
            Cat::Other
        }
    }
}

#[test]
fn plain_message_is_static() {
    assert_eq!(units::list_separator(), ", ");
    let a = units::LIST_SEPARATOR.get();
    let b = units::LIST_SEPARATOR.get();
    assert!(std::ptr::eq(a, b));
}

#[test]
fn lookup_finds_plain_and_refuses_unknown() {
    assert_eq!(lookup("units-list-separator"), Some(units::LIST_SEPARATOR));
    assert_eq!(lookup("units-nope"), None);
    assert_eq!(lookup(""), None);
}

#[test]
fn key_and_english_come_back() {
    assert_eq!(units::LIST_SEPARATOR.key(), "units-list-separator");
    assert_eq!(units::LIST_SEPARATOR.english(), ", ");
    assert_eq!(Text(u16::MAX).key(), "");
    assert_eq!(Text(u16::MAX).get(), "");
}

#[test]
fn language_name_is_english() {
    assert_eq!(crate::language::name(), "English");
}

#[test]
fn num_saturates() {
    assert_eq!(Num::from(u64::MAX).0, i64::MAX);
    assert_eq!(Num::from(usize::MAX).0, i64::MAX);
    assert_eq!(Num::from(-3i32).0, -3);
    assert_eq!(Num::from(7u8).0, 7);
}

#[test]
fn format_fills_parts() {
    let body = Body::Parts(&[
        Part::Lit("a "),
        Part::Str(0),
        Part::Lit(" b "),
        Part::Int(1),
    ]);
    let args = [Arg::Str("x"), Arg::Num(7.into())];
    assert_eq!(format_body(&body, &args, &English), "a x b 7");
}

#[test]
fn format_picks_the_plural_variant() {
    let body = Body::Plural(
        0,
        &[
            (Cat::One, &[Part::Int(0), Part::Lit(" file")]),
            (Cat::Other, &[Part::Int(0), Part::Lit(" files")]),
        ],
    );
    assert_eq!(
        format_body(&body, &[Arg::Num(1.into())], &English),
        "1 file"
    );
    assert_eq!(
        format_body(&body, &[Arg::Num(2.into())], &English),
        "2 files"
    );
    assert_eq!(
        format_body(&body, &[Arg::Num(0.into())], &English),
        "0 files"
    );
}

#[test]
fn missing_arguments_print_nothing() {
    let body = Body::Parts(&[Part::Lit("a "), Part::Str(3)]);
    assert_eq!(format_body(&body, &[], &English), "a ");
}

#[test]
fn shipped_lists_english_first() {
    assert_eq!(crate::shipped(), vec!["en"]);
}
