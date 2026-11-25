use super::*;

fn reference_squeezed(text: &str) -> String {
    as_stored(text)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

#[test]
fn the_table_is_what_claude_code_2_1_328_may_drop() {
    let dropped = [
        0x00ADu32, 0x034F, 0x061C, 0x115F, 0x1160, 0x17B4, 0x17B5, 0x180B, 0x180E, 0x180F, 0x200B,
        0x200C, 0x200D, 0x200E, 0x200F, 0x202A, 0x202B, 0x202C, 0x202D, 0x202E, 0x2060, 0x2064,
        0x2065, 0x2066, 0x2069, 0x206F, 0x3164, 0xFE00, 0xFE0F, 0xFEFF, 0xFFA0, 0xFFF0, 0xFFF8,
        0xFFF9, 0xFFFB, 0x1107F, 0x13430, 0x1343F, 0x16FE4, 0x1BCA0, 0x1BCA3, 0x1D173, 0x1D17A,
        0xE0000, 0xE0001, 0xE0041, 0xE007F, 0xE0080, 0xE0100, 0xE01F0, 0xE0FFF,
    ];
    for n in dropped {
        let c = char::from_u32(n).unwrap();
        assert!(is_dropped(c), "U+{n:04X}");
        assert_eq!(as_stored(&format!("a{c}b")), "ab", "U+{n:04X}");
    }
}

#[test]
fn the_edges_of_every_range_are_exact() {
    for (from, to) in DROPPED {
        for n in [from, to] {
            assert!(is_dropped(char::from_u32(n).unwrap()), "U+{n:04X} inside");
        }
        for n in [from - 1, to + 1] {
            let inside = DROPPED.iter().any(|(a, b)| (*a..=*b).contains(&n));
            let c = char::from_u32(n).unwrap();
            assert_eq!(is_dropped(c), inside, "U+{n:04X}");
        }
    }
}

#[test]
fn controls_go_except_tab_and_the_line_breaks() {
    for n in (0u32..0x20).chain(0x7F..0xA0) {
        let c = char::from_u32(n).unwrap();
        let kept = n == 9 || n == 10;
        let breaks = is_line_break(c);
        assert_eq!(is_dropped(c), !kept && !breaks, "U+{n:04X}");
    }
    assert_eq!(as_stored("a\tb\nc"), "a\tb\nc");
    assert_eq!(as_stored("a\u{7}b\u{1b}c\u{7f}d\u{9f}e"), "abcde");
}

#[test]
fn every_kind_of_line_break_is_one_newline() {
    assert_eq!(as_stored("a\r\nb"), "a\nb");
    assert_eq!(as_stored("a\rb"), "a\nb");
    assert_eq!(as_stored("a\r\r\nb"), "a\n\nb");
    assert_eq!(
        as_stored("a\u{b}b\u{c}c\u{85}d\u{2028}e\u{2029}f"),
        "a\nb\nc\nd\ne\nf"
    );
}

#[test]
fn what_claude_code_keeps_is_kept() {
    for c in [
        ' ',
        '\u{a0}',
        '\u{202f}',
        '\u{2000}',
        '\u{2800}',
        '\u{b7}',
        '\u{2022}',
        '\u{fffd}',
        '\u{fffc}',
        '\u{1d159}',
        '\u{3000}',
        'é',
        'ệ',
        '日',
        '\u{1f600}',
    ] {
        assert!(!is_dropped(c), "U+{:04X}", c as u32);
        assert_eq!(as_stored(&format!("a{c}b")), format!("a{c}b"));
    }
}

#[test]
fn an_unassigned_code_point_outside_the_table_is_kept() {
    for n in [
        0x0378u32, 0x05FF, 0x2072, 0xFFEF, 0xFFFE, 0xFFFF, 0xE1000, 0x2FA1F, 0x10FFFF,
    ] {
        let c = char::from_u32(n).unwrap();
        assert!(!is_dropped(c), "U+{n:04X}");
        assert_eq!(as_stored(&format!("a{c}b")), format!("a{c}b"), "U+{n:04X}");
    }
}

#[test]
fn plain_text_is_not_copied() {
    for plain in [
        "carry the lantern\n",
        "tab\there",
        "Việt Nam 日本",
        "\u{a0}\u{2800}",
    ] {
        assert!(matches!(as_stored(plain), Cow::Borrowed(_)), "{plain}");
    }
    assert!(matches!(as_stored("a\u{200b}b"), Cow::Owned(_)));
    assert!(matches!(as_stored("a\rb"), Cow::Owned(_)));
    assert!(matches!(as_stored("a\u{7}b"), Cow::Owned(_)));
}

#[test]
fn squeezing_joins_the_words_with_single_spaces_after_the_drop() {
    assert_eq!(
        squeezed("  carry\u{200b}the \n\n lantern\t"),
        "carrythe lantern"
    );
    assert_eq!(squeezed("a\u{a0}b\u{202f}c"), "a b c");
    assert_eq!(squeezed("\u{feff}"), "");
    assert_ne!(squeezed("carry the"), squeezed("carrythe"));
}

#[test]
fn the_one_pass_squeeze_is_the_old_two_step_one() {
    for text in [
        "",
        " ",
        "\n\n",
        "one",
        "  one  two\t\tthree\r\nfour\n",
        "a\u{200b} b\u{200b}\u{200b}c \u{200b} d",
        "\u{feff} \u{200b}\n x \u{3164}",
        "x\u{2028}y\u{2029}z\u{85}w\u{b}v\u{c}u",
        "👨\u{200d}👩\u{200d}👧 ❤\u{fe0f}\u{a0}ok",
        "می\u{200c}شود\n\n\u{200f}שלום",
        "trailing \u{200b}",
        "\u{200b}leading",
    ] {
        assert_eq!(squeezed(text), reference_squeezed(text), "{text:?}");
        assert_eq!(
            squeezed_chars(text).collect::<String>(),
            squeezed(text),
            "{text:?}"
        );
    }
}

#[test]
fn same_words_ignores_whitespace_and_dropped_characters_only() {
    assert!(same_words("a\u{200b}b  c\n", "ab c"));
    assert!(same_words("", "\u{200b} \n"));
    assert!(!same_words("a b", "ab"));
    assert!(!same_words("ab c", "ab d"));
    assert!(!same_words("ab", "ab c"));
}

#[test]
fn same_stored_keeps_the_spaces_and_the_line_breaks() {
    assert!(same_stored("a\u{200b}b\r\nc", "ab\nc"));
    assert!(!same_stored("ab  c", "ab c"));
    assert!(!same_stored("ab\nc", "ab c"));
}

#[test]
fn a_text_of_only_dropped_characters_and_spaces_is_blank() {
    for text in [
        "",
        " ",
        "\n\t",
        "\u{200b}",
        "\u{feff}\u{2060} \u{3164}\n",
        "\u{7}",
    ] {
        assert!(is_blank(text), "{text:?}");
    }
    for text in ["a", "\u{200b}a", "\u{2800}", "\u{fffd}", "\u{a0}x"] {
        assert!(!is_blank(text), "{text:?}");
    }
}

#[test]
fn a_context_character_matches_whether_claude_code_kept_it_or_dropped_it() {
    let pairs = [
        ("\u{2764}\u{fe0f}", "\u{2764}"),
        ("👨\u{200d}👩\u{200d}👧", "👨👩👧"),
        ("🏳\u{fe0f}\u{200d}🌈", "🏳🌈"),
        ("👍\u{1f3fd}\u{200d}", "👍\u{1f3fd}"),
        (
            "🏴\u{e0067}\u{e0062}\u{e0065}\u{e006e}\u{e0067}\u{e007f}",
            "🏴",
        ),
        (
            "🏴\u{e0067}\u{e0062}\u{e0073}\u{e0063}\u{e0074}\u{e007f}",
            "🏴",
        ),
        (
            "🏴\u{e0067}\u{e0062}\u{e0077}\u{e006c}\u{e0073}\u{e007f}",
            "🏴",
        ),
        ("می\u{200c}شود", "میشود"),
        ("क\u{094d}\u{200d}ष", "क\u{094d}ष"),
        ("\u{200f}שלום\u{200e}", "שלום"),
        ("\u{061c}مرحبا", "مرحبا"),
        ("ក\u{200b}ខ", "កខ"),
        ("1\u{fe0f}\u{20e3}", "1\u{20e3}"),
    ];
    for (kept, stripped) in pairs {
        assert!(same_words(kept, stripped), "{kept:?} {stripped:?}");
        assert_eq!(squeezed(kept), squeezed(stripped), "{kept:?}");
        assert_eq!(
            squeezed(&format!("a {kept} b")),
            squeezed(&format!("a {stripped} b"))
        );
    }
}

#[test]
fn texts_that_differ_in_a_visible_character_do_not_match_even_with_the_context_ones() {
    let pairs = [
        ("👨\u{200d}👩\u{200d}👧", "👨👩"),
        ("👨\u{200d}👩", "👩\u{200d}👨"),
        ("\u{2764}\u{fe0f}", "💔"),
        (
            "🏴\u{e0067}\u{e0062}\u{e0065}\u{e006e}\u{e0067}\u{e007f}",
            "🏳",
        ),
        ("می\u{200c}شود", "می\u{200c}شوم"),
        ("\u{200f}שלום", "שלום\u{200f}x"),
        ("ក\u{200b}ខ", "កគ"),
        ("1\u{fe0f}\u{20e3}", "2\u{fe0f}\u{20e3}"),
    ];
    for (a, b) in pairs {
        assert!(!same_words(a, b), "{a:?} {b:?}");
    }
}
