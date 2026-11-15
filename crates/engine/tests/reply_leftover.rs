use notchling_engine::reply::exec::{
    execute_clearing, Clearing, Failure, Markers, NoQueueLog, Outcome, Pty,
};
use notchling_engine::reply::fake::FakePrompt;
use notchling_engine::reply::keys;
use notchling_engine::reply::plan;
use notchling_engine::terminal::input_box::{self, Input};
use std::path::Path;

type Shot = (Vec<String>, Option<(usize, usize)>);

fn shot(name: &str) -> Shot {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/input-box")
        .join(name);
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let mut lines = text.lines();
    let head: Vec<&str> = lines.next().unwrap().split(' ').collect();
    let cursor = (head[1].parse().unwrap(), head[2].parse().unwrap());
    let shown = head[3] == "true";
    (lines.map(String::from).collect(), shown.then_some(cursor))
}

fn read(name: &str) -> Input {
    let (lines, cursor) = shot(name);
    input_box::read(&lines, cursor).unwrap_or_else(|| panic!("{name}: no input box"))
}

fn prompt_of(name: &str) -> FakePrompt {
    let (lines, _) = shot(name);
    let rows: Vec<&str> = lines.iter().map(String::as_str).collect();
    FakePrompt::new(&rows)
}

fn send(pty: &mut FakePrompt, text: &str, clearing: &Clearing) -> Result<Outcome, Failure> {
    execute_clearing(
        &plan::reply(text),
        pty,
        &Markers,
        &mut NoQueueLog,
        &mut Vec::new(),
        clearing,
    )
}

fn typed(text: &str) -> Vec<String> {
    vec![
        keys::clear_input(),
        keys::bracketed(text),
        keys::ENTER.to_string(),
    ]
}

#[test]
fn the_box_text_is_what_a_person_would_read_in_it() {
    let one = read("typed.txt");
    assert_eq!(one.typed(), "Reply with just the word ok.");
    let lines = read("three-lines.txt");
    assert_eq!(lines.typed().lines().count(), 3, "{:?}", lines.typed());
    assert!(!lines.typed().starts_with('❯'));
    assert!(read("shell-mode.txt").typed().starts_with("ls -la"));
    assert_eq!(
        read("new-job-hint.txt").typed(),
        "Try \"how do I log an error?\""
    );
}

#[test]
fn only_a_box_with_typed_text_has_text_to_show() {
    let (lines, cursor) = shot("typed.txt");
    assert_eq!(
        input_box::typed_text(&lines, cursor).as_deref(),
        Some("Reply with just the word ok.")
    );
    for name in [
        "new-job-hint.txt",
        "queued-hint.txt",
        "working-after-send.txt",
    ] {
        let (lines, cursor) = shot(name);
        assert_eq!(input_box::typed_text(&lines, cursor), None, "{name}");
    }
}

#[test]
fn a_recorded_leftover_of_ours_is_told_from_one_that_is_not() {
    let box_of = read("typed.txt");
    let ours = ["Reply with just the word ok. Then stop.".to_string()];
    assert!(box_of.leftover_of(&ours), "a start of what we typed");
    assert!(box_of.leftover_of(&["Reply with just the word ok.".to_string()]));
    assert!(!box_of.leftover_of(&["Say something else entirely".to_string()]));
    assert!(!box_of.leftover_of(&[]));
}

#[test]
fn pictures_a_session_turned_into_chips_do_not_hide_the_words_we_typed() {
    let left = read("pictures-left-unsent.txt");
    assert_eq!(
        left.held,
        ["❯\u{a0}[Image #5] [Image #6]Say only the word race."]
    );
    let ours = ["Say only the word race.\n\n/tmp/shots/one.png /tmp/shots/two.jpg".to_string()];
    assert!(left.leftover_of(&ours));
    assert!(!left.leftover_of(&["Say only the word ok.".to_string()]));
}

#[test]
fn a_folded_paste_and_an_empty_box_are_never_a_leftover() {
    let folded = Input {
        held: vec!["❯\u{a0}[Pasted text #7 +39 lines]".to_string()],
        empty: false,
    };
    assert!(!folded.leftover_of(&["[Pasted text #7 +39 lines]".to_string()]));
    let hint = read("new-job-hint.txt");
    assert!(!hint.leftover_of(&["Try how do I log an error".to_string()]));
}

#[test]
fn a_recorded_screen_with_our_own_unsent_message_is_cleared_and_the_next_one_goes_once() {
    let mut pty = prompt_of("pictures-left-unsent.txt");
    let clearing = Clearing {
        earlier: vec!["Say only the word race.\n\n/tmp/shots/one.png /tmp/shots/two.jpg".into()],
        any: false,
    };
    let out = send(&mut pty, "What changed?", &clearing);
    assert_eq!(out, Ok(Outcome { taken: false }));
    assert_eq!(pty.sent, ["What changed?"]);
    assert_eq!(pty.writes, typed("What changed?"));
}

#[test]
fn a_recorded_screen_with_a_draft_nobody_sent_is_refused_until_the_person_asks() {
    let mut pty = prompt_of("typed.txt");
    let out = send(&mut pty, "What changed?", &Clearing::default());
    assert_eq!(out, Err(Failure::NotCleared));
    assert!(pty.writes.is_empty());
    assert_eq!(pty.held(), Some("Reply with just the word ok."));

    let asked = Clearing {
        earlier: Vec::new(),
        any: true,
    };
    let out = send(&mut pty, "What changed?", &asked);
    assert_eq!(out, Ok(Outcome { taken: false }));
    assert_eq!(pty.sent, ["What changed?"]);
    assert_eq!(pty.writes, typed("What changed?"));
    assert!(pty.now_ms() > 0);
}
