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
