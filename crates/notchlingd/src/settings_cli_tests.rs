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
            socket: None
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
    assert_eq!(outcome(&base("denied")).0, NOT_ALLOWED);
    assert!(outcome(&base("expired"))
        .1
        .contains("didn't answer in time"));
    assert_eq!(outcome(&base("expired")).0, NOT_ALLOWED);
    assert_eq!(outcome(&base("failed")), (REFUSED, "m".into()));
    assert_eq!(outcome(&base("withdrawn")), (TROUBLE, "m".into()));
    assert_eq!(outcome(&base("waiting")).0, NOT_ALLOWED);
}
