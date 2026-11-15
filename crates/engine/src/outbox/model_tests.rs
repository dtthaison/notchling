use super::*;

#[test]
fn words_round_trip_through_as_str_and_parse() {
    for tool in [Tool::Claude, Tool::Codex, Tool::Antigravity] {
        assert_eq!(Tool::parse(tool.as_str()), Some(tool));
    }
    for kind in [Kind::Prompt, Kind::Command] {
        assert_eq!(Kind::parse(kind.as_str()), Some(kind));
    }
    for via in [Via::Chat, Via::Notch, Via::Phone, Via::Agent] {
        assert_eq!(Via::parse(via.as_str()), Some(via));
    }
    for state in [
        State::Queued,
        State::Waiting,
        State::Sending,
        State::InQueue,
        State::Unsure,
        State::Box,
        State::Stopped,
        State::Gone,
    ] {
        assert_eq!(State::parse(state.as_str()), Some(state));
    }
    for why in [
        Why::Dialog,
        Why::Queue,
        Why::Turn,
        Why::Retry,
        Why::NotFound,
        Why::Restart,
        Why::Dropped,
        Why::NotCleared,
        Why::Mixed,
    ] {
        assert_eq!(Why::parse(why.as_str()), Some(why));
    }
    assert_eq!(State::parse("nonsense"), None);
}

#[test]
fn wire_words_are_snake_case() {
    assert_eq!(State::InQueue.as_str(), "in_queue");
    assert_eq!(Why::NotFound.as_str(), "not_found");
    assert_eq!(Why::NotCleared.as_str(), "not_cleared");
    assert_eq!(Tool::Antigravity.as_str(), "antigravity");
    assert_eq!(
        serde_json::to_string(&State::InQueue).unwrap(),
        "\"in_queue\""
    );
    assert_eq!(
        serde_json::to_string(&Why::NotCleared).unwrap(),
        "\"not_cleared\""
    );
    assert_eq!(
        serde_json::from_str::<Via>("\"phone\"").unwrap(),
        Via::Phone
    );
}

#[test]
fn kind_is_command_only_for_claude_slash_text() {
    assert_eq!(Kind::of(Tool::Claude, "/clear"), Kind::Command);
    assert_eq!(Kind::of(Tool::Claude, "\n /clear"), Kind::Command);
    assert_eq!(Kind::of(Tool::Claude, "say /clear"), Kind::Prompt);
    assert_eq!(Kind::of(Tool::Codex, "/clear"), Kind::Prompt);
    assert_eq!(Kind::of(Tool::Antigravity, "/clear"), Kind::Prompt);
}
