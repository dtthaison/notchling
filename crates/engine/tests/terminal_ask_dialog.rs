//! Ports of `AskDialogTests`: a question read off a background job's terminal. The two key tests
//! (`testKeysForAReadQuestion`, `testMultiSelectTicksThenTabsOn`) test `ReplySender`'s key planner,
//! which is E3b's reply planner, not the screen reader.

mod common;

use common::notchling_fixture;
use notchling_engine::terminal::ask_dialog;
use notchling_engine::terminal::TerminalScreen;

fn rule() -> String {
    "─".repeat(120)
}

const FOOTER: &str = "Enter to select · Tab/Arrow keys to navigate · Esc to cancel";

fn fixture_lines() -> Vec<String> {
    let path = notchling_fixture("terminal/question-open.bin");
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let mut screen = TerminalScreen::new(50, 200);
    screen.feed(&bytes);
    screen.lines()
}

fn owned(lines: &[&str]) -> Vec<String> {
    lines.iter().map(|s| s.to_string()).collect()
}

/// The first page of a real two-question dialog (Claude Code 2.1.318).
#[test]
fn reads_the_recorded_dialog() {
    let page = ask_dialog::page(&fixture_lines()).expect("a page");
    assert_eq!(page.question, "Favourite color?");
    assert_eq!(page.options, ["Red", "Blue"]);
    assert_eq!(page.details, ["Red", "Blue"]);
    assert_eq!(page.tabs, ["Color", "Fruit"]);
    assert!(!page.multi_select);
    assert!(!page.on_review);
}

/// A single-question dialog read from a live job (2.1.319): no tab row, the header is a chip over the
/// question, and the question has brackets in it.
#[test]
fn reads_the_recorded_single_question_dialog() {
    let rule = rule();
    let page = ask_dialog::page(&owned(&[
        " ▐▛███▛█   Claude Code v2.1.319",
        "",
        "❯ Call the AskUserQuestion tool exactly once with one question: 'Pick (a size)?' (options Small, Large). Then stop. Do",
        "  not read or change any files.",
        &rule,
        " ☐ Size",
        "",
        "Pick (a size)?",
        "",
        "❯ 1. Small",
        "     Choose the small size.",
        "  2. Large",
        "     Choose the large size.",
        "  3. Type something.",
        &rule,
        "  4. Chat about this",
        "",
        "Enter to select · ↑/↓ to navigate · Esc to cancel",
        "",
        "──────────────────────────── askprobe2 ─",
    ]))
    .expect("a page");
    assert_eq!(page.question, "Pick (a size)?");
    assert_eq!(page.header, "Size");
    assert_eq!(page.options, ["Small", "Large"]);
    assert_eq!(
        page.details,
        ["Choose the small size.", "Choose the large size."]
    );
    assert!(page.tabs.is_empty());
    assert!(ask_dialog::shows(
        "Pick (a size)?",
        &owned(&[
            &rule,
            " ☐ Size",
            "Pick (a size)?",
            "❯ 1. Small",
            "  2. Type something.",
            &rule,
            "  3. Chat about this",
            FOOTER
        ])
    ));
}

#[test]
fn second_question_page_with_answered_tab() {
    let rule = rule();
    let page = ask_dialog::page(&owned(&[
        "❯ Call the AskUserQuestion tool exactly once with two questions",
        &rule,
        "←  ☒ Color  ☐ Fruit  ✔ Submit  →",
        "",
        "Favourite fruit?",
        "",
        "❯ 1. Apple",
        "     Apple",
        "  2. Pear",
        "     Pear",
        "  3. Type something.",
        &rule,
        "  4. Chat about this",
        "",
        FOOTER,
    ]))
    .expect("a page");
    assert_eq!(page.question, "Favourite fruit?");
    assert_eq!(page.options, ["Apple", "Pear"]);
    assert_eq!(
        page.tabs,
        ["Color", "Fruit"],
        "an answered tab's mark is not part of its name"
    );
}

#[test]
fn single_question_wraps_and_has_no_tabs() {
    let rule = rule();
    let page = ask_dialog::page(&owned(&[
        "❯ Ask me how to deploy",
        "",
        "Which environment should the release go to first, and should the database migration run",
        "before or after the new build is live?",
        "",
        "❯ 1. Staging",
        "     Safest: nothing real is touched",
        "  2. Production",
        "  3. Type something.",
        &rule,
        "  4. Chat about this",
        FOOTER,
    ]))
    .expect("a page");
    assert_eq!(page.question, "Which environment should the release go to first, and should the database migration run before or after the new build is live?");
    assert_eq!(page.options, ["Staging", "Production"]);
    assert_eq!(page.details, ["Safest: nothing real is touched", ""]);
    assert!(page.tabs.is_empty());
}

/// A multi-select page read from a live job (2.1.319): the checkbox follows the number, "Type
/// something" has one too, and a Next row follows the list.
#[test]
fn checkboxes_mean_multi_select() {
    let rule = rule();
    let page = ask_dialog::page(&owned(&[
        &rule,
        "←  ☐ Pets  ☐ Where  ✔ Submit  →",
        "",
        "Which pets?",
        "",
        "❯ 1. [ ] Cat",
        "         A cat",
        "  2. [x] Dog",
        "         A dog",
        "  3. [ ] Fish",
        "         A fish",
        "  4. [ ] Type something",
        "     Next",
        &rule,
        "  5. Chat about this",
        "",
        "Enter to select · Tab/Arrow keys to navigate · Esc to cancel",
    ]))
    .expect("a page");
    assert!(page.multi_select);
    assert_eq!(page.question, "Which pets?");
    assert_eq!(page.options, ["Cat", "Dog", "Fish"]);
    assert_eq!(page.details, ["A cat", "A dog", "A fish"]);
    assert_eq!(page.tabs, ["Pets", "Where"]);
}

#[test]
fn the_review_screen_is_not_a_question() {
    let rule = rule();
    let lines = owned(&[
        &rule,
        "←  ☒ Color  ☒ Fruit  ✔ Submit  →",
        "Review your answers",
        "",
        " ● Favourite color? → Red",
        "",
        "Ready to submit your answers?",
        "❯ 1. Submit answers",
        "  2. Cancel",
        FOOTER,
    ]);
    assert!(ask_dialog::is_review(&lines));
    let page = ask_dialog::page(&lines).expect("a page");
    assert!(page.on_review);
    assert!(page.options.is_empty());
}

/// A permission dialog is numbered and ends in "Esc to cancel" too, but it isn't an AskUserQuestion.
#[test]
fn permission_dialog_is_not_a_question() {
    let rule = rule();
    assert_eq!(
        ask_dialog::page(&owned(&[
            &rule,
            "Bash command",
            "  rm -rf build",
            "Do you want to proceed?",
            "❯ 1. Yes",
            "  2. Yes, and don't ask again for rm commands",
            "  3. No, and tell Claude what to do differently",
            "Esc to cancel",
        ])),
        None
    );
}

#[test]
fn nothing_open() {
    let rule = rule();
    assert_eq!(
        ask_dialog::page(&owned(&["❯ ", &rule, "  ? for shortcuts"])),
        None
    );
}

#[test]
fn shows_matches_the_dialogs_question_only() {
    let rule = rule();
    let lines = fixture_lines();
    assert!(ask_dialog::shows("Favourite color?", &lines));
    // The recorded prompt line above the dialog names this question too; that is not the dialog asking it.
    assert!(lines.join(" ").contains("Favourite fruit?"));
    assert!(!ask_dialog::shows("Favourite fruit?", &lines));
    let wrapped = owned(&[
        &rule,
        "Which environment should",
        "the release go   to first, and",
        "❯ 1. Staging",
        "  2. Type something.",
        &rule,
        "  3. Chat about this",
        FOOTER,
    ]);
    assert!(ask_dialog::shows(
        "Which environment should the release go\nto first",
        &wrapped
    ));
    assert!(!ask_dialog::shows(
        "Which environment",
        &owned(&[&rule, "Review your answers", "❯ 1. Submit answers", FOOTER])
    ));
}

/// The chip over a single question is at most 40 characters; a longer first row is the question.
#[test]
fn a_long_first_row_is_not_a_chip() {
    let rule = rule();
    let long = format!("☐ {}", "x".repeat(41));
    let page = ask_dialog::page(&owned(&[
        &rule,
        &long,
        "Pick?",
        "❯ 1. A",
        "  2. Type something.",
        &rule,
        "  3. Chat about this",
        FOOTER,
    ]))
    .expect("a page");
    assert_eq!(page.header, "");
    assert_eq!(page.question, format!("{long} Pick?"));
}
