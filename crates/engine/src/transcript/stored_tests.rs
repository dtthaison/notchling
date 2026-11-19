use super::*;

#[test]
fn the_table_is_what_claude_code_2_1_328_drops() {
    let dropped = [
        0x00ADu32, 0x034F, 0x061C, 0x115F, 0x1160, 0x17B4, 0x17B5, 0x180B, 0x180E, 0x180F, 0x200B,
        0x200C, 0x200D, 0x200E, 0x200F, 0x202A, 0x202B, 0x202C, 0x202D, 0x202E, 0x2060, 0x2064,
        0x2066, 0x2069, 0x206F, 0x3164, 0xFE00, 0xFE0F, 0xFEFF, 0xFFA0, 0xFFF0, 0xFFF9, 0xFFFB,
        0x1107F, 0x13430, 0x1343F, 0x16FE4, 0x1BCA0, 0x1BCA3, 0x1D173, 0x1D17A, 0xE0000, 0xE0001,
        0xE0041, 0xE007F, 0xE0100, 0xE0FFF,
    ];
    for n in dropped {
        let c = char::from_u32(n).unwrap();
        assert!(is_dropped(c), "U+{n:04X}");
        assert_eq!(as_stored(&format!("a{c}b")), "ab", "U+{n:04X}");
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
fn plain_text_is_not_copied() {
    assert!(matches!(as_stored("carry the lantern\n"), Cow::Borrowed(_)));
    assert!(matches!(as_stored("Việt Nam 日本"), Cow::Borrowed(_)));
    assert!(matches!(as_stored("a\u{200b}b"), Cow::Owned(_)));
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
