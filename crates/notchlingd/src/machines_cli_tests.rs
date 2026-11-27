use super::*;

fn scanned(args: &[&str]) -> Scanned {
    let raw: Vec<String> = args.iter().map(|s| s.to_string()).collect();
    scan(&raw, USAGE, &OWN).unwrap()
}

#[test]
fn reads_map_to_their_ops() {
    assert_eq!(read_call(&["list"]).unwrap().0, ops::MACHINES_LIST);
    assert_eq!(
        read_call(&["check", "m1"]).unwrap(),
        (ops::MACHINES_CHECK, json!({"machine": "m1"}))
    );
    assert_eq!(read_call(&["peers"]).unwrap().0, ops::MACHINES_PEERS);
    assert!(read_call(&["add", "a@b"]).is_none());
}

#[test]
fn changes_become_one_request_with_the_consent_options() {
    let s = scanned(&[
        "add",
        "me@box",
        "--name",
        "box",
        "--reason",
        "need it",
        "--dry-run",
    ]);
    let body = request(&s).unwrap();
    assert_eq!(body["action"], "add");
    assert_eq!(body["args"], json!({"address": "me@box", "name": "box"}));
    assert_eq!(body["consent"]["reason"], "need it");
    assert_eq!(body["dryRun"], true);
}

#[test]
fn each_word_maps_to_an_action() {
    let one = |args: &[&str]| request(&scanned(args)).unwrap();
    assert_eq!(one(&["remove", "m1"])["action"], "remove");
    assert_eq!(
        one(&["fix", "m1", "stays-awake"])["args"]["step"],
        "stays-awake"
    );
    assert_eq!(one(&["running", "m1", "off"])["args"]["on"], false);
    assert_eq!(one(&["import", "p", "--user", "me"])["args"]["user"], "me");
    assert_eq!(
        one(&["addresses", "m1", "a@x", "b@y"])["args"]["addresses"],
        json!(["a@x", "b@y"])
    );
    assert_eq!(one(&["network", "switch", "work"])["action"], "network");
    assert!(request(&scanned(&["running", "m1", "maybe"])).is_err());
    assert!(request(&scanned(&["import", "p"])).is_err());
    assert!(request(&scanned(&["bogus"])).is_err());
}

#[test]
fn a_check_waits_longer_than_the_other_reads() {
    use notchling_engine::machines::engine::CHEAP_TIMEOUT;
    use notchling_engine::machines::tailscale::LISTING_SECS;
    use notchling_local_api::CALL_TIMEOUT;
    assert!(read_timeout(ops::MACHINES_CHECK) >= std::time::Duration::from_secs(2 * CHEAP_TIMEOUT));
    for op in [ops::MACHINES_PEERS, ops::NETWORK_LIST] {
        assert!(
            read_timeout(op) >= std::time::Duration::from_secs(LISTING_SECS),
            "{op}"
        );
    }
    for op in [ops::MACHINES_LIST, ops::MACHINES_PLAN] {
        assert_eq!(read_timeout(op), CALL_TIMEOUT, "{op}");
    }
}
