use super::*;

fn words(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| s.to_string()).collect()
}

#[test]
fn on_and_off_parse_with_the_common_flags() {
    let on = parse(&words(&[
        "on",
        "vault",
        "--reason",
        "to read a token",
        "--no-wait",
        "--expires-in",
        "120",
        "--key",
        "k1",
        "--json",
    ]))
    .unwrap();
    assert!(on.on && on.json && !on.flags.wait);
    assert_eq!(on.name, "vault");
    assert_eq!(
        request_body(&on),
        json!({"switch": "vault", "on": true,
               "consent": {"reason": "to read a token", "expiresInMs": 120_000, "key": "k1"}})
    );
    let off = parse(&words(&["off", "cluster"])).unwrap();
    assert!(!off.on);
    assert_eq!(off.name, "cluster");
}

#[test]
fn a_bad_switch_or_verb_is_usage() {
    assert!(parse(&words(&["on", "everything"])).is_err());
    assert!(parse(&words(&["up", "vault"])).is_err());
    assert!(parse(&words(&["on"])).is_err());
    assert!(parse(&words(&["on", "vault", "--odd"])).is_err());
}

#[test]
fn a_dry_run_says_so_in_the_body() {
    let dry = parse(&words(&["on", "acting", "--dry-run"])).unwrap();
    assert_eq!(request_body(&dry)["dryRun"], json!(true));
}

#[test]
fn off_says_whether_it_changed() {
    assert_eq!(
        off_line(&json!({"switch": "bridge", "changed": true})),
        "bridge is off."
    );
    assert_eq!(
        off_line(&json!({"switch": "bridge", "changed": false})),
        "bridge was already off."
    );
}
