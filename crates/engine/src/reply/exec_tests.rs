//! The executor against a fake terminal: what it writes, when, and where it stops.

use super::exec::{execute, Failure, Markers, NoQueueLog, Outcome, Pty, QueueLog, ScreenCheck};
use super::keys;
use super::plan;
use super::question::{Choice, PendingQuestion, QuestionItem};
use std::time::Duration;

#[derive(Clone, Debug, PartialEq, Eq)]
enum Ev {
    Wait(u64, u64),
    Sleep(u64),
    Write(String),
}

type React = Box<dyn FnMut(&str, &mut Vec<String>)>;

/// A terminal whose screen is a list of lines, changed by `react` on each write. Time moves only
/// when the executor waits or sleeps.
struct Fake {
    clock: u64,
    events: Vec<Ev>,
    screen: Vec<String>,
    react: React,
    draws: bool,
    broken: bool,
}

impl Fake {
    fn new(screen: &[&str]) -> Fake {
        Fake {
            clock: 0,
            events: Vec::new(),
            screen: lines(screen),
            react: Box::new(|_, _| {}),
            draws: true,
            broken: false,
        }
    }

    fn reacting(mut self, react: impl FnMut(&str, &mut Vec<String>) + 'static) -> Fake {
        self.react = Box::new(react);
        self
    }

    fn writes(&self) -> Vec<String> {
        self.events
            .iter()
            .filter_map(|e| match e {
                Ev::Write(k) => Some(k.clone()),
                _ => None,
            })
            .collect()
    }
}

impl Pty for Fake {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<()> {
        if self.broken {
            return Err(std::io::Error::other("the pty closed"));
        }
        let keys = String::from_utf8_lossy(bytes).into_owned();
        (self.react)(&keys, &mut self.screen);
        self.events.push(Ev::Write(keys));
        Ok(())
    }
    fn wait_quiet(&mut self, quiet: Duration, timeout: Duration) -> bool {
        let (q, t) = (quiet.as_millis() as u64, timeout.as_millis() as u64);
        self.events.push(Ev::Wait(q, t));
        self.clock += if self.draws { q.min(t) } else { t };
        self.draws
    }
    fn screen(&mut self) -> Vec<String> {
        self.screen.clone()
    }
    fn sleep(&mut self, duration: Duration) {
        let ms = duration.as_millis() as u64;
        self.events.push(Ev::Sleep(ms));
        self.clock += ms;
    }
    fn now_ms(&self) -> u64 {
        self.clock
    }
}

fn lines(v: &[&str]) -> Vec<String> {
    v.iter().map(|l| l.to_string()).collect()
}

const PROMPT: &[&str] = &["> ", "  ? for shortcuts"];
const PERMISSION: &[&str] = &[
    "Bash command",
    " Do you want to proceed?",
    " 1. Yes",
    " 2. No",
];

#[test]
fn a_reply_goes_out_as_clear_paste_enter_with_the_pauses() {
    let mut pty = Fake::new(PROMPT);
    let out = execute(&plan::reply("hi"), &mut pty, &Markers, &mut NoQueueLog);
    assert_eq!(out, Ok(Outcome { taken: false }));
    assert_eq!(
        pty.events,
        vec![
            Ev::Wait(500, 15_000),
            Ev::Write(keys::clear_input()),
            Ev::Sleep(150),
            Ev::Write("\x1b[200~hi\x1b[201~".into()),
            Ev::Sleep(150),
            Ev::Write("\r".into()),
            Ev::Sleep(150),
            Ev::Wait(400, 3_000),
        ]
    );
}

#[test]
fn nothing_is_typed_into_an_open_dialog() {
    let mut pty = Fake::new(PERMISSION);
    let out = execute(&plan::reply("1"), &mut pty, &Markers, &mut NoQueueLog);
    assert_eq!(out, Err(Failure::Dialog));
    assert!(pty.writes().is_empty());
}

#[test]
fn a_dialog_that_opens_mid_type_gets_no_text_and_no_enter() {
    // The permission dialog draws right after the clear keys go in.
    let mut pty = Fake::new(PROMPT).reacting(|keys, screen| {
        if keys == keys::clear_input() {
            *screen = lines(PERMISSION);
        }
    });
    let out = execute(&plan::reply("1"), &mut pty, &Markers, &mut NoQueueLog);
    assert_eq!(out, Err(Failure::Dialog));
    assert_eq!(pty.writes(), vec![keys::clear_input()]);
}

#[test]
fn a_session_that_never_draws_is_a_timeout_with_nothing_typed() {
    let mut pty = Fake::new(&[]);
    pty.draws = false;
    let out = execute(&plan::reply("hi"), &mut pty, &Markers, &mut NoQueueLog);
    assert_eq!(out, Err(Failure::Timeout));
    assert!(pty.writes().is_empty());
}

#[test]
fn a_write_that_fails_stops_the_plan() {
    let mut pty = Fake::new(PROMPT);
    pty.broken = true;
    let out = execute(&plan::reply("hi"), &mut pty, &Markers, &mut NoQueueLog);
    assert!(matches!(out, Err(Failure::Pty(_))), "{out:?}");
    assert!(
        !pty.events.contains(&Ev::Wait(400, 3_000)),
        "no detach wait"
    );
}

#[test]
fn an_interrupt_is_one_esc_even_on_a_busy_screen() {
    let mut pty = Fake::new(&["✻ Thinking… (esc to interrupt)"]);
    pty.draws = false; // a busy screen never goes quiet: the settle lets it go ahead
    let out = execute(
        &plan::interrupt(true).unwrap(),
        &mut pty,
        &Markers,
        &mut NoQueueLog,
    );
    assert_eq!(out, Ok(Outcome::default()));
    assert_eq!(pty.writes(), vec!["\x1b".to_string()]);
    assert_eq!(pty.events[0], Ev::Wait(500, 2_200));
}

/// A queue log that sees the `popAll` on its `after`-th look after the mark.
struct Log {
    marked: bool,
    looks: usize,
    after: Option<usize>,
}

impl QueueLog for Log {
    fn mark(&mut self) {
        self.marked = true;
    }
    fn popped_all(&mut self, text: &str) -> bool {
        assert!(self.marked, "looked before Up was marked");
        assert_eq!(text, "latest");
        self.looks += 1;
        self.after.is_some_and(|n| self.looks >= n)
    }
}

#[test]
fn a_take_back_that_pops_queues_the_older_ones_again() {
    let mut pty = Fake::new(PROMPT);
    let mut log = Log {
        marked: false,
        looks: 0,
        after: Some(2),
    };
    let keep = vec!["older one".to_string(), "older two".to_string()];
    let out = execute(
        &plan::take_back("latest", &keep),
        &mut pty,
        &Markers,
        &mut log,
    );
    assert_eq!(out, Ok(Outcome { taken: true }));
    assert_eq!(
        pty.writes(),
        vec![
            keys::clear_input(),
            "\x1b[A".into(),
            keys::clear_input(),
            "\x1b[200~older one\x1b[201~".into(),
            "\r".into(),
            "\x1b[200~older two\x1b[201~".into(),
            "\r".into(),
        ]
    );
    assert_eq!(log.looks, 2);
    assert_eq!(pty.events.last(), Some(&Ev::Wait(400, 3_000)));
}

#[test]
fn a_take_back_with_nothing_popped_only_clears_what_up_brought_back() {
    let mut pty = Fake::new(PROMPT);
    let mut log = Log {
        marked: false,
        looks: 0,
        after: None,
    };
    let out = execute(
        &plan::take_back("latest", &["older".to_string()]),
        &mut pty,
        &Markers,
        &mut log,
    );
    assert_eq!(out, Ok(Outcome { taken: false }));
    assert_eq!(
        pty.writes(),
        vec![keys::clear_input(), "\x1b[A".into(), keys::clear_input()]
    );
    // It looked for three seconds, then still detached.
    assert_eq!(log.looks, 10);
    assert_eq!(pty.events.last(), Some(&Ev::Wait(400, 3_000)));
}

/// AskDialog's part, faked: a page line `Q: <question>`, the review screen `Review your answers`.
struct Pages;

impl ScreenCheck for Pages {
    fn shows(&self, question: &str, lines: &[String]) -> bool {
        lines.iter().any(|l| l == &format!("Q: {question}"))
    }
    fn is_review(&self, lines: &[String]) -> bool {
        lines.iter().any(|l| l.contains("Review your answers"))
    }
}

fn page(question: &str) -> Vec<String> {
    lines(&[&format!("Q: {question}"), "Enter to select · Esc to cancel"])
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

fn two() -> PendingQuestion {
    PendingQuestion {
        tool_use_id: "t".into(),
        items: vec![item("Pets?", 3, true), item("Where?", 2, false)],
    }
}

/// The dialog of `two()`: Tab leaves Pets, a digit leaves Where, `1` submits the review.
fn dialog(start: &str) -> Fake {
    let mut fake = Fake::new(&[]);
    fake.screen = page(start);
    fake.reacting(|keys, screen| {
        let on = |q: &str| screen.iter().any(|l| l == &format!("Q: {q}"));
        if keys == "\t" && on("Pets?") {
            *screen = page("Where?");
        } else if keys == "2" && on("Where?") {
            *screen = lines(&["Review your answers", "Submit answers"]);
        } else if keys == "1" && screen.iter().any(|l| l.contains("Review")) {
            *screen = lines(PROMPT);
        } else if keys == keys::LEFT && on("Where?") {
            *screen = page("Pets?");
        }
    })
}

#[test]
fn an_answer_types_each_page_and_submits_the_review() {
    let mut pty = dialog("Pets?");
    let plan = plan::answer(&[Choice::Options(vec![1, 3]), Choice::Option(2)], &two());
    let out = execute(&plan, &mut pty, &Pages, &mut NoQueueLog);
    assert_eq!(out, Ok(Outcome::default()));
    assert_eq!(pty.writes(), lines(&["1", "3", "\t", "2", "1"]));
}

#[test]
fn an_answer_walks_back_to_the_first_question() {
    let mut pty = dialog("Where?");
    let plan = plan::answer(&[Choice::Options(vec![2]), Choice::Option(2)], &two());
    assert_eq!(
        execute(&plan, &mut pty, &Pages, &mut NoQueueLog),
        Ok(Outcome::default())
    );
    assert_eq!(pty.writes(), lines(&[keys::LEFT, "2", "\t", "2", "1"]));
}

#[test]
fn an_answer_stops_when_the_page_is_another_question() {
    // Tab moves to a question Chat doesn't know: nothing more is typed.
    let mut pty = dialog("Pets?").reacting(|keys, screen| {
        if keys == "\t" {
            *screen = page("Something else?");
        }
    });
    let plan = plan::answer(&[Choice::Options(vec![1]), Choice::Option(2)], &two());
    let out = execute(&plan, &mut pty, &Pages, &mut NoQueueLog);
    assert_eq!(out, Err(Failure::QuestionMismatch));
    assert_eq!(pty.writes(), lines(&["1", "\t"]));
}

#[test]
fn an_answer_to_a_dialog_that_moved_on_types_nothing() {
    let mut pty = dialog("Unknown?");
    let plan = plan::answer(&[Choice::Options(vec![1]), Choice::Option(2)], &two());
    let out = execute(&plan, &mut pty, &Pages, &mut NoQueueLog);
    assert_eq!(out, Err(Failure::QuestionMismatch));
    // Left as many times as there are questions plus one, never a digit.
    assert_eq!(pty.writes(), lines(&[keys::LEFT, keys::LEFT, keys::LEFT]));
}

#[test]
fn an_answer_to_a_closed_question_is_gone() {
    let mut pty = Fake::new(PROMPT);
    let plan = plan::answer(&[Choice::Option(1)], &two());
    let out = execute(&plan, &mut pty, &Pages, &mut NoQueueLog);
    assert_eq!(out, Err(Failure::QuestionGone));
    assert!(pty.writes().is_empty());
}

#[test]
fn failures_say_what_the_swift_app_says() {
    assert_eq!(
        Failure::Dialog.to_string(),
        "The session is asking you something. Answer it in the terminal, then send again."
    );
    assert_eq!(
        Failure::Timeout.to_string(),
        "The session didn't open in time."
    );
    assert_eq!(
        Failure::QuestionGone.to_string(),
        "That question isn't open in the session any more."
    );
}
