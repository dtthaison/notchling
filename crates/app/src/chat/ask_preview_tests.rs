use super::*;
use crate::chat::asks::card;

const NOW: i64 = 5_000_000_000;

fn cards(state: &str) -> Vec<crate::chat::asks::Card> {
    preview(state, NOW)
        .unwrap_or_default()
        .iter()
        .filter_map(|row| card(row, NOW))
        .collect()
}

#[test]
fn every_state_makes_one_card_and_an_unknown_name_none() {
    for state in STATES {
        assert_eq!(cards(state).len(), 1, "{state}");
    }
    assert!(preview("nothing", NOW).is_none());
}

#[test]
fn the_open_states_show_who_asks_and_what_can_be_picked() {
    let open = &cards("open")[0];
    assert!(open.open);
    assert!(open.choices.is_empty());
    assert!(
        open.header.starts_with("This session asks"),
        "{}",
        open.header
    );
    assert!(open.header.ends_with("waited 4 min"), "{}", open.header);
    let choices = &cards("open-choices")[0];
    assert_eq!(choices.choices, ["Keep it", "Drop it", "Ask me later"]);
    assert!(choices.header.contains("agent/ask"), "{}", choices.header);
    let unknown = &cards("unknown")[0];
    assert!(unknown
        .header
        .starts_with("This session or one of its subagents"));
}

#[test]
fn the_closed_states_keep_one_line() {
    let line = |state: &str| cards(state)[0].closed_line.clone();
    assert_eq!(line("answered").as_deref(), Some("Answered · Mac: Keep it"));
    assert_eq!(line("dismissed").as_deref(), Some("Dismissed"));
    assert_eq!(line("gone").as_deref(), Some("Stopped waiting"));
    assert!(!cards("gone")[0].open);
}
