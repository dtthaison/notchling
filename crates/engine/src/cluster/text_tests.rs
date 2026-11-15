use super::text::{clean, plain, unseen, NAME_MAX};

const HIDDEN: [(&str, char, char); 21] = [
    ("soft hyphen", '\u{ad}', '\u{ad}'),
    ("combining grapheme joiner", '\u{34f}', '\u{34f}'),
    ("hangul choseong filler", '\u{115f}', '\u{115f}'),
    ("hangul jungseong filler", '\u{1160}', '\u{1160}'),
    ("arabic letter mark", '\u{61c}', '\u{61c}'),
    ("mongolian vowel separator", '\u{180e}', '\u{180e}'),
    ("zero width and marks", '\u{200b}', '\u{200f}'),
    ("line separator", '\u{2028}', '\u{2028}'),
    ("paragraph separator", '\u{2029}', '\u{2029}'),
    ("bidi embeddings", '\u{202a}', '\u{202e}'),
    ("invisible operators", '\u{2060}', '\u{2064}'),
    ("bidi isolates and deprecated", '\u{2066}', '\u{206f}'),
    ("hangul filler", '\u{3164}', '\u{3164}'),
    ("variation selectors", '\u{fe00}', '\u{fe0f}'),
    ("byte order mark", '\u{feff}', '\u{feff}'),
    ("halfwidth hangul filler", '\u{ffa0}', '\u{ffa0}'),
    ("interlinear annotation", '\u{fff9}', '\u{fffb}'),
    ("tag characters", '\u{e0000}', '\u{e007f}'),
    ("variation selectors supplement", '\u{e0100}', '\u{e01ef}'),
    ("c0 controls", '\u{0}', '\u{1f}'),
    ("c1 controls", '\u{7f}', '\u{9f}'),
];

fn every(first: char, last: char) -> Vec<char> {
    (first as u32..=last as u32)
        .filter_map(char::from_u32)
        .collect()
}

fn inside_a_range(c: char) -> bool {
    HIDDEN
        .iter()
        .any(|(_, a, b)| (*a as u32..=*b as u32).contains(&(c as u32)))
}

#[test]
fn every_character_in_every_hidden_range_is_unseen() {
    for (what, first, last) in HIDDEN {
        for c in every(first, last) {
            assert!(unseen(c), "{what}: U+{:04X}", c as u32);
        }
    }
}

#[test]
fn the_characters_just_outside_each_range_are_kept() {
    for (what, first, last) in HIDDEN {
        for c in [
            (first as u32).checked_sub(1).and_then(char::from_u32),
            char::from_u32(last as u32 + 1),
        ]
        .into_iter()
        .flatten()
        {
            assert_eq!(unseen(c), inside_a_range(c), "{what}: U+{:04X}", c as u32);
        }
    }
}

#[test]
fn ordinary_text_is_not_touched() {
    for text in [
        "Sam's MacBook Pro",
        "büro-Mac",
        "机器 2",
        "emoji 🙂 ok",
        "a b",
    ] {
        assert_eq!(clean(text, 200), text);
        assert_eq!(plain(text, 200), text);
    }
}

#[test]
fn clean_replaces_each_hidden_character_with_a_space_after_the_cap() {
    for (what, first, last) in HIDDEN {
        for c in every(first, last).into_iter().take(40) {
            let got = clean(&format!("a{c}b"), 200);
            assert_eq!(got, "a b", "{what}: U+{:04X}", c as u32);
        }
    }
    assert_eq!(clean("abcdef", 3), "abc");
    let capped = clean(&"\u{e0041}".repeat(500), 10);
    assert_eq!(capped.chars().count(), 10);
    assert!(capped.chars().all(|c| c == ' '));
}

#[test]
fn plain_drops_each_hidden_character_caps_and_trims() {
    for (what, first, last) in HIDDEN {
        for c in every(first, last).into_iter().take(40) {
            let got = plain(&format!(" a{c}b\u{200b} "), 200);
            assert_eq!(got, "ab", "{what}: U+{:04X}", c as u32);
        }
    }
    assert_eq!(plain("  Sam  ", NAME_MAX), "Sam");
    assert_eq!(
        plain(&"x".repeat(1 << 16), NAME_MAX).chars().count(),
        NAME_MAX
    );
    assert_eq!(plain(&format!("{}x", "\u{e0041}".repeat(1 << 12)), 5), "x");
    assert_eq!(plain("\u{3164}\u{ffa0}\u{115f}\u{1160}", NAME_MAX), "");
}

#[test]
fn a_name_spelled_in_tag_characters_does_not_survive() {
    let hidden: String = "evil"
        .chars()
        .filter_map(|c| char::from_u32(0xe0000 + c as u32))
        .collect();
    assert_eq!(plain(&format!("m5{hidden}"), NAME_MAX), "m5");
}
