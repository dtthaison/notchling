mod common;

use common::notchling_fixture;
use notchling_engine::reply::exec::ScreenCheck;
use notchling_engine::reply::keys;
use notchling_engine::reply::live::AskDialog;
use notchling_engine::reply::question::{keys_for, page_keys, Choice, QuestionItem};
use notchling_engine::terminal::{ask_dialog, dialog_open, TerminalScreen};

fn screen(name: &str) -> TerminalScreen {
    let path = notchling_fixture(&format!("terminal/{name}"));
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let mut screen = TerminalScreen::new(50, 200);
    screen.feed(&bytes);
    screen
}

fn s(keys: &[&str]) -> Vec<String> {
    keys.iter().map(|k| k.to_string()).collect()
}

#[test]
fn keys_for_a_read_question() {
    let page = ask_dialog::page(&screen("question-open.bin").lines()).expect("a page");
    let item = QuestionItem {
        header: "Color".into(),
        question: page.question,
        options: page.options,
        details: page.details,
        multi_select: false,
    };
    assert_eq!(keys_for(&Choice::Option(2), &item), s(&["2"]));
    assert_eq!(
        keys_for(&Choice::Text("teal".into()), &item),
        s(&["3", "teal", "\r"])
    );
}

#[test]
fn multi_select_ticks_then_tabs_on() {
    let pets = QuestionItem {
        header: "Pets".into(),
        question: "Which pets?".into(),
        options: s(&["Cat", "Dog", "Fish"]),
        details: Vec::new(),
        multi_select: true,
    };
    assert_eq!(
        page_keys(&Choice::Options(vec![1, 3]), &pets),
        s(&["1", "3", "\t"])
    );
    assert_eq!(page_keys(&Choice::Option(2), &pets), s(&["2"]));
    assert_eq!(
        page_keys(&Choice::Text("x".into()), &pets),
        s(&["4", "x", "\r"])
    );
}

#[test]
fn bracketed() {
    assert_eq!(keys::bracketed("a"), "\u{1b}[200~a\u{1b}[201~");
    assert_eq!(
        keys::clear_input(),
        "\u{05}\u{15}\u{7f}".repeat(20) + "\u{05}\u{15}"
    );
    let pasted = keys::bracketed("hi\u{1b}[201~1\rmore\u{9b}201~x");
    assert_eq!(pasted, "\u{1b}[200~hi[201~1\rmore201~x\u{1b}[201~");
    assert_eq!(pasted.split("\u{1b}[201~").count(), 2);
}

#[test]
fn the_two_dialog_checks_agree_on_every_recording() {
    assert!(dialog_open(&screen("question-open.bin")));
    assert!(!dialog_open(&screen("plugin-0-attach.bin")));
    for name in [
        "question-open.bin",
        "plugin-0-attach.bin",
        "plugin-1-typed.bin",
        "plugin-2-enter.bin",
        "plugin-3-down.bin",
        "plugin-4-esc.bin",
    ] {
        let screen = screen(name);
        let lines = screen.lines();
        let open = dialog_open(&screen);
        assert_eq!(keys::dialog_open(&lines), open, "{name}: the reply's check");
        assert_eq!(
            AskDialog.dialog_open(&lines),
            open,
            "{name}: the executor's"
        );
    }
}

#[test]
fn the_executor_reads_pages_with_ask_dialog() {
    let lines = screen("question-open.bin").lines();
    assert!(AskDialog.shows("Favourite color?", &lines));
    assert!(!AskDialog.shows("Favourite fruit?", &lines));
    assert!(!AskDialog.is_review(&lines));
    let review = s(&[
        "Review your answers",
        "",
        " ● Favourite color?",
        "   → Red",
        "",
        "Ready to submit your answers?",
        "",
        "❯ 1. Submit answers",
        "  2. Cancel",
    ]);
    assert_eq!(AskDialog.is_review(&review), ask_dialog::is_review(&review));
    assert!(AskDialog.is_review(&review));
}
