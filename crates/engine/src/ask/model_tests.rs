use super::*;

#[test]
fn words_round_trip() {
    for word in ["main", "subagent", "codex", "antigravity", "unknown"] {
        assert_eq!(Asker::parse(word).unwrap().as_str(), word);
    }
    for word in ["background", "wait"] {
        assert_eq!(Mode::parse(word).unwrap().as_str(), word);
    }
    for word in [
        "open",
        "answered",
        "delivered",
        "dismissed",
        "expired",
        "gone",
    ] {
        assert_eq!(AskState::parse(word).unwrap().as_str(), word);
    }
    for word in ["mac", "phone", "main"] {
        assert_eq!(By::parse(word).unwrap().as_str(), word);
    }
    assert_eq!(Asker::parse("other"), None);
    assert_eq!(By::parse(""), None);
}

#[test]
fn closed_states() {
    assert!(!AskState::Open.is_closed());
    assert!(!AskState::Answered.is_closed());
    for state in [
        AskState::Delivered,
        AskState::Dismissed,
        AskState::Expired,
        AskState::Gone,
    ] {
        assert!(state.is_closed());
    }
}

#[test]
fn serde_uses_snake_case() {
    assert_eq!(
        serde_json::to_string(&Asker::Subagent).unwrap(),
        "\"subagent\""
    );
    assert_eq!(
        serde_json::from_str::<AskState>("\"dismissed\"").unwrap(),
        AskState::Dismissed
    );
}

#[test]
fn ids_are_q_and_32_hex() {
    let a = new_id();
    let b = new_id();
    assert_ne!(a, b);
    assert_eq!(a.len(), 33);
    assert!(a.starts_with('q'));
    assert!(a[1..]
        .chars()
        .all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c)));
}

#[test]
fn constants_match_the_spec() {
    assert_eq!(MAX_QUESTION_CHARS, 2_000);
    assert_eq!((MIN_CHOICES, MAX_CHOICES, MAX_CHOICE_CHARS), (2, 6, 120));
    assert_eq!(MAX_ANSWER_BYTES, 10_240);
    assert_eq!((MAX_OPEN_PER_SESSION, MAX_OPEN), (20, 100));
    assert_eq!(KEEP_CLOSED_MS, 24 * 60 * 60 * 1000);
    assert_eq!(DEADLINE_VARIABLE, "NOTCHLING_DELEGATE_DEADLINE_MS");
}
