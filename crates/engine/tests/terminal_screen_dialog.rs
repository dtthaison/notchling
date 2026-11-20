mod common;

use common::notchling_fixture;
use notchling_engine::terminal::screen_dialog::{self, DialogPick, Shown};
use notchling_engine::terminal::{dialog_open, TerminalScreen};
use std::path::Path;

fn screen_of(bytes: &[u8]) -> TerminalScreen {
    let mut screen = TerminalScreen::new(50, 200);
    screen.feed(bytes);
    screen
}

fn made_up(name: &str) -> TerminalScreen {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/dialog-made-up")
        .join(name);
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    screen_of(text.replace('\n', "\r\n").as_bytes())
}

fn recorded(name: &str) -> TerminalScreen {
    let path = notchling_fixture(&format!("terminal/{name}"));
    screen_of(&std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display())))
}

fn labels(dialog: &screen_dialog::ScreenDialog) -> Vec<(u64, &str, bool)> {
    dialog
        .options
        .iter()
        .map(|o| (o.number, o.label.as_str(), o.selected))
        .collect()
}

#[test]
fn the_switch_model_confirmation_is_read_whole() {
    let screen = made_up("switch-model.txt");
    assert!(dialog_open(&screen), "the reply path refuses it too");
    let dialog = screen_dialog::read(&screen.lines()).expect("a dialog");
    assert_eq!(dialog.title, "Switch model?");
    assert_eq!(
        dialog.body,
        [
            "Your next response will be slower and use more tokens",
            "This conversation is cached for the current model. Switching to Opus 5.5 means the full history gets re-read."
        ]
    );
    assert_eq!(
        labels(&dialog),
        [
            (1, "Yes, switch to Opus 5.5", true),
            (2, "No, go back", false)
        ]
    );
}

#[test]
fn the_marker_follows_the_selected_option() {
    let dialog =
        screen_dialog::read(&made_up("switch-model-second.txt").lines()).expect("a dialog");
    assert_eq!(
        labels(&dialog),
        [
            (1, "Yes, switch to Opus 5.5", false),
            (2, "No, go back", true)
        ]
    );
}

#[test]
fn a_dialog_with_no_rule_is_still_read() {
    let screen = made_up("switch-model-borderless.txt");
    let dialog = screen_dialog::read(&screen.lines()).expect("a dialog");
    assert_eq!(dialog.title, "Switch model?");
    assert_eq!(dialog.body.len(), 2);
    assert_eq!(dialog.options.len(), 2);
}

#[test]
fn wrapped_body_and_option_rows_are_joined() {
    let dialog =
        screen_dialog::read(&made_up("switch-model-wrapped.txt").lines()).expect("a dialog");
    assert_eq!(dialog.title, "Switch model?");
    assert_eq!(
        dialog.body[1],
        "This conversation is cached for the current model. Switching to Opus 5.5 means the full history gets re-read."
    );
    assert_eq!(
        dialog.options[0].label,
        "Yes, switch to Opus 5.5 and keep the conversation as it is"
    );
    assert_eq!(dialog.options[1].label, "No, go back");
}

#[test]
fn a_numbered_list_in_the_chat_is_no_dialog() {
    assert_eq!(
        screen_dialog::read(&made_up("numbered-list-in-chat.txt").lines()),
        None
    );
}

#[test]
fn recorded_screens_without_a_confirmation_read_as_none() {
    for name in [
        "question-open.bin",
        "attach-idle.bin",
        "plugin-0-attach.bin",
    ] {
        assert_eq!(screen_dialog::read(&recorded(name).lines()), None, "{name}");
    }
}

#[test]
fn a_pick_matches_only_the_dialog_it_was_made_from() {
    let first = screen_dialog::read(&made_up("switch-model.txt").lines()).unwrap();
    let second = screen_dialog::read(&made_up("switch-model-second.txt").lines()).unwrap();
    assert!(
        first.same_as(&second),
        "the marker moving is the same dialog"
    );
    let pick = DialogPick {
        title: "Switch model?".into(),
        number: 1,
        label: "Yes, switch to Opus 5.5".into(),
    };
    assert!(first.offers(&pick));
    let other = DialogPick {
        label: "Yes, switch to Fable 5.1".into(),
        ..pick.clone()
    };
    assert!(!first.offers(&other));
    let renamed = DialogPick {
        title: "Switch effort?".into(),
        ..pick
    };
    assert!(!first.offers(&renamed));
}

fn chrome(name: &str) -> TerminalScreen {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/chrome")
        .join(name);
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    screen_of(text.replace('\n', "\r\n").as_bytes())
}

#[test]
fn every_switch_model_screen_is_shown_for_dismiss() {
    for name in [
        "switch-model.txt",
        "switch-model-second.txt",
        "switch-model-wrapped.txt",
        "switch-model-borderless.txt",
    ] {
        let shown = screen_dialog::shown(&made_up(name).lines());
        assert!(matches!(shown, Some(Shown::Screen(_))), "{name}: {shown:?}");
    }
    let permission = screen_dialog::shown(&made_up("permission-prompt.txt").lines());
    assert!(matches!(permission, Some(Shown::Permission(_))));
}

#[test]
fn a_menu_whose_last_hint_offers_esc_is_shown() {
    let shown = screen_dialog::shown(&chrome("menu-on.txt").lines());
    assert!(matches!(shown, Some(Shown::Hint(_))), "{shown:?}");
}

#[test]
fn output_that_only_looks_like_a_dialog_is_not_shown() {
    for name in [
        "blockquote-list.txt",
        "list-then-press-enter.txt",
        "list-marked-then-prose.txt",
        "list-streaming-no-box.txt",
        "list-in-output.txt",
        "list-streaming.txt",
        "numbered-list-in-chat.txt",
    ] {
        assert_eq!(screen_dialog::shown(&made_up(name).lines()), None, "{name}");
    }
    assert_eq!(
        screen_dialog::shown(&recorded("attach-idle.bin").lines()),
        None
    );
}
