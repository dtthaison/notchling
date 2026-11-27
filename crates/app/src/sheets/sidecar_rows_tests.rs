use super::*;
use serde_json::json;

fn shown(state: &str, running: bool) -> Option<Shown> {
    Shown::from_reply(&Ok(json!({"state": state, "running": running})))
}

#[test]
fn the_daemons_states_map_to_the_row_states() {
    for (word, state) in [
        ("no_app", State::NoApp),
        ("off", State::Off),
        ("partial", State::Partial),
        ("on", State::On),
        ("blocked", State::Blocked),
    ] {
        assert_eq!(shown(word, false).unwrap().state, state, "{word}");
    }
    assert_eq!(shown("sideways", false), None);
    assert_eq!(Shown::from_reply(&Err("no daemon".into())), None);
    assert_eq!(Shown::from_reply(&Ok(json!({}))), None);
}

#[test]
fn on_says_whether_antigravity_has_started_the_sidecar() {
    assert_eq!(
        shown("on", true).unwrap().word(),
        t::settings::sidecar_state_on()
    );
    assert_eq!(
        shown("on", false).unwrap().word(),
        t::settings::sidecar_state_waiting()
    );
    assert_ne!(
        t::settings::sidecar_state_on(),
        t::settings::sidecar_state_waiting()
    );
}

#[test]
fn every_state_has_its_own_words_and_the_buttons_follow_the_state() {
    let mut words = std::collections::HashSet::new();
    for state in ["no_app", "off", "partial", "on", "blocked"] {
        let s = shown(state, true).unwrap();
        assert!(words.insert(s.word()), "{state}");
        assert_eq!(
            s.can_turn_on(),
            matches!(state, "off" | "partial"),
            "{state}"
        );
        assert_eq!(
            s.can_turn_off(),
            matches!(state, "on" | "partial"),
            "{state}"
        );
    }
}

#[test]
fn the_row_shows_when_the_channel_is_chosen_or_something_of_it_is_still_in_antigravity() {
    assert!(visible(true, None));
    assert!(!visible(false, None));
    assert!(!visible(false, shown("off", false).as_ref()));
    assert!(!visible(false, shown("no_app", false).as_ref()));
    assert!(visible(false, shown("on", true).as_ref()));
    assert!(visible(false, shown("partial", false).as_ref()));
    assert!(is_chosen(Some(&json!("sidecar"))));
    assert!(!is_chosen(Some(&json!("acp"))));
    assert!(!is_chosen(None));
}

#[test]
fn a_failure_keeps_the_daemons_words_and_a_success_says_done() {
    assert_eq!(
        outcome(&Err("install is off".into())),
        ("install is off".to_owned(), true)
    );
    assert_eq!(
        outcome(&Ok(json!({"state": "on"}))),
        (t::settings::sidecar_done().to_owned(), false)
    );
}

#[test]
fn the_words_say_what_the_row_does() {
    assert!(t::settings::antigravity_sidecar().contains("Turn on the Notchling sidecar"));
    assert_eq!(t::settings::sidecar_turn_on(), "Turn On");
    assert!(t::settings::sidecar_confirm_note().contains("config.json"));
    assert!(t::settings::sidecar_note().contains("Message from System"));
    assert!(t::settings::sidecar_note().contains("workspace folder"));
}
