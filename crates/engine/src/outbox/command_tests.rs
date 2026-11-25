use super::*;

#[test]
fn a_typed_command_is_told_after_the_cleaning_the_paste_gets() {
    for text in [
        "\u{200B}/clear",
        "\u{FEFF}!git push",
        "\u{200B} \u{FEFF}/compact",
        "\r\n/clear",
        "  \u{200B}!ls",
        "\u{2060}\u{2063}/model opus",
    ] {
        assert!(typed_command(Tool::Claude, text), "{text:?}");
    }
    for text in [
        "say \u{200B}/clear",
        "\u{200B}",
        "\u{200B}hello",
        "\u{FEFF}",
    ] {
        assert!(!typed_command(Tool::Claude, text), "{text:?}");
    }
    assert!(!typed_command(Tool::Codex, "\u{200B}/clear"));
}

#[test]
fn a_message_that_is_a_dropped_picture_is_not_a_command_but_other_paths_are() {
    for text in [
        "/Users/me/shot.png",
        "  /Users/me/shot.png",
        "/tmp/a.png /tmp/b.JPG",
        "/tmp/a.png\n/tmp/b.png",
        "/tmp/a.png\ncheck this",
        "/tmp/a.png /tmp/err.log",
        "\u{200B}/Users/me/shot.png",
    ] {
        assert!(!typed_command(Tool::Claude, text), "{text:?}");
    }
    for text in [
        "/tmp/err.log",
        "/tmp/err.log check this",
        "/Users/me/shot.png check this",
        "/tmp/err.log /tmp/a.png",
        "/clear",
        "/model opus",
        "!/tmp/a.png",
        "!cat /tmp/a.png",
    ] {
        assert!(typed_command(Tool::Claude, text), "{text:?}");
    }
}

#[test]
fn a_short_draft_ending_in_an_ellipsis_is_not_our_cut_short_command() {
    assert!(!holds_command(Tool::Claude, "/rev…", "/review"));
    assert!(!holds_command(Tool::Claude, "rev…", "/review"));
}

#[test]
fn a_command_cut_at_the_shown_limit_is_still_ours() {
    let text = format!("/review {}", "x".repeat(1_500));
    let cut: String = text.chars().take(BOX_SHOWN_MAX).chain(['…']).collect();
    assert!(holds_command(Tool::Claude, &cut, &text));
    let short: String = text.chars().take(BOX_SHOWN_MAX - 1).chain(['…']).collect();
    assert!(!holds_command(Tool::Claude, &short, &text));
}
