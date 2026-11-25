use super::cut_steps;
use crate::transcript::conversation::Parser;
use crate::transcript::model::Conversation;

const MAIN: &str = include_str!("../../../tests/fixtures/background-job/main.jsonl");

fn parse(lines: usize) -> Conversation {
    let mut parser = Parser::new();
    for line in MAIN.lines().take(lines) {
        parser.consume(line.as_bytes());
    }
    parser.conversation()
}

fn agent_steps(conv: &Conversation) -> usize {
    cut_steps(conv)
        .iter()
        .filter(|step| step.tool == "Agent")
        .count()
}

#[test]
fn a_background_agent_that_reported_is_not_cut_work() {
    let conv = parse(5);
    assert_eq!(
        conv.finished_agents
            .get("a0b1c2d3e4f5a6b7c")
            .map(String::as_str),
        Some("completed")
    );
    assert_eq!(agent_steps(&conv), 0);
}

#[test]
fn a_background_agent_the_parent_resumed_is_cut_work_again() {
    let conv = parse(usize::MAX);
    assert_eq!(
        conv.finished_agents
            .get("a0b1c2d3e4f5a6b7c")
            .map(String::as_str),
        Some("resumed")
    );
    assert_eq!(agent_steps(&conv), 1);
}
