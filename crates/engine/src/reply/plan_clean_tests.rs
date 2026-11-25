use super::plan::{reply, reply_in, Step};

fn pasted(text: &str) -> String {
    let plan = reply(text);
    plan.steps
        .iter()
        .find_map(|step| match step {
            Step::Paste(text) => Some(text.clone()),
            _ => None,
        })
        .expect("a paste step")
}

#[test]
fn bidi_overrides_embeddings_and_isolates_are_not_typed() {
    for mark in [
        '\u{202A}', '\u{202B}', '\u{202C}', '\u{202D}', '\u{202E}', '\u{2066}', '\u{2067}',
        '\u{2068}', '\u{2069}',
    ] {
        let text = format!("read this{mark} as it is");
        assert_eq!(pasted(&text), "read this as it is", "U+{:04X}", mark as u32);
    }
}

#[test]
fn a_leading_slash_with_a_zero_width_space_is_typed_without_it() {
    assert_eq!(pasted("/model\u{200B} opus"), "/model opus");
}

#[test]
fn other_characters_claude_code_always_drops_are_not_typed() {
    for mark in [
        '\u{00AD}',
        '\u{2060}',
        '\u{2061}',
        '\u{3164}',
        '\u{FEFF}',
        '\u{FFA0}',
        '\u{FFF9}',
        '\u{1D173}',
        '\u{115F}',
        '\u{16FE4}',
    ] {
        let text = format!("one{mark}two");
        assert_eq!(pasted(&text), "onetwo", "U+{:04X}", mark as u32);
    }
}

#[test]
fn control_characters_are_not_typed_but_tabs_and_line_breaks_are_kept() {
    assert_eq!(pasted("a\u{1}b\u{7f}c\u{85}d"), "abc d");
    assert_eq!(pasted("a\tb\nc"), "a\tb\nc");
    assert_eq!(pasted("a\r\nb"), "a\nb");
}

#[test]
fn tag_characters_outside_a_flag_are_not_typed() {
    assert_eq!(pasted("a\u{E0041}b"), "ab");
    assert_eq!(pasted("a\u{E0001}b\u{E007F}c"), "abc");
}

#[test]
fn a_broken_flag_sequence_keeps_the_flag_and_loses_the_tags() {
    let broken = "\u{1F3F4}\u{E0067}\u{E0062}";
    assert_eq!(pasted(broken), "\u{1F3F4}");
    let unknown = "\u{1F3F4}\u{E0061}\u{E0062}\u{E007F}";
    assert_eq!(pasted(unknown), "\u{1F3F4}");
}

#[test]
fn a_heart_with_its_emoji_selector_reaches_the_box_unchanged() {
    assert_eq!(pasted("I \u{2764}\u{FE0F} this"), "I \u{2764}\u{FE0F} this");
    assert_eq!(pasted("I \u{2764} this"), "I \u{2764} this");
}

#[test]
fn a_family_with_its_joiners_reaches_the_box_unchanged() {
    let family = "\u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F467}";
    assert_eq!(pasted(&format!("us: {family}")), format!("us: {family}"));
}

#[test]
fn the_three_tag_flags_reach_the_box_unchanged() {
    for tags in ["gbeng", "gbsct", "gbwls"] {
        let flag: String = std::iter::once('\u{1F3F4}')
            .chain(
                tags.chars()
                    .map(|c| char::from_u32(0xE0000 + c as u32).unwrap()),
            )
            .chain(std::iter::once('\u{E007F}'))
            .collect();
        assert_eq!(
            pasted(&format!("go {flag}!")),
            format!("go {flag}!"),
            "{tags}"
        );
    }
}

#[test]
fn persian_text_keeps_its_zero_width_non_joiner() {
    let text = "\u{645}\u{6CC}\u{200C}\u{62E}\u{648}\u{627}\u{647}\u{645}";
    assert_eq!(pasted(text), text);
}

#[test]
fn hindi_text_keeps_its_zero_width_joiner() {
    let text = "\u{915}\u{94D}\u{200D}\u{937}";
    assert_eq!(pasted(text), text);
}

#[test]
fn a_right_to_left_mark_in_right_to_left_text_reaches_the_box_unchanged() {
    let text = "\u{5E9}\u{5DC}\u{5D5}\u{5DD}\u{200F} (hi)";
    assert_eq!(pasted(text), text);
    let left = "see \u{200E}\u{5E9}\u{5DC}\u{5D5}\u{5DD}";
    assert_eq!(pasted(left), left);
}

#[test]
fn variation_selectors_reach_the_box_unchanged() {
    for text in [
        "\u{2764}\u{FE0E}",
        "\u{8FBB}\u{E0100}",
        "1\u{FE0F}\u{20E3}",
        "\u{2200}\u{FE00}",
    ] {
        assert_eq!(pasted(text), text);
    }
}

#[test]
fn a_zero_width_space_is_kept_in_thai_khmer_lao_and_myanmar_text_and_dropped_elsewhere() {
    for word in [
        "\u{E01}\u{200B}\u{E02}",
        "\u{1780}\u{200B}\u{1781}",
        "\u{E81}\u{200B}\u{E82}",
        "\u{1000}\u{200B}\u{1001}",
    ] {
        assert_eq!(pasted(word), word);
    }
    assert_eq!(pasted("a\u{200B}b"), "ab");
}

#[test]
fn a_message_with_nothing_to_drop_is_typed_as_it_is() {
    for text in [
        "hello there",
        "line one\n\nline two",
        "tab\tseparated",
        "caf\u{E9}",
    ] {
        assert_eq!(pasted(text), text);
    }
}

#[test]
fn a_reply_into_an_open_attach_cleans_the_same_way() {
    let plan = reply_in("a\u{202E}b");
    let text = plan.steps.iter().find_map(|step| match step {
        Step::Paste(text) => Some(text.as_str()),
        _ => None,
    });
    assert_eq!(text, Some("ab"));
}

#[test]
fn a_shell_line_is_cleaned_before_it_is_normalised() {
    assert_eq!(pasted("!ls\u{202E} -la"), "!ls -la");
}
