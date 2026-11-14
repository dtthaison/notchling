use super::keys::{self, timing::*};
use super::plan::{self, Plan, PlanError, Step, Until};
use super::question::{Choice, PendingQuestion, QuestionItem};

fn s(keys: &[&str]) -> Vec<String> {
    keys.iter().map(|k| k.to_string()).collect()
}

#[test]
fn the_keys_are_the_swift_apps() {
    let clear = keys::clear_input();
    assert_eq!(clear, "\x05\x15\x7f".repeat(20) + "\x05\x15");
    assert_eq!(clear.len(), 62);
    assert!(!clear.contains('\x1b') && !clear.contains('\x03') && !clear.contains('\x0b'));
    assert_eq!(keys::bracketed("a"), "\x1b[200~a\x1b[201~");
    assert_eq!(keys::ESC, "\x1b");
    assert_eq!(keys::ENTER, "\r");
    assert_eq!(keys::TAB, "\t");
    assert_eq!(keys::UP, "\x1b[A");
    assert_eq!(keys::LEFT, "\x1b[D");
    assert_eq!(keys::RIGHT, "\x1b[C");
    assert_eq!(keys::DOWN, "\x1b[B");
    assert_eq!(keys::SUBMIT, "1");
}

#[test]
fn the_timings_are_the_swift_apps() {
    assert_eq!((ROWS, COLS), (50, 200));
    assert_eq!(KEY_PAUSE_MS, 150);
    assert_eq!((ATTACH_QUIET_MS, ATTACH_TIMEOUT_MS), (500, 15_000));
    assert_eq!((SETTLE_MS, SETTLE_SLACK_MS), (1_200, 1_000));
    assert_eq!((DETACH_QUIET_MS, DETACH_TIMEOUT_MS), (400, 3_000));
    assert_eq!(TAKE_BACK_PAUSE_MS, 300);
    assert_eq!(
        (
            POP_ALL_WAIT_MS,
            POP_ALL_QUIET_MS,
            POP_ALL_STEP_MS,
            POP_ALL_SLACK_MS
        ),
        (3_000, 300, 500, 1_000)
    );
    assert_eq!((WAIT_QUIET_MS, WAIT_STEP_MS), (300, 900));
    assert_eq!((PAGE_WAIT_MS, SUBMIT_WAIT_MS), (4_000, 4_000));
    assert_eq!((WALK_BACK_QUIET_MS, WALK_BACK_TIMEOUT_MS), (400, 1_500));
}

#[test]
fn pasted_text_cant_end_the_paste_early() {
    let pasted = keys::bracketed("hi\x1b[201~1\rmore\u{9b}201~x");
    assert_eq!(pasted, "\x1b[200~hi[201~1\rmore201~x\x1b[201~");
    assert_eq!(pasted.matches(keys::PASTE_END).count(), 1);
    assert!(pasted.ends_with(keys::PASTE_END));
    assert_eq!(keys::bracketed("a\r\nb"), "\x1b[200~a\nb\x1b[201~");
    assert_eq!(keys::bracketed("\x1b"), "\x1b[200~\x1b[201~");
    assert_eq!(
        keys::bracketed("x\x1b[200~y\x1b"),
        "\x1b[200~x[200~y\x1b[201~"
    );
    for text in ["\x1b\x1b[201~\x1b[201~", "\u{9b}201~", "a\x1b", "\x1b[201"] {
        let p = keys::bracketed(text);
        assert_eq!(p.matches('\x1b').count(), 2, "{text:?}: only the brackets");
        assert!(!p.contains('\u{9b}'));
    }
}

#[test]
fn the_dialog_markers_are_the_swift_apps() {
    assert_eq!(
        keys::DIALOG_MARKERS,
        [
            "Enter to select",
            "Esc to cancel",
            "Chat about this",
            "Do you want to proceed?",
            "Do you want to allow",
            "Submit answers"
        ]
    );
    let lines = |v: &[&str]| v.iter().map(|l| l.to_string()).collect::<Vec<_>>();
    assert!(keys::dialog_open(&lines(&[
        "x",
        " Do you want to proceed?",
        " 1. Yes",
        "",
        ""
    ])));
    assert!(!keys::dialog_open(&lines(&["> ", "  ? for shortcuts"])));
    let mut far = lines(&["Esc to cancel", "1", "2", "3", "4", "5", "6"]);
    assert!(!keys::dialog_open(&far));
    far.extend(lines(&["", "  ", ""]));
    assert!(!keys::dialog_open(&far));
    far.remove(6);
    assert!(keys::dialog_open(&far));
}

fn wait(quiet_ms: u64, timeout_ms: u64, required: bool) -> Step {
    Step::WaitQuiet {
        quiet_ms,
        timeout_ms,
        required,
    }
}

#[test]
fn the_waits_on_the_input_box_are_notchlings_own() {
    assert_eq!((INPUT_WAIT_MS, CLEAR_WAIT_MS), (5_000, 3_000));
    assert_eq!((PASTE_WAIT_MS, SENT_WAIT_MS), (10_000, 8_000));
    assert_eq!(INPUT_POLL_MS, 100);
}

#[test]
fn a_reply_is_clear_paste_enter_each_checked_on_the_input_box() {
    let plan = plan::reply("hello\nthere");
    assert_eq!(
        plan,
        Plan {
            steps: vec![
                wait(500, 15_000, true),
                Step::RefuseDialog,
                Step::AwaitInput { timeout_ms: 5_000 },
                Step::Keys(keys::clear_input()),
                Step::Pause(150),
                Step::RefuseDialog,
                Step::AwaitEmpty { timeout_ms: 3_000 },
                Step::Paste("hello\nthere".into()),
                Step::AwaitPaste { timeout_ms: 10_000 },
                Step::Submit { timeout_ms: 8_000 },
            ],
            detach: vec![wait(400, 3_000, false)],
        }
    );
    assert_eq!(
        plan.writes(),
        vec![
            keys::clear_input(),
            "\x1b[200~hello\nthere\x1b[201~".to_string(),
            "\r".into()
        ]
    );
    assert_eq!(
        plan::reply("! ls \n  -la").writes()[1],
        "\x1b[200~!ls -la\x1b[201~"
    );
}

#[test]
fn an_interrupt_is_one_esc_after_the_settle_and_only_while_working() {
    assert_eq!(plan::interrupt(false), Err(PlanError::NotWorking));
    assert_eq!(
        plan::interrupt(true),
        Ok(Plan {
            steps: vec![
                wait(500, 2_200, false),
                Step::Keys("\x1b".into()),
                Step::Pause(150)
            ],
            detach: vec![wait(400, 3_000, false)],
        })
    );
}

#[test]
fn a_take_back_clears_presses_up_reads_pop_all_and_queues_the_rest_again() {
    let plan = plan::take_back("And TWO", &["Then say ONE".to_string()]);
    assert_eq!(
        plan.steps,
        vec![
            wait(500, 2_200, false),
            Step::RefuseDialog,
            Step::Keys(keys::clear_input()),
            Step::Pause(300),
            Step::MarkQueue,
            Step::Keys("\x1b[A".into()),
            Step::AwaitPopAll {
                text: "And TWO".into(),
                timeout_ms: 3_000
            },
            Step::Keys(keys::clear_input()),
            Step::Pause(300),
            Step::EndUnlessTaken,
            Step::Paste("Then say ONE".into()),
            Step::Pause(300),
            Step::Keys("\r".into()),
            Step::Pause(300),
        ]
    );
    assert_eq!(plan.detach, vec![wait(400, 3_000, false)]);
}

fn item(question: &str, options: usize, multi: bool) -> QuestionItem {
    QuestionItem {
        header: String::new(),
        question: question.into(),
        options: (1..=options).map(|n| format!("o{n}")).collect(),
        details: vec![],
        multi_select: multi,
    }
}

#[test]
fn an_answer_checks_each_page_types_its_keys_and_submits_on_review() {
    let q = PendingQuestion {
        tool_use_id: "t".into(),
        items: vec![item("Pets?", 3, true), item("Where?", 2, false)],
    };
    let plan = plan::answer(
        &[Choice::Options(vec![1, 3]), Choice::Text("x\ny".into())],
        &q,
    );
    let keys = |k: &str| [Step::Keys(k.into()), Step::Pause(150)];
    let mut want = vec![
        wait(500, 15_000, true),
        Step::RequireDialog,
        Step::WalkBackTo {
            question: "Pets?".into(),
            tries: 3,
        },
        Step::ExpectQuestion("Pets?".into()),
    ];
    for k in ["1", "3", "\t"] {
        want.extend(keys(k));
    }
    want.push(Step::WaitFor {
        until: Until::Shows("Where?".into()),
        timeout_ms: 4_000,
    });
    want.push(Step::ExpectQuestion("Where?".into()));
    for k in ["3", "x y", "\r"] {
        want.extend(keys(k));
    }
    want.push(Step::WaitFor {
        until: Until::ReviewOrGone,
        timeout_ms: 4_000,
    });
    want.push(Step::SubmitIfReview { timeout_ms: 4_000 });
    assert_eq!(plan.steps, want);
    assert_eq!(plan.detach, vec![wait(400, 3_000, false)]);
    assert_eq!(plan.writes(), s(&["1", "3", "\t", "3", "x y", "\r", "1"]));
    let none = PendingQuestion {
        tool_use_id: "t".into(),
        items: vec![],
    };
    assert!(plan::answer(&[], &none).is_empty());
}

fn swift_root() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../legacy/macos/Sources")
}

fn swift_from(root: &std::path::Path, rel: &str) -> String {
    let path = root.join(rel);
    std::fs::read_to_string(&path).unwrap_or_else(|error| {
        panic!(
            "{} is missing ({error}). This test compares the Rust reply code with the Swift app \
             while both exist; when the Swift app is removed on purpose, delete \
             `the_swift_source_still_says_the_same` and the Swift comparison in this file's docs.",
            path.display()
        )
    })
}

#[test]
#[should_panic(expected = "is missing")]
fn a_missing_swift_source_fails_instead_of_passing_without_comparing() {
    swift_from(
        std::path::Path::new("/nonexistent/legacy/macos/Sources"),
        "Notchling/Agents/ReplySender.swift",
    );
}

fn swift(rel: &str) -> String {
    swift_from(&swift_root(), rel)
}

fn has(source: &str, file: &str, snippets: &[&str]) {
    for snippet in snippets {
        assert!(
            source.contains(snippet),
            "{file} no longer says {snippet:?}: change the Rust value to match, record the fixtures again"
        );
    }
}

#[test]
fn the_swift_source_still_says_the_same() {
    let sender = swift("Notchling/Agents/ReplySender.swift");
    has(
        &sender,
        "ReplySender.swift",
        &[
            r#"static let clearInput = String(repeating: "\u{05}\u{15}\u{7F}", count: 20) + "\u{05}\u{15}""#,
            r#"try type([clearInput, ConsoleCore.TerminalKeys.bracketed(text), "\r"], toJob: jobID, refuseDialog: true)"#,
            r#"static let dialogMarkers = ["Enter to select", "Esc to cancel", "Chat about this", "Do you want to proceed?","#,
            r#""Do you want to allow", "Submit answers"]"#,
            "drawn.suffix(6)",
            r#"try type(["\u{1b}"], toJob: jobID, settle: 1.2)"#,
            "write(master, chunk)\n                Thread.sleep(forTimeInterval: 0.15)",
            "try attached(jobID, refuseDialog: true, settle: 1.2)",
            "write(master, clearInput)\n            Thread.sleep(forTimeInterval: 0.3)",
            r#"write(master, "\u{1b}[A")"#,
            "let deadline = Date().addingTimeInterval(3)",
            "waitForQuiet(master, quiet: 0.3, timeout: 0.5, into: screen)",
            "since: pressed.addingTimeInterval(-1)",
            "write(master, ConsoleCore.TerminalKeys.bracketed(text))\n                Thread.sleep(forTimeInterval: 0.3)\n                write(master, \"\\r\")\n                Thread.sleep(forTimeInterval: 0.3)",
            "winsize(ws_row: 50, ws_col: 200",
            "waitForQuiet(master, quiet: 0.5, timeout: settle + 1, into: screen)",
            "guard waitForQuiet(master, quiet: 0.5, timeout: 15, into: screen) else { throw Failure.timeout }",
            "if refuseDialog, dialogOpen(screen) { throw Failure.dialog }",
            "_ = waitForQuiet(master, quiet: 0.4, timeout: 3)\n    }",
            r#"return ["\(item.options.count + 1)", t.replacingOccurrences(of: "\n", with: " "), "\r"]"#,
            r#"return numbers.map(String.init) + Array(repeating: "\t", count: item.options.count + 1) + ["\r"]"#,
            r#"if question.items.count > 1 || question.items.contains(where: \.multiSelect) { chunks.append("1") }"#,
            r#"answers[item.question] = labels.joined(separator: ", ")"#,
        ],
    );
    let dialog = swift("Notchling/Agents/ReplySender+Dialog.swift");
    has(
        &dialog,
        "ReplySender+Dialog.swift",
        &[
            r#"private static let arrowLeft = "\u{1b}[D""#,
            "_ = waitForQuiet(master, quiet: 0.3, timeout: 0.9, into: screen)",
            r#"case .options(let numbers): return numbers.map(String.init) + ["\t"]"#,
            "guard dialogOpen(screen) else { throw Failure.questionGone }",
            "guard tries <= items.count else { throw Failure.questionMismatch }",
            "_ = waitForQuiet(master, quiet: 0.4, timeout: 1.5, into: screen)",
            "guard AskDialog.shows(item.question, in: screen.lines) else { throw Failure.questionMismatch }",
            "write(master, key)\n                    Thread.sleep(forTimeInterval: 0.15)",
            "let moved = wait(master, screen, timeout: 4)",
            "return AskDialog.isReview(s.lines) || !dialogOpen(s)",
            r#"write(master, "1")
                wait(master, screen, timeout: 4) { !dialogOpen($0) }"#,
        ],
    );
    let paste = swift("ConsoleCore/MessageSending.swift");
    has(
        &paste,
        "MessageSending.swift",
        &[
            r#"let flat = text.replacingOccurrences(of: "\r\n", with: "\n")"#,
            r#"$0 != "\u{1b}" && $0 != "\u{9b}""#,
            r#"return "\u{1b}[200~" + safe + "\u{1b}[201~""#,
        ],
    );
}
