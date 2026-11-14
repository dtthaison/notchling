use crate::choose::Choice;
use crate::format::{Body, Part, Table};
use crate::pseudo::transform;
use crate::state::Active;
use crate::{Arg, Text};

#[test]
fn transform_accents_pads_and_wraps() {
    let out = transform("Send");
    assert!(out.starts_with("\u{27e6}\u{160}\u{e9}\u{f1}"));
    assert!(out.ends_with("~\u{27e7}"));
    let inside: Vec<char> = out.chars().skip(1).take(out.chars().count() - 2).collect();
    assert_eq!(inside.len(), 6);
}

#[test]
fn transform_of_nothing_is_an_empty_pair() {
    assert_eq!(transform(""), "\u{27e6}\u{27e7}");
}

static EN_BODIES: [Body; 2] = [
    Body::Plain("Send"),
    Body::Parts(&[Part::Lit("Moved to "), Part::Str(0)]),
];
static TABLES: [(&str, Table); 1] = [("en", Table::Full(&EN_BODIES))];

fn pseudo() -> Active {
    Active::with_tables(
        &Choice {
            tag: "en",
            pseudo: true,
            region: None,
            mark_fallback: false,
        },
        &TABLES,
    )
}

#[test]
fn a_formatted_message_keeps_its_variables() {
    let out = pseudo().format(Text(1), &[Arg::Str("docs")]);
    assert!(out.starts_with('\u{27e6}'));
    assert!(out.ends_with('\u{27e7}'));
    assert!(out.contains("M\u{f4}v\u{e9}d \u{163}\u{f4} docs"));
    assert_eq!(out.chars().count(), 19 + 2);
}

#[test]
fn a_plain_message_is_cached_once() {
    let active = pseudo();
    let a = active.get(Text(0));
    let b = active.get(Text(0));
    assert!(std::ptr::eq(a, b));
    assert_eq!(a, transform("Send"));
}
