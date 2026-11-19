use super::input_box::shown_as_chips;

const ONE: &str =
    "where add assets\n\n'/Users/me/Library/Application Support/Notch/drops/1/Shot 1\u{202f}AM.png'";
const TWO: &str = "compare these\n\n/tmp/drops/a.png '/tmp/drops/b c.jpg'";

#[test]
fn a_turn_is_the_message_with_its_picture_paths_as_chips() {
    assert!(shown_as_chips(ONE, "[Image #3]where add assets"));
    assert!(shown_as_chips(ONE, "[Image #1024]where add assets"));
    assert!(shown_as_chips(TWO, "[Image #4] [Image #5]compare these"));
    assert!(shown_as_chips(
        TWO,
        "[Image #4]compare these /tmp/drops/a.png"
    ));
    assert!(shown_as_chips(
        "one\ntwo three\n\n/tmp/drops/a.png",
        "[Image #2]one\ntwo three"
    ));
    assert!(shown_as_chips("/tmp/drops/a.png", "[Image #2]"));
}

#[test]
fn only_that_message_is() {
    assert!(!shown_as_chips(ONE, "where add assets"));
    assert!(!shown_as_chips(ONE, "[Image #3]where add asset"));
    assert!(!shown_as_chips(ONE, "[Image #3]where add assets too"));
    assert!(!shown_as_chips(ONE, "[Image #3][Image #4]where add assets"));
    assert!(!shown_as_chips(ONE, "[Image #3]"));
    assert!(!shown_as_chips(TWO, "[Image #4]compare"));
}

#[test]
fn a_message_with_no_picture_path_is_never_matched_through_a_chip() {
    assert!(!shown_as_chips(
        "where add assets",
        "[Image #3]where add assets"
    ));
    assert!(!shown_as_chips("see [Image #1] now", "see [Image #2] now"));
    assert!(!shown_as_chips("see [Image #1] now", "see [Image #1] now"));
    assert!(!shown_as_chips(
        "see [Image #1] now\n\n/tmp/drops/a.png",
        "[Image #2]see [Image #1] now"
    ));
    assert!(!shown_as_chips(
        "look at /tmp/drops/a.png",
        "[Image #2]look at"
    ));
}
