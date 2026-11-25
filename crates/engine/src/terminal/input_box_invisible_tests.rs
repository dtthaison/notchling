use super::input_box::{read, Fill, Input, Pictured};

const RULE_LINE: &str = "────────────────────────────────";
const SHOT: &str = "/tmp/drops/a.png";

fn box_of(rows: &[&str]) -> Input {
    let mut screen = vec![RULE_LINE.to_string()];
    screen.extend(rows.iter().map(|r| r.to_string()));
    screen.push(RULE_LINE.to_string());
    read(&screen, None).unwrap()
}

fn shown(text: &str, turn: &str, images: Option<usize>) -> bool {
    Pictured::of(text).is_some_and(|pictured| pictured.is_shown_as(turn, images))
}

#[test]
fn a_box_without_the_zero_width_spaces_of_the_message_shows_it() {
    let text = "carry\u{200b}the\u{200b}blue\u{200b}lantern";
    let seen = box_of(&["❯\u{a0}carrytheblue lantern"]);
    assert!(seen.shows("carry\u{200b}the blue lantern"));
    assert_eq!(seen.fill(text), Fill::Whole);
    assert!(seen.carries(text));
}

#[test]
fn a_filler_character_claude_code_drops_is_not_a_letter() {
    let seen = box_of(&["❯\u{a0}carry the lantern"]);
    assert_eq!(seen.fill("carry\u{3164} the lantern"), Fill::Whole);
    assert_eq!(seen.fill("carry the \u{115f}lantern"), Fill::Whole);
    assert_eq!(seen.fill("carry the lamp"), Fill::Foreign);
}

#[test]
fn a_picture_message_with_only_an_emoji_and_a_zero_width_space_is_its_chip() {
    let text = format!("\u{1f44d}\u{200b}\n\n{SHOT}");
    assert!(shown(&text, "[Image #2]\u{1f44d}", Some(1)));
    assert!(shown(&text, "[Image #2]\u{1f44d}\u{200b}", Some(1)));
    assert!(!shown(&text, "[Image #2]\u{1f44e}", Some(1)));
}

#[test]
fn a_picture_message_with_words_and_zero_width_spaces_is_its_chip() {
    let text = format!("carry\u{200b}the\u{200b}lantern\n\n{SHOT}");
    assert!(shown(&text, "[Image #2]carrythelantern", Some(1)));
    assert!(shown(&text, "[Image #2]carry the lantern", Some(1)));
    assert!(!shown(&text, "[Image #2]carry the lamp", Some(1)));
}

#[test]
fn a_picture_path_after_windows_or_unicode_line_breaks_is_still_a_picture() {
    for breaks in [
        "\r\n\r\n",
        "\r\r",
        "\u{2028}\u{2028}",
        "\u{85}\u{85}",
        "\u{b}\u{c}",
    ] {
        let text = format!("why this lamp{breaks}{SHOT}{breaks}");
        let pictured = Pictured::of(&text);
        assert!(pictured.is_some(), "{breaks:?}");
        assert!(
            pictured.is_some_and(|p| p.is_shown_as("[Image #2]why this lamp", Some(1))),
            "{breaks:?}"
        );
    }
}

#[test]
fn a_dropped_character_beside_a_picture_path_is_still_a_picture() {
    for text in [
        format!("why\n\u{200b}{SHOT}"),
        format!("why\n{SHOT}\u{feff}"),
        format!("why\n\u{3164}{SHOT}\u{2060}\n"),
    ] {
        assert!(Pictured::of(&text).is_some(), "{text:?}");
    }
}

#[test]
fn a_picture_message_with_windows_line_breaks_is_the_box_with_its_chip() {
    let text = format!("why this lamp\r\n\r\n{SHOT}\r\n");
    let seen = box_of(&["❯\u{a0}[Image #1]why this lamp"]);
    assert_eq!(seen.fill(&text), Fill::Whole);
    assert!(seen.carries(&text));
}

#[test]
fn a_box_with_a_start_of_what_was_typed_is_a_leftover_through_dropped_characters() {
    let seen = box_of(&["❯\u{a0}carry the blue lant"]);
    let sent = "carry\u{200b}the blue lantern across the quiet bridge".to_string();
    assert!(seen.leftover_of(&[sent]));
    let whole = box_of(&["❯\u{a0}carrytheblue lantern"]);
    assert!(whole.leftover_of(&["carry\u{200b}the\u{200b}blue lantern".to_string()]));
}

#[test]
fn a_box_with_other_words_is_not_a_leftover_even_with_dropped_characters() {
    let seen = box_of(&["❯\u{a0}carry the red lantern"]);
    let sent = "carry\u{200b}the blue lantern across the quiet bridge".to_string();
    assert!(!seen.leftover_of(&[sent]));
    assert!(!seen.leftover_of(&[]));
    let empty = box_of(&["❯\u{a0}"]);
    assert!(!empty.leftover_of(&["carry the red lantern".to_string()]));
}

#[test]
fn a_leftover_picture_message_is_known_by_its_chip_and_its_words() {
    let text = format!("carry\u{200b}the lantern\r\n\r\n{SHOT}");
    let seen = box_of(&["❯\u{a0}[Image #1]carry the lantern"]);
    assert!(seen.leftover_of(&[text]));
}
