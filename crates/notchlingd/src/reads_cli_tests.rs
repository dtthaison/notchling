use super::*;

fn words(list: &[&str]) -> Vec<String> {
    list.iter().map(|w| (*w).to_string()).collect()
}

fn checked(name: &str, list: &[&str]) -> Result<Value, Failure> {
    let args = words(list);
    let parsed = parse(name, &args).map_err(Failure::usage)?;
    check(name, &parsed)
}

#[test]
fn a_flag_value_that_starts_with_two_dashes_is_a_usage_error() {
    for (name, list) in [
        ("limits", vec!["--tool", "--json"]),
        ("limits", vec!["--tool", "codex", "--pool", "--json"]),
        ("search", vec!["x", "--kind", "--since", "5"]),
        ("search", vec!["x", "--since", "--json"]),
        ("limits", vec!["--socket", "--tool", "codex"]),
        ("limits", vec!["--tool", ""]),
    ] {
        let failure = checked(name, &list).unwrap_err();
        assert_eq!(failure.exit, contract::BAD_REQUEST, "{list:?}");
    }
}

#[test]
fn since_is_a_finite_number_of_seconds_from_zero() {
    for bad in ["NaN", "inf", "-inf", "-1", "soon"] {
        let failure = checked("search", &["x", "--since", bad]).unwrap_err();
        assert_eq!(failure.exit, contract::BAD_REQUEST, "{bad}");
    }
    let query = checked("search", &["x", "--since", "0"]).unwrap();
    assert_eq!(query["since"], json!(0.0));
}

#[test]
fn usage_is_checked_before_any_connection() {
    for (name, list) in [
        ("limits", vec!["--tool", "nope"]),
        ("limits", vec!["--pool", "x"]),
        ("search", vec!["x", "--kind", "folders"]),
        ("awake", vec!["maybe"]),
        ("awake", vec!["on", "off"]),
    ] {
        assert!(checked(name, &list).is_err(), "{name} {list:?}");
    }
    assert_eq!(
        checked("awake", &["on"]).unwrap()["mode"],
        json!("on"),
        "on and off are settled before a socket is opened"
    );
}

#[test]
fn awake_on_forwards_the_socket_and_json_and_a_socket_named_on_is_not_the_word() {
    let args = words(&["--socket", "on", "off", "--json"]);
    let parsed = parse("awake", &args).unwrap();
    assert_eq!(parsed.words, words(&["off"]));
    assert_eq!(
        awake_set("off", &parsed, true),
        words(&["set", "mode", "off", "--socket", "on", "--json"])
    );
    let plain = parse("awake", &words(&["on"])).unwrap();
    assert_eq!(
        awake_set("on", &plain, false),
        words(&["set", "mode", "on"])
    );
}
