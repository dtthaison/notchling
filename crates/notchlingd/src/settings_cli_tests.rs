use super::*;

fn args(words: &[&str]) -> Vec<String> {
    words.iter().map(|w| w.to_string()).collect()
}

#[test]
fn the_three_commands_parse_and_anything_else_is_usage() {
    assert_eq!(
        parse(&args(&["list", "--json"])),
        Ok(Args {
            sub: Sub::List,
            json: true,
            socket: None,
            flags: Flags::default(),
        })
    );
    assert_eq!(
        parse(&args(&["get", "chatHistory", "--socket", "/tmp/s"])).map(|a| (a.sub, a.socket)),
        Ok((
            Sub::Get("chatHistory".into()),
            Some(PathBuf::from("/tmp/s"))
        ))
    );
    assert_eq!(
        parse(&args(&["set", "chatHistory", "8mb"])).map(|a| a.sub),
        Ok(Sub::Set("chatHistory".into(), "8mb".into()))
    );
    assert!(parse(&args(&["set", "chatHistory"]))
        .unwrap_err()
        .starts_with("settings set needs a key and a value."));
    assert!(parse(&args(&["get"]))
        .unwrap_err()
        .starts_with("settings get needs a key."));
    for bad in [&[][..], &["remove", "x"], &["list", "extra"], &["--socket"]] {
        assert!(parse(&args(bad)).is_err(), "{bad:?}");
    }
}

fn reply() -> Value {
    json!({"settings": [
        {"key": "switch.acting", "title": "Act on Sessions", "kind": "bool", "class": "never",
         "switch": true, "value": true, "why": "a switch"},
        {"key": "chatHistory", "title": "Conversation history to load", "kind": "string",
         "class": "free", "value": "4mb", "default": "4mb",
         "allowed": ["2mb", "4mb", "8mb", "16mb"],
         "description": "How much of a long conversation Chat loads"},
        {"key": "limitSwitchAt", "title": "Switch at", "kind": "int", "class": "ask",
         "value": 95, "range": [80, 99]},
        {"key": "accounts", "title": "Claude Code accounts", "kind": "json", "class": "never",
         "why": "logins"}
    ]})
}

#[test]
fn the_list_groups_settings_by_who_may_change_them() {
    assert_eq!(
        list_lines(&reply()),
        [
            "You may change these:",
            "  chatHistory = 4mb   Conversation history to load (one of 2mb, 4mb, 8mb, 16mb)",
            "",
            "These ask the user first, in Notchling's Inbox:",
            "  limitSwitchAt = 95   Switch at (a whole number from 80 to 99)",
            "",
            "Only the user changes these, in Notchling:",
            "  switch.acting = on   Act on Sessions",
            "  accounts   Claude Code accounts",
        ]
    );
}

#[test]
fn one_setting_says_what_it_takes_and_who_may_change_it() {
    let list = reply();
    let history = &list["settings"][1];
    assert_eq!(
        entry_lines(history),
        [
            "chatHistory = 4mb",
            "Conversation history to load: How much of a long conversation Chat loads.",
            "Takes one of 2mb, 4mb, 8mb, 16mb. Default 4mb.",
            "Agents may change it.",
        ]
    );
    let mut ask = list["settings"][2].clone();
    ask["pending"] = json!([{"id": "setting-1", "value": 90}]);
    let lines = entry_lines(&ask);
    assert_eq!(
        lines[3],
        "Agents may ask; the user allows or denies it in Notchling's Inbox."
    );
    assert_eq!(lines[4], "Waiting for the user's approval: 90.");
    assert_eq!(
        entry_lines(&list["settings"][3])[3],
        "Agents can't change it: logins."
    );
}

#[test]
fn a_withdrawn_request_says_withdrawn_and_a_failed_one_says_failed() {
    for (state, reason) in [("withdrawn", "withdrawn"), ("failed", "failed")] {
        let reply = json!({"key": "theme", "value": "dark", "state": state, "message": "m"});
        let (code, text) = outcome(&reply);
        let failure = failure_of(&reply, code, text);
        assert_eq!(
            (failure.exit, failure.reason.as_str()),
            (1, reason),
            "{state}"
        );
        assert_eq!(failure.op.as_deref(), Some("settings.set"));
    }
}

#[test]
fn a_trouble_while_waiting_is_the_op_that_was_waiting() {
    let mut calls: Vec<String> = Vec::new();
    let mut call = |op: &str, _: Value, _: Duration| -> Result<Value, ClientError> {
        calls.push(op.to_string());
        match op {
            "settings.set" => Ok(json!({"state": "waiting", "id": "r1"})),
            _ => Err(ClientError::Timeout),
        }
    };
    let got = set(&mut call, ("theme", "dark"), &Flags::default(), true);
    let (op, error) = got.unwrap_err();
    assert_eq!(op, ops::SETTINGS_ASKED);
    assert!(matches!(error, ClientError::Timeout));
    assert_eq!(calls, ["settings.set", "settings.asked"]);

    let mut refused = |_: &str, _: Value, _: Duration| -> Result<Value, ClientError> {
        Err(ClientError::Timeout)
    };
    let (op, _) = set(&mut refused, ("theme", "dark"), &Flags::default(), true).unwrap_err();
    assert_eq!(op, ops::SETTINGS_SET);
}

#[test]
fn each_outcome_has_its_words_and_exit_code() {
    let base =
        |state: &str| json!({"key": "limitMode", "value": "off", "state": state, "message": "m"});
    assert_eq!(
        outcome(&base("applied")),
        (DONE, "Set limitMode to off.".into())
    );
    let mut same = base("applied");
    same["changed"] = json!(false);
    assert_eq!(outcome(&same), (DONE, "limitMode is already off.".into()));
    assert_eq!(outcome(&base("denied")).0, contract::DENIED);
    assert!(outcome(&base("expired"))
        .1
        .contains("didn't answer in time"));
    assert_eq!(outcome(&base("expired")).0, contract::EXPIRED);
    assert_eq!(outcome(&base("failed")), (contract::FAILED, "m".into()));
    assert_eq!(outcome(&base("withdrawn")), (contract::FAILED, "m".into()));
    assert_eq!(outcome(&base("waiting")).0, contract::PENDING);
}

#[test]
fn the_common_flags_are_read_and_sent_in_the_consent_object() {
    let parsed = parse(&args(&[
        "set", "limitMode", "off", "--reason", "asked", "--no-wait", "--timeout", "30",
        "--expires-in", "600", "--key", "k1", "--dry-run",
    ]))
    .unwrap();
    assert_eq!(parsed.flags.reason.as_deref(), Some("asked"));
    assert!(!parsed.flags.wait && parsed.flags.dry_run);
    assert_eq!(parsed.flags.timeout, Duration::from_secs(30));
    assert_eq!(parsed.flags.expires_in, Some(600));
    assert_eq!(parsed.flags.key.as_deref(), Some("k1"));
    assert!(parse(&args(&["set", "a", "b", "--timeout", "soon"])).is_err());

    let mut sent: Vec<Value> = Vec::new();
    let mut call = |_: &str, body: Value, _: Duration| -> Result<Value, ClientError> {
        sent.push(body);
        Ok(json!({"state": "waiting", "id": "c1"}))
    };
    let got = set(&mut call, ("limitMode", "off"), &parsed.flags, true).unwrap();
    assert_eq!(got["id"], "c1", "--no-wait returns the first reply at once");
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0]["dryRun"], true);
    assert_eq!(sent[0]["consent"]["reason"], "asked");
    assert_eq!(sent[0]["consent"]["expiresInMs"], 600_000);
    assert_eq!(sent[0]["consent"]["key"], "k1");
}
