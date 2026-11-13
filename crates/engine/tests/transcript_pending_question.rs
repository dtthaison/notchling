use notchling_engine::transcript::conversation::Parser;
use notchling_engine::transcript::snapshot::{PendingQuestion, QuestionItem};

fn ask(id: &str, question: &str, extra: &str) -> String {
    format!(
        r#"{{"type":"assistant"{extra},"timestamp":"2026-11-08T08:10:00.000Z","message":{{"id":"m-{id}","role":"assistant","content":[{{"type":"tool_use","id":"{id}","name":"AskUserQuestion","input":{{"questions":[{{"header":"README","question":"{question}","multiSelect":false,"options":[{{"label":"Now","description":"Right away"}},{{"label":"After the tests","description":"Later"}}]}}]}}}}]}}}}"#
    )
}

fn answer(id: &str) -> String {
    format!(
        r#"{{"type":"user","timestamp":"2026-11-08T08:11:00.000Z","message":{{"role":"user","content":[{{"type":"tool_result","tool_use_id":"{id}","content":"Now"}}]}}}}"#
    )
}

fn pending(lines: &[String], parser: Parser) -> Option<PendingQuestion> {
    let mut parser = parser;
    for line in lines {
        parser.consume(line.as_bytes());
    }
    parser.conversation().pending_question
}

fn expected(id: &str, question: &str) -> PendingQuestion {
    PendingQuestion {
        tool_use_id: id.into(),
        items: vec![QuestionItem {
            header: "README".into(),
            question: question.into(),
            options: vec!["Now".into(), "After the tests".into()],
            details: vec!["Right away".into(), "Later".into()],
            multi_select: false,
        }],
    }
}

#[test]
fn an_unanswered_question_is_pending() {
    let got = pending(&[ask("q1", "Update it?", "")], Parser::new());
    assert_eq!(got, Some(expected("q1", "Update it?")));
}

#[test]
fn an_answered_question_is_not() {
    let lines = [ask("q1", "Update it?", ""), answer("q1")];
    assert_eq!(pending(&lines, Parser::new()), None);
}

#[test]
fn the_last_question_wins() {
    let lines = [ask("q1", "First?", ""), ask("q2", "Second?", "")];
    assert_eq!(
        pending(&lines, Parser::new()),
        Some(expected("q2", "Second?"))
    );
    let lines = [
        ask("q1", "First?", ""),
        ask("q2", "Second?", ""),
        answer("q1"),
    ];
    assert_eq!(
        pending(&lines, Parser::new()),
        Some(expected("q2", "Second?"))
    );
}

#[test]
fn a_sidechain_question_is_not_the_sessions() {
    let lines = [ask("q1", "Subagent?", r#","isSidechain":true"#)];
    assert_eq!(pending(&lines, Parser::new()), None);
}
