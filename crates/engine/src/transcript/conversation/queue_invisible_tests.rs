use super::Parser;
use crate::transcript::model::Conversation;
use serde_json::json;

fn enqueue(content: &str) -> String {
    json!({"type": "queue-operation", "operation": "enqueue", "content": content}).to_string()
}

fn operation(name: &str, content: &str) -> String {
    json!({"type": "queue-operation", "operation": name, "content": content}).to_string()
}

fn dequeue() -> String {
    json!({"type": "queue-operation", "operation": "dequeue"}).to_string()
}

fn said(text: &str) -> String {
    json!({"type": "user", "message": {"role": "user", "content": text}}).to_string()
}

fn parse(lines: &[String]) -> Conversation {
    let mut parser = Parser::new();
    for line in lines {
        parser.consume(line.as_bytes());
    }
    parser.conversation()
}

#[test]
fn a_remove_or_pop_all_names_the_entry_through_dropped_characters() {
    for name in ["remove", "popAll"] {
        let c = parse(&[
            enqueue("carry the\u{200b} lantern"),
            enqueue("and the lamp"),
            operation(name, "carry the lantern"),
        ]);
        assert_eq!(c.queued, ["and the lamp"], "{name}");
    }
}

#[test]
fn a_remove_of_another_text_leaves_the_queue_alone() {
    let c = parse(&[
        enqueue("carry the lantern"),
        operation("remove", "carry the lamp"),
    ]);
    assert_eq!(c.queued, ["carry the lantern"]);
}

#[test]
fn a_taken_line_names_the_waiting_entry_through_dropped_characters() {
    let c = parse(&[
        enqueue("first one"),
        enqueue("second\u{200b} one"),
        dequeue(),
        said("second one"),
    ]);
    assert_eq!(c.queued, ["first one"]);
}
