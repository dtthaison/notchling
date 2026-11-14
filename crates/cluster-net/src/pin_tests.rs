use super::*;

#[test]
fn pinned_fingerprints_are_known() {
    let pins = Pins::default();
    pins.set(["aa".to_string(), "bb".to_string()]);
    assert!(pins.contains("aa"));
    assert!(!pins.contains("cc"));
    assert!(matches!(pins.admit("bb"), Some(Admit::Pinned)));
    assert!(pins.admit("cc").is_none(), "no join is open");
    pins.set(["cc".to_string()]);
    assert!(!pins.contains("aa"), "set replaces the list");
}

#[test]
fn join_window_admits_one_unpinned_peer() {
    let pins = Pins::default();
    assert!(!pins.may_handshake("x"));
    pins.set_join_open(true);
    assert!(pins.may_handshake("x"));
    let slot = pins.admit("x").expect("the joiner");
    assert!(matches!(slot, Admit::Joiner(_)));
    assert!(pins.admit("y").is_none(), "one unpinned peer at a time");
    assert!(!pins.may_handshake("y"));
    drop(slot);
    assert!(
        pins.admit("y").is_some(),
        "the slot frees when its link ends"
    );
}

#[test]
fn closing_the_window_admits_nobody_new() {
    let pins = Pins::default();
    pins.set_join_open(true);
    pins.set_join_open(false);
    assert!(pins.admit("x").is_none());
}

#[test]
fn a_joiner_that_gets_pinned_frees_the_slot() {
    let pins = Pins::default();
    pins.set_join_open(true);
    let slot = pins.admit("x").unwrap();
    pins.set(["x".to_string()]);
    assert!(pins.admit("y").is_some());
    drop(slot);
    assert!(pins.contains("x"));
}
