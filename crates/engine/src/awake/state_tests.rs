use super::*;
use serde_json::json;

#[test]
fn a_hold_is_named_by_its_kind_and_nothing_is_null() {
    assert_eq!(state_json(None), json!({"holding": null}));
    assert_eq!(state_json(Some(Kind::System)), json!({"holding": "system"}));
    assert_eq!(
        state_json(Some(Kind::Display)),
        json!({"holding": "display"})
    );
}

#[test]
fn the_wire_words_read_back_to_the_same_kind() {
    for kind in [Kind::System, Kind::Display] {
        assert_eq!(Kind::parse(kind.as_str()), Some(kind));
        assert_eq!(holding_in(&state_json(Some(kind))), Some(Some(kind)));
    }
    assert_eq!(holding_in(&state_json(None)), Some(None));
}

#[test]
fn a_reply_that_isnt_a_state_says_nothing() {
    assert_eq!(holding_in(&json!({})), None);
    assert_eq!(holding_in(&json!({"holding": "forever"})), None);
    assert_eq!(holding_in(&json!({"holding": 3})), None);
    assert_eq!(holding_in(&json!("system")), None);
}
