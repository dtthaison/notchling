use super::input_box::{read, Fill, Input};

const RULE_LINE: &str = "────────────────────────────────";

fn box_of(rows: &[&str]) -> Input {
    let mut screen = vec![RULE_LINE.to_string()];
    screen.extend(rows.iter().map(|r| r.to_string()));
    screen.push(RULE_LINE.to_string());
    read(&screen, None).unwrap()
}

const WORDS: &str = "i want click refresh it will refresh is it ok?";
const SHOT: &str = "/Users/me/Library/Application Support/Notchling/drops/1794723941465/Screenshot 2026-11-15 at 1.25.35 PM.png";

fn with_shot(words: &str) -> String {
    format!("{words}\n\n'{SHOT}'")
}

#[test]
fn a_picture_path_the_session_turned_into_a_chip_is_still_the_message() {
    let text = with_shot(WORDS);
    let seen = box_of(&["❯\u{a0}[Image #48]i want click refresh it will refresh is it ok?"]);
    assert_eq!(seen.fill(&text), Fill::Whole);
    assert!(seen.carries(&text));
    let narrow = box_of(&[
        "❯\u{a0}[Image #48]i want click refresh it will",
        "  refresh is it ok?",
    ]);
    assert_eq!(
        narrow.fill(&text),
        Fill::Whole,
        "wrapping and the chip's number do not matter"
    );
    let two = "Say only the word race.\n\n/tmp/shots/one.png /tmp/shots/two.jpg";
    let both = box_of(&["❯\u{a0}[Image #5] [Image #6]Say only the word race."]);
    assert_eq!(both.fill(two), Fill::Whole);
    let before = "'/tmp/a b.PNG'\n\nlook at this";
    assert_eq!(
        box_of(&["❯\u{a0}[Image #1]look at this"]).fill(before),
        Fill::Whole
    );
    assert_eq!(
        box_of(&["❯\u{a0}[Image #1]"]).fill("'/tmp/a b.png'"),
        Fill::Whole
    );
}

#[test]
fn chips_that_are_in_before_their_words_are_a_part_of_the_message() {
    let text = with_shot(WORDS);
    assert_eq!(box_of(&["❯\u{a0}[Image #48]"]).fill(&text), Fill::Part);
    assert_eq!(
        box_of(&["❯\u{a0}[Image #48]i want click"]).fill(&text),
        Fill::Part
    );
    assert_eq!(box_of(&["❯\u{a0}[Image #48]"]).fill("hello"), Fill::Foreign);
}

#[test]
fn a_picture_the_session_could_not_read_stays_a_path_in_the_box() {
    let text = with_shot(WORDS);
    let unread = box_of(&[
        "❯\u{a0}i want click refresh it will refresh is it ok?",
        &format!("  '{SHOT}'"),
    ]);
    assert_eq!(
        unread.fill(&text),
        Fill::Whole,
        "the whole paste went in as it was"
    );
    let two = "look\n\n/tmp/a.png /tmp/b.png";
    assert_eq!(
        box_of(&["❯\u{a0}[Image #3]look", "  /tmp/b.png"]).fill(two),
        Fill::Whole,
        "one of two was read"
    );
    assert_eq!(
        box_of(&["❯\u{a0}[Image #3]look", "  /tmp/a.png"]).fill(two),
        Fill::Whole
    );
    assert_eq!(
        box_of(&["❯\u{a0}[Image #3]look", "  /tmp/c.png"]).fill(two),
        Fill::Foreign
    );
}

#[test]
fn many_pictures_are_all_read_or_none_of_them() {
    let paths: Vec<String> = (0..9).map(|n| format!("/tmp/p{n}.png")).collect();
    let text = format!("look\n\n{}", paths.join(" "));
    let chips: String = (1..=9).map(|n| format!("[Image #{n}]")).collect();
    let all = format!("❯\u{a0}{chips}look");
    assert_eq!(box_of(&[&all]).fill(&text), Fill::Whole);
    assert_eq!(
        box_of(&["❯\u{a0}[Image #1]look"]).fill(&text),
        Fill::Foreign
    );
    let some = format!("❯\u{a0}[Image #1] [Image #2]look {}", paths[2..].join(" "));
    assert_eq!(box_of(&[&some]).fill(&text), Fill::Foreign);
}

#[test]
fn a_chip_that_is_not_one_of_our_pictures_is_somebody_elses() {
    let text = with_shot(WORDS);
    assert_eq!(
        box_of(&["❯\u{a0}[Image #9]Fix the tests"]).fill("Fix the tests"),
        Fill::Foreign,
        "a message with no picture"
    );
    assert_eq!(
        box_of(&["❯\u{a0}[Image #1] [Image #2]i want click refresh it will refresh is it ok?"])
            .fill(&text),
        Fill::Foreign,
        "more chips than pictures"
    );
    assert_eq!(
        box_of(&["❯\u{a0}[Image #48]i want click refresh it will refresh is it ok? and mine"])
            .fill(&text),
        Fill::Foreign
    );
    assert_eq!(
        box_of(&["❯\u{a0}[Image #48]a draft of my own"]).fill(&text),
        Fill::Foreign
    );
    let not_a_picture = "see /tmp/notes.png now";
    assert_eq!(
        box_of(&["❯\u{a0}[Image #1]see now"]).fill(not_a_picture),
        Fill::Foreign,
        "a path inside a sentence is never read as a picture"
    );
}

#[test]
fn only_a_line_of_nothing_but_paths_in_the_form_chat_writes_holds_pictures() {
    let own = |rows: &[&str], text: &str| box_of(rows).fill(text);
    for text in [
        "/fix the layout bug in header.png",
        "/review /tmp/notes.png",
        "/tmp/a.png and /tmp/b.png",
        "'/tmp/a.png' vs '/tmp/b.png'",
        "see /tmp/notes.png please",
        "hello /tmp/x.jpg",
        "look\n\n/tmp/a.png then",
        "look\n\n/a.png",
        "look\n\nnotes.png",
    ] {
        assert_eq!(own(&["❯\u{a0}[Image #1]"], text), Fill::Foreign, "{text}");
        assert_eq!(
            own(&["❯\u{a0}[Image #1]look"], text),
            Fill::Foreign,
            "{text}"
        );
    }
    assert_eq!(
        own(
            &["❯\u{a0}/fix the layout bug in header.png"],
            "/fix the layout bug in header.png"
        ),
        Fill::Whole
    );
    let phone = "hello /tmp/phone-images/x.jpg";
    assert_eq!(
        own(&["❯\u{a0}hello /tmp/phone-images/x.jpg"], phone),
        Fill::Whole
    );
    let chats = "look\n\n'/tmp/it'\\''s a.png' /tmp/b.png /tmp/notes.rs";
    assert_eq!(
        own(
            &["❯\u{a0}[Image #1] [Image #2]look", "  /tmp/notes.rs"],
            chats
        ),
        Fill::Whole
    );
    let phone_only = "\"/tmp/phone images/x.jpg\"";
    assert_eq!(own(&["❯\u{a0}[Image #1]"], phone_only), Fill::Whole);
}

#[test]
fn a_chip_the_person_pasted_is_never_the_message_nor_a_leftover_of_it() {
    let plain = [String::from("Please run the whole suite again")];
    for row in [
        "❯\u{a0}[Image #5]Please run the whole suite again",
        "❯\u{a0}[Image #5]Please run the",
        "❯\u{a0}[Image #5] [Image #6]Please run the whole suite again",
    ] {
        let left = box_of(&[row]);
        assert!(!left.leftover_of(&plain), "{row}");
        assert_eq!(left.fill(&plain[0]), Fill::Foreign, "{row}");
    }
    let one = [String::from("Fix this bug now\n\n/tmp/a.png")];
    assert!(!box_of(&["❯\u{a0}[Image #5] [Image #6]Fix this bug now"]).leftover_of(&one));
    assert!(box_of(&["❯\u{a0}[Image #5]Fix this bug now"]).leftover_of(&one));
    assert!(box_of(&["❯\u{a0}Fix this bug now"]).leftover_of(&one));
}

#[test]
fn a_message_that_names_a_chip_in_words_is_told_by_its_words_exactly() {
    let text = "why does [Image #1] fail?";
    assert_eq!(
        box_of(&["❯\u{a0}why does [Image #1] fail?"]).fill(text),
        Fill::Whole
    );
    assert_eq!(
        box_of(&["❯\u{a0}why does [Image #1]", "  fail?"]).fill(text),
        Fill::Whole
    );
    assert_eq!(box_of(&["❯\u{a0}why does "]).fill(text), Fill::Part);
    assert_eq!(box_of(&["❯\u{a0}why does [Ima"]).fill(text), Fill::Part);
    assert_eq!(
        box_of(&["❯\u{a0}why does [Image #1]"]).fill(text),
        Fill::Part
    );
    for row in [
        "❯\u{a0}why does [Image #2] fail?",
        "❯\u{a0}why does fail?",
        "❯\u{a0}[Image #1]why does [Image #1] fail?",
        "❯\u{a0}[Image #2]why does [Image #1] fail?",
        "❯\u{a0}why does [Image #1] fail?more",
    ] {
        assert_eq!(box_of(&[row]).fill(text), Fill::Foreign, "{row}");
    }
    let more = box_of(&["❯\u{a0}why does [Image #1] fail? and more"]);
    assert!(more.carries(text));
    assert!(!box_of(&["❯\u{a0}[Image #1]"]).carries(text));
    assert!(!box_of(&["❯\u{a0}why does fail?"]).carries(text));
    let long = format!("{} see [Image #1]", "word ".repeat(300));
    let seen = box_of(&["❯\u{a0}[Pasted text #2 +15 lines]"]);
    assert_eq!(seen.fill(&long), Fill::Folded);
    assert!(seen.carries(&long));
    assert!(!seen.leftover_of(&[long]));
}

#[test]
fn a_chip_the_person_pasted_never_stands_in_for_one_a_message_names() {
    for (text, row) in [
        ("[Image #1]", "❯\u{a0}[Image #5]"),
        (
            "why does [Image #1] fail?",
            "❯\u{a0}[Image #5]why does fail?",
        ),
        (
            "why does [Image #1] fail?",
            "❯\u{a0}[Image #5] [Pasted text #10 +20 lines]",
        ),
        (
            "why does [Image #1] fail?",
            "❯\u{a0}why does [Image #5] fail?",
        ),
    ] {
        let seen = box_of(&[row]);
        assert_eq!(seen.fill(text), Fill::Foreign, "{text} in {row}");
        assert!(!seen.carries(text), "{text} in {row}");
        assert!(!seen.leftover_of(&[text.into()]), "{text} in {row}");
    }
    for (earlier, row) in [
        (
            "why does [Image #1] fail?",
            "❯\u{a0}[Image #5]why does fail?",
        ),
        (
            "why does [Image #1] fail?",
            "❯\u{a0}[Image #5]why does fail",
        ),
        ("why does [Image #1] fail?", "❯\u{a0}why does fail"),
        ("see [Image #1]", "❯\u{a0}[Image #5]see"),
        ("see [Image #1]", "❯\u{a0}see"),
    ] {
        assert!(
            !box_of(&[row]).leftover_of(&[earlier.into()]),
            "{earlier} in {row}"
        );
    }
    assert_eq!(
        box_of(&["❯\u{a0}[Image #5] [Image #1]"]).fill("[Image #1]"),
        Fill::Foreign
    );
    assert!(!box_of(&["❯\u{a0}[Image #5] [Image #1]"]).leftover_of(&["[Image #1]".to_string()]));
    let earlier = [String::from("why does [Image #1] fail?")];
    assert!(box_of(&["❯\u{a0}why does [Image #1] fail?"]).leftover_of(&earlier));
    assert!(box_of(&["❯\u{a0}why does [Image #1] fa"]).leftover_of(&earlier));
    assert!(box_of(&["❯\u{a0}[Image #1]"]).leftover_of(&["[Image #1]".to_string()]));
}

#[test]
fn a_message_with_chip_words_and_picture_paths_is_never_taken_for_ours() {
    let both = "why does [Image #1] fail?\n\n'/tmp/a b.png'";
    assert_eq!(box_of(&["❯"]).fill(both), Fill::Empty);
    for row in [
        "❯\u{a0}[Image #2]why does [Image #1] fail?",
        "❯\u{a0}[Image #2]",
        "❯\u{a0}why does [Image #1] fail?",
        "❯\u{a0}why does ",
        "❯\u{a0}[Image #1]",
    ] {
        let seen = box_of(&[row]);
        assert_eq!(seen.fill(both), Fill::Foreign, "{row}");
        assert!(!seen.carries(both), "{row}");
        assert!(!seen.leftover_of(&[both.into()]), "{row}");
    }
}

#[test]
fn a_line_of_paths_is_read_piece_by_piece_the_way_claude_code_reads_it() {
    let text = "/v1/users /tmp/shot.png";
    assert_eq!(
        box_of(&["❯\u{a0}[Image #5]/v1/users"]).fill(text),
        Fill::Whole,
        "the picture is the chip and the other path stays text"
    );
    assert_eq!(
        box_of(&["❯\u{a0}/v1/users /tmp/shot.png"]).fill(text),
        Fill::Whole,
        "the picture could not be read"
    );
    for row in [
        "❯\u{a0}[Image #5]/v1/other",
        "❯\u{a0}[Image #5]/v1/users/more",
        "❯\u{a0}[Image #5] [Image #6]/v1/users",
        "❯\u{a0}/v1/users /tmp/other.png",
    ] {
        assert_eq!(box_of(&[row]).fill(text), Fill::Foreign, "{row}");
    }
    let plan = [String::from("/superpowers/plan /tmp/shot.png")];
    assert!(box_of(&["❯\u{a0}[Image #5]/superpowers/plan"]).leftover_of(&plan));
    assert!(!box_of(&["❯\u{a0}[Image #5]/superpowers/other"]).leftover_of(&plan));
    assert!(!box_of(&["❯\u{a0}[Image #5] [Image #6]/superpowers/plan"]).leftover_of(&plan));
    for text in [
        "/v1/users /tmp/shot.png now",
        "/v1/users and /tmp/shot.png",
        "/v1/users /tmp/shot.png.bak",
        "/v1/users shot.png",
    ] {
        assert_eq!(
            box_of(&["❯\u{a0}[Image #5]/v1/users"]).fill(text),
            Fill::Foreign,
            "{text}"
        );
    }
}

#[test]
fn a_long_message_with_a_picture_folds_after_its_chip() {
    let text = with_shot(&"word ".repeat(300));
    let seen = box_of(&["❯\u{a0}[Image #2] [Pasted text #3 +20 lines]"]);
    assert_eq!(seen.fill(&text), Fill::Folded);
    assert!(seen.carries(&text));
    let foreign = box_of(&["❯\u{a0}[Image #2] [Image #3] [Pasted text #4 +20 lines]"]);
    assert_eq!(foreign.fill(&text), Fill::Foreign);
}

#[test]
fn a_box_with_chips_carries_the_message_with_more_typed_beside_it() {
    let text = with_shot(WORDS);
    let more = box_of(&["❯\u{a0}[Image #48]i want click refresh it will refresh is it ok?more"]);
    assert_eq!(more.fill(&text), Fill::Foreign);
    assert!(more.carries(&text));
    let own = box_of(&["❯\u{a0}[Image #48]my own words"]);
    assert!(!own.carries(&text));
    assert!(!box_of(&["❯"]).carries(&text));
}

#[test]
fn our_own_message_left_with_its_chips_is_a_leftover_even_when_short() {
    let earlier = [with_shot("ok")];
    assert!(box_of(&["❯\u{a0}[Image #47]ok"]).leftover_of(&earlier));
    let long = [with_shot(WORDS)];
    let left = box_of(&["❯\u{a0}[Image #47]i want click refresh it will refresh is it ok?"]);
    assert!(left.leftover_of(&long));
    assert!(left.leftover_of(&[with_shot("ok"), with_shot(WORDS)]));
    assert!(!box_of(&["❯\u{a0}[Image #47]fine"]).leftover_of(&earlier));
    assert!(
        !box_of(&["❯\u{a0}[Image #47]"]).leftover_of(&[format!("'{SHOT}'")]),
        "a chip alone could be a picture the person pasted"
    );
    assert!(!box_of(&["❯\u{a0}[Image #47] [Pasted text #48 +20 lines]"]).leftover_of(&long));
}

#[test]
fn the_recorded_chip_screens_read_as_the_message_that_made_them() {
    let two = "Say only the word race.\n\n/tmp/shots/one.png /tmp/shots/two.jpg";
    let recorded = Input {
        held: vec!["❯\u{a0}[Image #5] [Image #6]Say only the word race.".to_string()],
        empty: false,
    };
    assert_eq!(recorded.fill(two), Fill::Whole);
    assert_eq!(recorded.fill("Say only the word ok."), Fill::Foreign);
}
