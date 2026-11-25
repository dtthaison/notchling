use super::*;

const A2: &str = "acct-2222222222222222";
const A3: &str = "acct-3333333333333333";
const LANE: &str = "lane-4444444444444444";

fn scanned(list: &[&str]) -> Scanned {
    let raw: Vec<String> = list.iter().map(|s| s.to_string()).collect();
    scan(&raw, USAGE, &[]).unwrap()
}

fn body(list: &[&str]) -> Result<Value, String> {
    request(&scanned(list))
}

fn lanes_body(list: &[&str]) -> Result<Value, String> {
    lanes_request(&scanned(list))
}

#[test]
fn each_word_becomes_an_action_with_its_args() {
    assert_eq!(
        body(&["switch", A2]).unwrap(),
        json!({"action": "switch", "args": {"account": A2}, "consent": {}})
    );
    for tool in ["claude", "codex", "agy"] {
        assert_eq!(body(&["add", tool]).unwrap()["args"], json!({"tool": tool}));
    }
    assert_eq!(
        body(&["remove", A2]).unwrap(),
        json!({"action": "remove", "args": {"account": A2}, "consent": {}})
    );
    assert_eq!(
        body(&["move-all", A3]).unwrap(),
        json!({"action": "move_all", "args": {"to": A3}, "consent": {}})
    );
    assert_eq!(
        body(&["move-all", "main"]).unwrap()["args"],
        json!({"to": "main"})
    );
    assert_eq!(
        body(&["move", "agent-7", LANE]).unwrap(),
        json!({"action": "move", "args": {"session": "agent-7", "to": LANE}, "consent": {}})
    );
    assert_eq!(
        body(&["move", "agent-7", "main"]).unwrap()["args"],
        json!({"session": "agent-7", "to": "main"})
    );
    assert_eq!(
        lanes_body(&["create", A2]).unwrap(),
        json!({"action": "lane_create", "args": {"account": A2}, "consent": {}})
    );
    assert_eq!(
        lanes_body(&["remove", LANE]).unwrap(),
        json!({"action": "lane_remove", "args": {"lane": LANE}, "consent": {}})
    );
    assert_eq!(
        lanes_body(&["switch", LANE, A2]).unwrap(),
        json!({"action": "lane_switch", "args": {"lane": LANE, "account": A2}, "consent": {}})
    );
}

#[test]
fn the_common_flags_go_in_the_consent_and_dry_run_on_the_body() {
    let flags = [
        "--reason",
        "needs the other login",
        "--key",
        "k1",
        "--expires-in",
        "60",
        "--dry-run",
    ];
    let mut list = vec!["switch", A2];
    list.extend(flags);
    let b = body(&list).unwrap();
    assert_eq!(b["dryRun"], json!(true));
    assert_eq!(
        b["consent"],
        json!({"reason": "needs the other login", "expiresInMs": 60_000, "key": "k1"})
    );
    let mut list = vec!["switch", LANE, A2];
    list.extend(flags);
    let b = lanes_body(&list).unwrap();
    assert_eq!(b["dryRun"], json!(true));
    assert_eq!(b["consent"]["key"], "k1");
    assert!(body(&["remove", A2]).unwrap().get("dryRun").is_none());
}

#[test]
fn a_bad_shape_is_usage() {
    assert_eq!(body(&["switch"]).unwrap_err(), USAGE);
    assert!(body(&["switch", "a", "b"]).is_err());
    assert!(body(&["add"]).is_err());
    assert!(body(&["add", "claude", "codex"]).is_err());
    assert!(body(&["remove"]).is_err());
    assert!(body(&["move-all"]).is_err());
    assert!(body(&["move", "agent-7"]).is_err());
    assert!(body(&["move", "a", "b", "c"]).is_err());
    assert!(body(&["list"]).is_err());
    let m = body(&["add", "gemini"]).unwrap_err();
    assert!(m.starts_with("The tool is claude, codex or agy."), "{m}");
    assert!(body(&["add", "antigravity"]).is_err());
    assert!(lanes_body(&["create"]).is_err());
    assert!(lanes_body(&["remove"]).is_err());
    assert!(lanes_body(&["switch", LANE]).is_err());
    assert!(lanes_body(&["list"]).is_err());
    let raw = vec!["switch".to_string(), A2.to_string(), "--bogus".to_string()];
    assert!(scan(&raw, USAGE, &[]).is_err());
}

#[test]
fn plain_accounts_stays_the_read() {
    for word in WORDS {
        assert!(handles(word), "{word}");
    }
    assert!(!handles("--json") && !handles("list") && !handles("") && !handles("create"));
}

#[test]
fn lanes_list_prints_the_reply() {
    let reply = json!({"organization": "acme", "lanes": [
        {"id": LANE, "account": A3, "name": "c***@w***.com", "sessions": 1, "needsLogin": false}]});
    let mut seen = Vec::new();
    let mut out = Vec::new();
    let code = lanes_list(
        &["list".to_string(), "--json".to_string()],
        |socket, op, args| {
            seen.push((socket.is_some(), op.to_string(), args));
            Ok(reply.clone())
        },
        &mut out,
    );
    assert_eq!(code, contract::OK);
    assert_eq!(seen, vec![(false, "lanes.summary".to_string(), json!({}))]);
    let text = String::from_utf8(out).unwrap();
    assert_eq!(serde_json::from_str::<Value>(&text).unwrap(), reply);
    assert!(text.contains('\n') && text.contains("  \"lanes\""));
}

#[test]
fn lanes_list_exits_0_on_a_no_session_reply_and_takes_no_other_words() {
    let reply = json!({"reason": "no_session", "lanes": []});
    let mut out = Vec::new();
    let code = lanes_list(&["list".to_string()], |_, _, _| Ok(reply.clone()), &mut out);
    assert_eq!(code, contract::OK);
    assert_eq!(
        serde_json::from_slice::<Value>(&out).unwrap()["reason"],
        "no_session"
    );
    for args in [
        vec!["list", "extra"],
        vec!["list", "--dry-run"],
        vec!["create"],
        vec![],
    ] {
        let raw: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        let mut called = false;
        let mut out = Vec::new();
        let code = lanes_list(
            &raw,
            |_, _, _| {
                called = true;
                Ok(json!({}))
            },
            &mut out,
        );
        assert_eq!(code, contract::BAD_REQUEST, "{args:?}");
        assert!(!called && out.is_empty());
    }
}

#[test]
fn lanes_list_help_prints_the_usage() {
    let mut out = Vec::new();
    let code = lanes_list(
        &["--help".to_string()],
        |_, _, _| panic!("no call"),
        &mut out,
    );
    assert_eq!(code, contract::OK);
    assert_eq!(String::from_utf8(out).unwrap().trim_end(), USAGE);
}

#[test]
fn the_done_line_names_the_words_and_the_new_account() {
    let line = |v: Value| done_line(&json!({"result": v}));
    assert_eq!(
        line(json!({"action": "switch", "done": true, "account": A2})),
        "Done: accounts switch (the user said yes)."
    );
    assert_eq!(
        line(json!({"action": "add", "done": true, "tool": "codex", "account": A2})),
        format!("Done: accounts add (the user said yes).\nNew account: {A2}")
    );
    assert_eq!(
        line(json!({"action": "add", "done": true, "tool": "claude", "account": null})),
        "Done: accounts add (the user said yes)."
    );
    assert_eq!(
        line(json!({"action": "remove", "done": true})),
        "Done: accounts remove (the user said yes)."
    );
    assert_eq!(
        line(json!({"action": "move_all", "done": true, "moved": 2, "errors": 0})),
        "Done: accounts move-all (the user said yes).\nMoved: 2, errors: 0"
    );
    assert_eq!(
        line(json!({"action": "move", "done": true})),
        "Done: accounts move (the user said yes)."
    );
    assert_eq!(
        line(json!({"action": "lane_create", "done": true, "lane": LANE})),
        format!("Done: lanes create (the user said yes).\nLane: {LANE}")
    );
    assert_eq!(
        line(json!({"action": "lane_remove", "done": true})),
        "Done: lanes remove (the user said yes)."
    );
    assert_eq!(
        line(json!({"action": "lane_switch", "done": true})),
        "Done: lanes switch (the user said yes)."
    );
}
