use super::tests::{ask_args, refusal, words, Fake, Rig, T0};
use super::*;
use crate::cli::{parse, Command};
use notchling_local_api::ErrorCode;
use serde_json::{json, Value};

#[test]
fn ask_arguments() {
    let a = ask_args(&[
        "--choice",
        "A",
        "--main-may-answer",
        "--wait",
        "--timeout",
        "30",
        "--json",
        "--socket",
        "/tmp/s.sock",
        "--choice",
        "B",
        "--",
        "Which",
        "one?",
    ]);
    assert_eq!(
        a,
        AskArgs {
            question: "Which one?".into(),
            choices: vec!["A".into(), "B".into()],
            main_may_answer: true,
            wait: true,
            timeout_s: Some(30),
            json: true,
            socket: Some("/tmp/s.sock".into()),
        }
    );
    for bad in [
        vec!["--"],
        vec!["question"],
        vec!["--timeout", "x", "--", "q"],
        vec!["--timeout", "0", "--", "q"],
        vec!["--choice"],
        vec!["--nope", "--", "q"],
    ] {
        assert!(parse_ask(&words(&bad)).is_err(), "{bad:?}");
    }
}

#[test]
fn answer_arguments() {
    let a = parse_answer(&words(&["q1", "--choice", "2"])).unwrap();
    assert_eq!((a.id.as_str(), a.choice, a.text), ("q1", Some(2), None));
    let a = parse_answer(&words(&["q1", "--", "Keep", "it"])).unwrap();
    assert_eq!((a.choice, a.text.as_deref()), (None, Some("Keep it")));
    for bad in [
        vec![],
        vec!["q1"],
        vec!["--choice", "1"],
        vec!["q1", "--choice", "x"],
        vec!["q1", "--choice", "0"],
        vec!["q1", "--choice", "1", "--", "text"],
        vec!["q1", "--"],
    ] {
        assert!(parse_answer(&words(&bad)).is_err(), "{bad:?}");
    }
}

#[test]
fn the_command_line_passes_the_words_through() {
    for word in ["ask", "asks", "answer"] {
        let parsed = parse(words(&[word, "--wait", "--", "x"]));
        let args = words(&["--wait", "--", "x"]);
        let want = match word {
            "ask" => Command::Ask { args },
            "asks" => Command::Asks { args },
            _ => Command::Answer { args },
        };
        assert_eq!(parsed, Ok(want));
    }
    let usage = crate::cli::usage();
    for line in ["notchling ask ", "notchling asks ", "notchling answer "] {
        assert!(usage.contains(line), "{line}");
    }
}

fn row(id: &str, label: Option<&str>, choices: Option<Vec<&str>>) -> Value {
    let mut row = json!({
        "id": id, "session": "s", "asker": "subagent", "branch": "agent/ask",
        "question": "Keep the cache?", "mainMayAnswer": true, "mode": "background",
        "state": "open", "waiting": true, "createdMs": T0 - 125_000, "updatedMs": T0,
    });
    if let Some(label) = label {
        row["label"] = json!(label);
    }
    if let Some(choices) = choices {
        row["choices"] = json!(choices);
    }
    row
}

#[test]
fn asks_and_answer_print_and_exit() {
    let mut rig = Rig::new();
    let rows = json!([
        row("q1", Some("explorer"), Some(vec!["Keep it", "Drop it"])),
        row("q2", None, None),
    ]);
    let mut link = Fake::new(&rig.clock, vec![Ok(rows.clone())]);
    let args = parse_asks(&words(&[])).unwrap();
    assert_eq!(rig.run(|env| asks(&mut link, &args, env)), 0);
    assert_eq!(
        rig.stdout(),
        "q1  «explorer» (subagent) · agent/ask · 2m\n\
         Keep the cache?\n\
         \x20 1. Keep it\n\
         \x20 2. Drop it\n\
         q2  (subagent) · agent/ask · 2m\n\
         Keep the cache?\n"
    );
    assert_eq!(link.of("ask.mine")[0], &json!({}));

    let mut rig = Rig::new();
    let mut link = Fake::new(&rig.clock, vec![Ok(rows.clone())]);
    let args = parse_asks(&words(&["--json"])).unwrap();
    assert_eq!(rig.run(|env| asks(&mut link, &args, env)), 0);
    assert_eq!(
        serde_json::from_str::<Value>(rig.stdout().trim()).unwrap(),
        rows
    );

    let mut rig = Rig::new();
    let mut link = Fake::new(&rig.clock, vec![Ok(json!([]))]);
    let args = parse_asks(&words(&[])).unwrap();
    assert_eq!(rig.run(|env| asks(&mut link, &args, env)), 0);
    assert_eq!(rig.stdout(), "No questions waiting.\n");

    let mut rig = Rig::new();
    let mut link = Fake::new(
        &rig.clock,
        vec![Ok(json!([])), Err(LinkError::Down), Ok(rows.clone())],
    );
    let args = parse_asks(&words(&["--wait"])).unwrap();
    assert_eq!(rig.run(|env| asks(&mut link, &args, env)), 0);
    assert_eq!(link.of("ask.mine").len(), 3);
    assert!(link.of("ask.mine").iter().all(|a| a["waitMs"] == 50_000));
    assert_eq!(*rig.sleeps.borrow(), [1_000]);
    assert!(rig.stdout().starts_with("q1  «explorer»"));

    let mut rig = Rig::new();
    let mut link = Fake::new(&rig.clock, vec![Ok(json!({"answered": true}))]);
    let args = parse_answer(&words(&["q1", "--choice", "2"])).unwrap();
    assert_eq!(rig.run(|env| answer(&mut link, &args, env)), 0);
    assert_eq!(rig.stdout(), "Answered.\n");
    assert_eq!(link.of("ask.answer")[0], &json!({"id": "q1", "choice": 2}));

    let mut rig = Rig::new();
    let mut link = Fake::new(&rig.clock, vec![Ok(json!({"answered": true}))]);
    let args = parse_answer(&words(&["q1", "--", "Keep it"])).unwrap();
    assert_eq!(rig.run(|env| answer(&mut link, &args, env)), 0);
    assert_eq!(
        link.of("ask.answer")[0],
        &json!({"id": "q1", "text": "Keep it"})
    );

    let mut rig = Rig::new();
    let mut link = Fake::new(
        &rig.clock,
        vec![Ok(
            json!({"answered": false, "state": "delivered", "by": "phone"}),
        )],
    );
    assert_eq!(rig.run(|env| answer(&mut link, &args, env)), 1);
    assert_eq!(rig.stdout(), "");
    assert_eq!(rig.stderr(), "Already answered · Phone. (failed)\n");

    let mut rig = Rig::new();
    let mut link = Fake::new(
        &rig.clock,
        vec![refusal(
            ErrorCode::Forbidden,
            "Only the person answers this question.",
        )],
    );
    assert_eq!(rig.run(|env| answer(&mut link, &args, env)), 3);
    assert_eq!(rig.stderr(), "Only the person answers this question.\n");

    let mut rig = Rig::new();
    let mut link = Fake::new(&rig.clock, vec![Err(LinkError::Down)]);
    assert_eq!(rig.run(|env| answer(&mut link, &args, env)), 3);
    assert!(rig.stderr().starts_with("Notchling isn't running."));
}
