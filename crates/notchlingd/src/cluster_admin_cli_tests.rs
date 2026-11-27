use super::*;

fn scanned(list: &[&str]) -> Scanned {
    let raw: Vec<String> = list.iter().map(|s| s.to_string()).collect();
    scan(
        &raw,
        USAGE,
        &["--node", "--name", "--role", "--acting", "--delegation"],
    )
    .unwrap()
}

fn body(list: &[&str]) -> Result<Value, String> {
    request(&scanned(list))
}

#[test]
fn each_word_becomes_an_action_with_its_args() {
    assert_eq!(
        body(&["create", "home"]).unwrap(),
        json!({"action": "create", "args": {"name": "home"}, "consent": {}})
    );
    assert_eq!(body(&["token"]).unwrap()["action"], "token");
    assert_eq!(
        body(&[
            "invite",
            "--node",
            "0123456789abcdef",
            "--name",
            "box",
            "--role",
            "master",
            "--acting",
            "on"
        ])
        .unwrap()["args"],
        json!({"node": "0123456789abcdef", "name": "box", "canBeMaster": true, "acting": true})
    );
    assert_eq!(
        body(&["member", "box", "--role", "worker", "--delegation", "off"]).unwrap()["args"],
        json!({"node": "box", "role": "worker", "delegation": false})
    );
    assert_eq!(
        body(&["failover", "on"]).unwrap()["args"],
        json!({"value": "on"})
    );
    assert_eq!(
        body(&["main", "abc", "off"]).unwrap()["args"],
        json!({"session": "abc", "on": false})
    );
    assert_eq!(
        body(&["allow", "box", "a", "b"]).unwrap()["args"],
        json!({"node": "box", "names": ["a", "b"]})
    );
    assert_eq!(body(&["remove", "box"]).unwrap()["action"], "remove");
    assert_eq!(body(&["move-home", "box"]).unwrap()["action"], "move_home");
    assert_eq!(body(&["move-now"]).unwrap()["action"], "move_now");
}

#[test]
fn the_common_flags_go_in_the_consent_and_dry_run_on_the_body() {
    let b = body(&[
        "remove",
        "box",
        "--reason",
        "retired",
        "--key",
        "k1",
        "--expires-in",
        "60",
        "--dry-run",
    ])
    .unwrap();
    assert_eq!(b["dryRun"], json!(true));
    assert_eq!(
        b["consent"],
        json!({"reason": "retired", "expiresInMs": 60_000, "key": "k1"})
    );
}

#[test]
fn a_bad_shape_is_usage() {
    assert!(body(&["create"]).is_err());
    assert!(body(&["create", "a", "b"]).is_err());
    assert!(body(&["invite", "--node", "x"]).is_err());
    assert!(body(&["member", "box"]).is_err());
    assert!(body(&["member", "box", "--role", "boss"]).is_err());
    assert!(body(&["main", "abc", "maybe"]).is_err());
    assert!(body(&["move-now", "box"]).is_err());
}

#[test]
fn only_the_admin_words_are_taken() {
    assert!(handles("create") && handles("move-home"));
    assert!(!handles("status") && !handles("join") && !handles("promote"));
}

#[test]
fn the_token_is_printed_once_with_the_done_line() {
    let line = done_line(&json!({"result": {"action": "token"}, "handover": {"token": "nlj1.x"}}));
    assert!(line.contains("nlj1.x"));
    assert!(!done_line(&json!({"result": {"action": "remove"}})).contains("token"));
}
