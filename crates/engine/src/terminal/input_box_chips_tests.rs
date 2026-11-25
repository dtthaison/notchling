use super::input_box::Pictured;

const ONE: &str =
    "where add assets\n\n'/Users/me/Library/Application Support/Notch/drops/1/Shot 1\u{202f}AM.png'";
const TWO: &str = "compare these\n\n/tmp/drops/a.png '/tmp/drops/b c.jpg'";
const ALONE: &str = "/tmp/drops/a.png";
const EMOJI: &str = "\u{1f44d}\n\n/tmp/drops/a.png";

fn shown(text: &str, turn: &str, images: Option<usize>) -> bool {
    Pictured::of(text).is_some_and(|pictured| pictured.is_shown_as(turn, images))
}

#[test]
fn a_turn_is_the_message_with_its_picture_paths_as_chips() {
    assert!(shown(ONE, "[Image #3]where add assets", Some(1)));
    assert!(shown(ONE, "[Image #3]where add assets", None));
    assert!(shown(ONE, "[Image #1024]where add assets", Some(1)));
    assert!(shown(TWO, "[Image #4] [Image #5]compare these", Some(2)));
    assert!(shown(
        TWO,
        "[Image #4]compare these /tmp/drops/a.png",
        Some(1)
    ));
    assert!(shown(
        "one\ntwo three\n\n/tmp/drops/a.png",
        "[Image #2]one\ntwo three",
        Some(1)
    ));
    assert!(shown(ALONE, "[Image #2]", Some(1)));
}

#[test]
fn only_that_message_is() {
    assert!(!shown(ONE, "where add assets", Some(0)));
    assert!(!shown(ONE, "[Image #3]where add asset", Some(1)));
    assert!(!shown(ONE, "[Image #3]where add assets too", Some(1)));
    assert!(!shown(ONE, "[Image #3][Image #4]where add assets", Some(2)));
    assert!(!shown(ONE, "[Image #3]", Some(1)));
    assert!(!shown(TWO, "[Image #4]compare", Some(1)));
}

#[test]
fn a_message_with_no_picture_path_is_never_matched_through_a_chip() {
    assert!(Pictured::of("where add assets").is_none());
    assert!(Pictured::of("see [Image #1] now").is_none());
    assert!(Pictured::of("look at /tmp/drops/a.rs").is_none());
    assert!(Pictured::of("see [Image #1] now\n\n/tmp/drops/a.png").is_none());
    assert!(Pictured::of("look at /tmp/drops/a.png").is_none());
}

#[test]
fn a_chip_needs_its_image_block() {
    assert!(shown(ONE, "[Image #3]where add assets", Some(1)));
    assert!(shown(ONE, "[Image #3]where add assets", Some(2)));
    assert!(!shown(ONE, "[Image #3]where add assets", Some(0)));
    assert!(!shown(TWO, "[Image #4] [Image #5]compare these", Some(1)));
}

#[test]
fn a_turn_of_only_its_pictures_is_a_message_of_only_pictures() {
    assert!(shown(ALONE, "", Some(1)));
    assert!(shown(ALONE, " \n", Some(3)));
    assert!(shown("/tmp/drops/a.png '/tmp/drops/b c.jpg'", "", Some(2)));
    assert!(!shown(ALONE, "", Some(0)));
    assert!(!shown(ALONE, "", None));
    assert!(!shown("/tmp/drops/a.png '/tmp/drops/b c.jpg'", "", Some(1)));
    assert!(!shown(ONE, "", Some(1)), "words that never arrived");
    assert!(!shown(EMOJI, "", Some(1)));
}

#[test]
fn a_caption_with_no_letters_is_told_by_its_exact_characters() {
    assert!(shown(EMOJI, "[Image #1]\u{1f44d}", Some(1)));
    assert!(!shown(EMOJI, "[Image #1]", Some(1)));
    assert!(!shown(EMOJI, "[Image #1]!", Some(1)));
    assert!(!shown(EMOJI, "[Image #1]\u{1f44e}", Some(1)));
    assert!(shown("?\n\n/tmp/drops/a.png", "[Image #1]?", Some(1)));
    assert!(!shown("?\n\n/tmp/drops/a.png", "[Image #1]!", Some(1)));
    assert!(!shown(ALONE, "[Image #1]\u{1f44d}", Some(1)));
    assert!(!shown(ALONE, "[Image #1]!", Some(1)));
}
