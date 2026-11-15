use super::*;

#[test]
fn the_three_words_for_a_message_that_is_gone_or_already_on_its_way_are_pinned() {
    assert_eq!(NO_MESSAGE, "No such message.");
    assert_eq!(BEING_SENT, "It's being sent.");
    assert_eq!(IN_QUEUE, "It's in the session's queue.");
}

#[test]
fn only_those_three_say_the_message_is_settled() {
    for message in [NO_MESSAGE, BEING_SENT, IN_QUEUE] {
        assert!(is_settled(message), "{message}");
    }
    for message in [
        "Type a message first.",
        "That message is over 100 KB.",
        "Couldn't save the message.",
        "That command opens a menu; open the session's terminal.",
        "",
    ] {
        assert!(!is_settled(message), "{message}");
    }
}
