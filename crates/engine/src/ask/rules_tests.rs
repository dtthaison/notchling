use super::*;
use crate::ask::model::{AskState, Asker, Mode, OpenError, Row};

fn row(asker: Asker, state: AskState, main_may_answer: bool) -> Row {
    Row {
        n: 1,
        id: "q1".to_string(),
        key: "k".to_string(),
        session: "s1".to_string(),
        session_id: None,
        asker,
        agent_id: None,
        label: None,
        agent_type: None,
        model: None,
        tool_use_id: None,
        folder: None,
        branch: None,
        question: "Keep it?".to_string(),
        choices: Some(vec!["a".to_string(), "b".to_string(), "c".to_string()]),
        main_may_answer,
        mode: Mode::Background,
        deadline_ms: None,
        pid: 1,
        pid_start: None,
        state,
        answer: None,
        choice: None,
        by: None,
        inbox_id: None,
        created_ms: 0,
        updated_ms: 0,
        closed_ms: None,
    }
}

const ASKERS: [Asker; 5] = [
    Asker::Main,
    Asker::Subagent,
    Asker::Codex,
    Asker::Antigravity,
    Asker::Unknown,
];

#[test]
fn may_answer_person_any_open() {
    for asker in ASKERS {
        assert_eq!(
            may_answer(&row(asker, AskState::Open, false), &Who::Person),
            Ok(())
        );
    }
    assert_eq!(
        may_answer(&row(Asker::Main, AskState::Answered, false), &Who::Person),
        Err(Refused::NotOpen(AskState::Answered))
    );
    assert_eq!(
        may_answer(&row(Asker::Main, AskState::Gone, false), &Who::Person),
        Err(Refused::NotOpen(AskState::Gone))
    );
}

#[test]
fn may_answer_agent_needs_every_rule() {
    let main = Who::Agent {
        session: "s1",
        is_main: true,
    };
    let open = row(Asker::Subagent, AskState::Open, true);
    assert_eq!(may_answer(&open, &main), Ok(()));
    assert_eq!(
        may_answer(&row(Asker::Subagent, AskState::Open, false), &main),
        Err(Refused::PersonOnly)
    );
    assert_eq!(
        may_answer(
            &open,
            &Who::Agent {
                session: "other",
                is_main: true
            }
        ),
        Err(Refused::PersonOnly)
    );
    assert_eq!(
        may_answer(
            &open,
            &Who::Agent {
                session: "s1",
                is_main: false
            }
        ),
        Err(Refused::PersonOnly)
    );
    assert_eq!(
        may_answer(&row(Asker::Subagent, AskState::Answered, true), &main),
        Err(Refused::NotOpen(AskState::Answered))
    );
    for asker in [Asker::Main, Asker::Unknown] {
        assert_eq!(
            may_answer(&row(asker, AskState::Open, true), &main),
            Err(Refused::PersonOnly)
        );
    }
    for asker in [Asker::Codex, Asker::Antigravity] {
        assert_eq!(may_answer(&row(asker, AskState::Open, true), &main), Ok(()));
    }
}

#[test]
fn main_may_see_matches_rules_one_to_three() {
    assert!(main_may_see(
        &row(Asker::Subagent, AskState::Open, true),
        "s1"
    ));
    assert!(main_may_see(&row(Asker::Codex, AskState::Open, true), "s1"));
    assert!(!main_may_see(
        &row(Asker::Subagent, AskState::Open, true),
        "s2"
    ));
    assert!(!main_may_see(
        &row(Asker::Subagent, AskState::Open, false),
        "s1"
    ));
    assert!(!main_may_see(
        &row(Asker::Subagent, AskState::Answered, true),
        "s1"
    ));
    assert!(!main_may_see(&row(Asker::Main, AskState::Open, true), "s1"));
    assert!(!main_may_see(
        &row(Asker::Unknown, AskState::Open, true),
        "s1"
    ));
}

fn choices(n: usize) -> Vec<String> {
    (0..n).map(|i| format!("c{i}")).collect()
}

#[test]
fn check_new_limits() {
    let two = choices(2);
    assert_eq!(check_new("", None, false), Err(OpenError::Empty));
    assert_eq!(check_new("  ", None, false), Err(OpenError::Empty));
    assert_eq!(
        check_new(&"x".repeat(2_001), None, false),
        Err(OpenError::TooLong)
    );
    assert_eq!(check_new(&"é".repeat(2_000), None, false), Ok(()));
    assert_eq!(
        check_new("q", Some(&choices(1)), false),
        Err(OpenError::Choices)
    );
    assert_eq!(
        check_new("q", Some(&choices(7)), false),
        Err(OpenError::Choices)
    );
    assert_eq!(check_new("q", Some(&choices(6)), false), Ok(()));
    assert_eq!(
        check_new("q", Some(&["a".to_string(), "".to_string()]), false),
        Err(OpenError::ChoiceText)
    );
    assert_eq!(
        check_new("q", Some(&["a".to_string(), "b".repeat(121)]), false),
        Err(OpenError::ChoiceText)
    );
    assert_eq!(
        check_new("q", Some(&["a".to_string(), "é".repeat(120)]), false),
        Ok(())
    );
    assert_eq!(
        check_new("q", None, true),
        Err(OpenError::WorkerNeedsChoices)
    );
    assert_eq!(check_new("q", Some(&two), true), Ok(()));
    assert_eq!(check_new("q", None, false), Ok(()));
}

#[test]
fn answer_text_rules() {
    let r = row(Asker::Main, AskState::Open, false);
    assert_eq!(
        answer_text(&r, None, Some(2)),
        Ok(("b".to_string(), Some(2)))
    );
    assert_eq!(
        answer_text(&r, None, Some(4)),
        Err(AnswerError::NoSuchChoice)
    );
    assert_eq!(
        answer_text(&r, None, Some(0)),
        Err(AnswerError::NoSuchChoice)
    );
    assert_eq!(answer_text(&r, None, None), Err(AnswerError::Missing));
    assert_eq!(
        answer_text(&r, Some("x"), Some(1)),
        Err(AnswerError::Missing)
    );
    assert_eq!(answer_text(&r, Some("  "), None), Err(AnswerError::Missing));
    assert_eq!(
        answer_text(&r, Some("typed"), None),
        Ok(("typed".to_string(), None))
    );
    let ten_kb = "a".repeat(MAX_ANSWER_BYTES);
    assert_eq!(
        answer_text(&r, Some(&ten_kb), None),
        Ok((ten_kb.clone(), None))
    );
    assert_eq!(
        answer_text(&r, Some(&format!("{ten_kb}a")), None),
        Err(AnswerError::TooLong)
    );
    let mut none = row(Asker::Main, AskState::Open, false);
    none.choices = None;
    assert_eq!(
        answer_text(&none, None, Some(1)),
        Err(AnswerError::NoSuchChoice)
    );
}

#[test]
fn cut_ends_with_an_ellipsis_within_the_limit() {
    assert_eq!(cut("short", 10), "short");
    assert_eq!(cut("abcdef", 4), "abc…");
    assert_eq!(cut("abcdef", 0), "");
    assert_eq!(cut("😀😀😀😀", 3).chars().count(), 3);
}
