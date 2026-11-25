use super::*;

const QUESTION: &str = "Which database?";

fn agy(id: &str, folder: &str, also: &[&str]) -> Target {
    Target {
        tool: "antigravity",
        also: also.iter().map(|a| a.to_string()).collect(),
        ..target(id, folder)
    }
}

fn items() -> Value {
    json!([{"header": "DB", "question": QUESTION, "options": ["Postgres", "SQLite"],
        "details": [], "multiSelect": false}])
}

fn dialog() -> Value {
    json!({"title": "Switch model?", "body": [],
        "options": [{"number": 1, "label": "Yes, switch", "selected": true},
                    {"number": 2, "label": "No, keep it", "selected": false}]})
}

fn with_screen(r: &Rig) {
    *r.dialog.lock().unwrap() = dialog();
    *r.question.lock().unwrap() = json!({"items": items()});
}

fn aimed_at(id: &str) -> Vec<(&'static str, Value)> {
    vec![
        (
            ops::SESSION_CHOOSE,
            json!({"id": id, "answers": [[2]], "question": {"items": items()}}),
        ),
        (
            ops::SESSION_RUN,
            json!({"id": id, "command": "/compact keep the tests"}),
        ),
        (ops::SESSION_PICK, json!({"id": id, "number": 1})),
        (ops::SESSION_PICK, json!({"id": id, "dismiss": true})),
        (ops::SESSION_BROWSE, json!({"id": id, "on": true})),
        (ops::SESSION_BROWSE, json!({"id": id, "on": false})),
    ]
}

fn acted(r: &Rig) -> Vec<String> {
    r.ops_run()
        .into_iter()
        .filter(|op| op != ops::SESSION_DIALOG && op != ops::SESSION_QUESTION)
        .collect()
}

fn change_of(r: &Rig, id: &str) -> Value {
    r.consent.store().get(id).unwrap().unwrap().change
}

fn fresh(tag: &str) -> Rig {
    let r = rig(tag);
    with_screen(&r);
    r.make_child("kid", "parent");
    r.dir
        .sessions
        .lock()
        .unwrap()
        .insert("agy-kid".into(), agy("agy-kid", "/work/a", &[]));
    r.make_child("agy-kid", "parent");
    r.dir
        .sessions
        .lock()
        .unwrap()
        .insert("agy-self".into(), agy("agy-self", "/work/a", &["parent"]));
    r
}

fn lines_of(r: &Rig, id: &str) -> Vec<String> {
    r.consent
        .store()
        .get(id)
        .unwrap()
        .unwrap()
        .facts
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["line"].as_str().unwrap().to_string())
        .collect()
}

#[test]
fn every_act_takes_a_card_on_a_child_and_on_itself() {
    let mut n = 0;
    for id in ["kid", "parent"] {
        for (op, args) in aimed_at(id) {
            n += 1;
            let r = fresh(&format!("acts-card-{n}"));
            let reply = r.call(op, args.clone()).unwrap();
            let card = r.card_of(&reply);
            assert_eq!(reply["request"]["kind"], "session", "{op} {id}");
            assert!(reply.get("free").is_none(), "{op} {id}");
            assert_eq!(change_of(&r, &card)["session"], json!(id));
            assert!(acted(&r).is_empty(), "nothing is done before a yes");
            assert_eq!(r.cards(), 1);
        }
    }
    for id in ["agy-kid", "agy-self"] {
        n += 1;
        let r = fresh(&format!("acts-card-{n}"));
        let reply = r
            .call(
                ops::SESSION_TUNE,
                json!({"id": id, "option": "mode", "value": "accept-edits"}),
            )
            .unwrap();
        let card = r.card_of(&reply);
        assert_eq!(change_of(&r, &card)["message"], "mode = accept-edits");
        assert!(acted(&r).is_empty(), "nothing is done before a yes");
    }
}

#[test]
fn the_card_says_what_it_would_do_and_a_yes_does_it() {
    let cases: Vec<(&str, Value, &str, &str, Value)> = vec![
        (
            ops::SESSION_RUN,
            json!({"id": "kid", "command": "/compact keep the tests"}),
            "consent-session-run",
            "asks to run a command in name of kid",
            json!([ops::SESSION_COMMAND, {"id": "kid", "command": "/compact keep the tests"}]),
        ),
        (
            ops::SESSION_BROWSE,
            json!({"id": "kid", "on": true}),
            "consent-session-browse-on",
            "asks to turn Chrome on for name of kid",
            json!([ops::SESSION_COMMAND, {"id": "kid", "command": "/chrome"}]),
        ),
        (
            ops::SESSION_BROWSE,
            json!({"id": "kid", "on": false}),
            "consent-session-browse-off",
            "asks to turn Chrome off for name of kid",
            json!([ops::SESSION_COMMAND, {"id": "kid", "command": "/chrome off"}]),
        ),
        (
            ops::SESSION_PICK,
            json!({"id": "kid", "number": 2}),
            "consent-session-pick",
            "asks to pick an option in a dialog of name of kid",
            json!([ops::SESSION_DIALOG_PICK,
                {"id": "kid", "title": "Switch model?", "number": 2, "label": "No, keep it"}]),
        ),
        (
            ops::SESSION_PICK,
            json!({"id": "kid", "dismiss": true}),
            "consent-session-dismiss",
            "asks to dismiss a dialog in name of kid",
            json!([ops::SESSION_DIALOG_DISMISS, {"id": "kid"}]),
        ),
        (
            ops::SESSION_CHOOSE,
            json!({"id": "kid", "answers": [[2]], "question": {"items": items()}}),
            "consent-session-choose",
            "asks to answer a question of name of kid",
            json!([ops::SESSION_ANSWER,
                {"id": "kid", "answers": [[2]], "question": {"items": items()}}]),
        ),
        (
            ops::SESSION_TUNE,
            json!({"id": "agy-kid", "option": "mode", "value": "accept-edits"}),
            "consent-session-tune",
            "asks to change an option of name of agy-kid",
            json!([ops::AGY_SET_OPTION,
                {"id": "agy-kid", "option": "mode", "value": "accept-edits"}]),
        ),
    ];
    for (n, (op, args, line, title, done)) in cases.into_iter().enumerate() {
        let r = fresh(&format!("acts-apply-{n}"));
        let reply = r.call(op, args).unwrap();
        let id = r.card_of(&reply);
        assert_eq!(lines_of(&r, &id), [line]);
        let shown = r.item(&id).title;
        assert!(shown.ends_with(title), "{shown}");
        assert!(!shown.contains("billing") && !shown.contains("compact"));
        if line == "consent-session-pick" {
            assert_eq!(
                change_of(&r, &id)["message"],
                "Switch model?\n2. No, keep it"
            );
        }
        if line == "consent-session-choose" {
            assert_eq!(change_of(&r, &id)["message"], format!("{QUESTION}: SQLite"));
        }
        assert_eq!(r.allow(&id, "mac").state.as_str(), "applied");
        let calls = r.calls.lock().unwrap().clone();
        let did: Vec<Value> = calls
            .into_iter()
            .filter(|(op, _)| op != ops::SESSION_DIALOG && op != ops::SESSION_QUESTION)
            .map(|(op, args)| json!([op, args]))
            .collect();
        assert_eq!(did, [done], "{line}");
    }
}

#[test]
fn another_organization_is_refused_before_any_card() {
    let r = rig("acts-org");
    with_screen(&r);
    r.dir
        .sessions
        .lock()
        .unwrap()
        .insert("abroad-agy".into(), agy("abroad-agy", "/work/b", &[]));
    r.make_child("abroad", "parent");
    let mut attempts = aimed_at("abroad");
    attempts.push((
        ops::SESSION_TUNE,
        json!({"id": "abroad-agy", "option": "mode", "value": "accept-edits"}),
    ));
    for (op, args) in attempts {
        let e = r.call(op, args).unwrap_err();
        assert_eq!(e.code, ErrorCode::Forbidden, "{op}");
        assert_eq!(reason(&e), "other_organization", "{op}");
    }
    assert_eq!(r.cards(), 0);
    assert!(acted(&r).is_empty());
}

#[test]
fn text_that_loosens_permissions_is_never_before_any_card() {
    let r = rig("acts-never");
    with_screen(&r);
    r.dir
        .sessions
        .lock()
        .unwrap()
        .insert("agy-kid".into(), agy("agy-kid", "/work/a", &[]));
    let mut cases: Vec<(&str, Value)> = Vec::new();
    for command in [
        "/permissions",
        "/Permission-Mode plan",
        "/permissions allow Bash",
        "/model opus --dangerously-skip-permissions",
        "/config set defaultMode bypassPermissions",
    ] {
        cases.push((ops::SESSION_RUN, json!({"id": "kid", "command": command})));
    }
    cases.push((
        ops::SESSION_CHOOSE,
        json!({"id": "kid", "answers": ["bypassPermissions"], "question": {"items": items()}}),
    ));
    let loose = json!([{"question": "Mode?", "options": ["Ask first", "Bypass permissions"]}]);
    cases.push((
        ops::SESSION_CHOOSE,
        json!({"id": "kid", "answers": [[2]], "question": {"items": loose}}),
    ));
    cases.push((
        ops::SESSION_TUNE,
        json!({"id": "agy-kid", "option": "permission-mode", "value": "ask"}),
    ));
    cases.push((
        ops::SESSION_TUNE,
        json!({"id": "agy-kid", "option": "mode", "value": "bypass"}),
    ));
    for (op, args) in cases {
        let e = r.call(op, args.clone()).unwrap_err();
        assert_eq!(e.code, ErrorCode::Forbidden, "{op} {args}");
        assert_eq!(reason(&e), "never", "{op} {args}");
    }
    *r.dialog.lock().unwrap() = json!({"title": "Plan ready", "body": [],
        "options": [{"number": 1, "label": "Yes, and bypass permissions", "selected": true}]});
    let pick = r
        .call(ops::SESSION_PICK, json!({"id": "kid", "number": 1}))
        .unwrap_err();
    assert_eq!(reason(&pick), "never");
    assert_eq!(r.cards(), 0);
    assert!(acted(&r).is_empty());
}

#[test]
fn a_plain_command_is_not_mistaken_for_one_that_loosens() {
    let r = rig("acts-plain");
    for command in ["/compact", "/model sonnet", "/review the permissions page"] {
        let e = r.call(ops::SESSION_RUN, json!({"id": "kid", "command": command}));
        assert!(e.is_ok(), "{command}: {e:?}");
    }
    for command in ["compact", "/", "/two\nlines", ""] {
        let e = r
            .call(ops::SESSION_RUN, json!({"id": "kid", "command": command}))
            .unwrap_err();
        assert_eq!(e.code, ErrorCode::BadRequest, "{command:?}");
    }
}

#[test]
fn a_space_after_the_slash_is_no_command_and_cannot_hide_a_loosening_one() {
    let r = rig("acts-slash-space");
    for command in [
        "/ permissions allow Bash",
        "/  allowed-tools Bash",
        "/\tpermission-mode plan",
        "/\u{a0}permissions",
        "/ compact",
    ] {
        let e = r
            .call(ops::SESSION_RUN, json!({"id": "kid", "command": command}))
            .unwrap_err();
        assert_eq!(e.code, ErrorCode::BadRequest, "{command:?}");
    }
    assert_eq!(r.cards(), 0);
    assert!(acted(&r).is_empty());
    let loosening = super::super::acts::loosening_command;
    for command in [
        "/ permissions allow Bash",
        "/  allowed-tools Bash",
        "/Permissions",
    ] {
        assert!(loosening(command), "{command:?}");
    }
    assert!(!loosening("/ compact"));
}

#[test]
fn with_acting_off_each_act_says_to_switch_it_on() {
    let r = rig("acts-off");
    with_screen(&r);
    r.dir
        .sessions
        .lock()
        .unwrap()
        .insert("agy-kid".into(), agy("agy-kid", "/work/a", &[]));
    r.acting.store(false, Ordering::SeqCst);
    let mut attempts = aimed_at("kid");
    attempts.push((
        ops::SESSION_TUNE,
        json!({"id": "agy-kid", "option": "mode", "value": "accept-edits"}),
    ));
    for (op, args) in attempts {
        let e = r.call(op, args).unwrap_err();
        assert_eq!(e.code, ErrorCode::Forbidden, "{op}");
        assert_eq!(reason(&e), "switch_off", "{op}");
        assert_eq!(
            field(&e, "ask"),
            json!({"command": "notchling switch on acting", "kind": "switch"})
        );
    }
    assert_eq!(r.cards(), 0);
}

#[test]
fn an_act_on_the_wrong_kind_of_session_is_refused() {
    let r = rig("acts-kind");
    with_screen(&r);
    r.dir
        .sessions
        .lock()
        .unwrap()
        .insert("agy-kid".into(), agy("agy-kid", "/work/a", &[]));
    let e = r
        .call(
            ops::SESSION_RUN,
            json!({"id": "agy-kid", "command": "/compact"}),
        )
        .unwrap_err();
    assert_eq!(reason(&e), "not_yet");
    let e = r
        .call(
            ops::SESSION_TUNE,
            json!({"id": "kid", "option": "mode", "value": "accept-edits"}),
        )
        .unwrap_err();
    assert_eq!(reason(&e), "not_yet");
    assert_eq!(r.cards(), 0);
}

#[test]
fn the_text_is_wiped_when_the_card_closes_and_an_answer_is_never_shown_to_the_agent() {
    let r = rig("acts-wipe");
    with_screen(&r);
    let run = r
        .call(
            ops::SESSION_RUN,
            json!({"id": "kid", "command": "/compact keep billing notes"}),
        )
        .unwrap();
    let run = r.card_of(&run);
    let choose = r
        .call(
            ops::SESSION_CHOOSE,
            json!({"id": "kid", "answers": ["Use the billing db"], "question": {"items": items()}}),
        )
        .unwrap();
    let choose = r.card_of(&choose);
    let open = r.consent.store().get(&choose).unwrap().unwrap();
    assert!(r
        .consent
        .project(&open, false)
        .to_string()
        .contains("billing"));
    let agent_view = r.consent.project(&open, true).to_string();
    assert!(!agent_view.contains("billing"), "{agent_view}");
    assert!(!agent_view.contains(QUESTION), "{agent_view}");
    let open_run = r.consent.store().get(&run).unwrap().unwrap();
    assert!(r
        .consent
        .project(&open_run, true)
        .to_string()
        .contains("billing"));
    r.allow(&run, "mac");
    r.consent.decide(&choose, false, None, "mac", None).unwrap();
    for id in [&run, &choose] {
        let change = change_of(&r, id);
        assert_eq!(change.get("message"), None, "{change}");
        assert_eq!(change.get("question"), None, "{change}");
        let done = r.consent.store().get(id).unwrap().unwrap();
        assert!(!r
            .consent
            .project(&done, true)
            .to_string()
            .contains("billing"));
    }
}

#[test]
fn a_dialog_that_changed_since_the_card_withdraws_the_yes() {
    let r = rig("acts-dialog");
    with_screen(&r);
    let pick = r
        .call(ops::SESSION_PICK, json!({"id": "kid", "number": 1}))
        .unwrap();
    let pick = r.card_of(&pick);
    let dismiss = r
        .call(ops::SESSION_PICK, json!({"id": "kid", "dismiss": true}))
        .unwrap();
    let dismiss = r.card_of(&dismiss);
    let relabelled = r
        .call(ops::SESSION_PICK, json!({"id": "kid", "number": 2}))
        .unwrap();
    let relabelled = r.card_of(&relabelled);
    let mut other = dialog();
    other["options"][1]["label"] = json!("No, and stop");
    *r.dialog.lock().unwrap() = other;
    assert_eq!(r.allow(&relabelled, "mac").state.as_str(), "withdrawn");
    assert_eq!(r.allow(&pick, "mac").state.as_str(), "applied");
    let mut retitled = dialog();
    retitled["title"] = json!("Delete everything?");
    *r.dialog.lock().unwrap() = retitled;
    assert_eq!(r.allow(&dismiss, "mac").state.as_str(), "withdrawn");
    assert_eq!(acted(&r), [ops::SESSION_DIALOG_PICK]);

    *r.dialog.lock().unwrap() = Value::Null;
    let gone = r
        .call(ops::SESSION_PICK, json!({"id": "kid", "dismiss": true}))
        .unwrap_err();
    assert_eq!(gone.code, ErrorCode::BadRequest);
}

#[test]
fn a_question_that_changed_since_the_card_withdraws_the_yes() {
    let r = rig("acts-question");
    with_screen(&r);
    let asked = r
        .call(
            ops::SESSION_CHOOSE,
            json!({"id": "kid", "answers": [[1]], "question": {"items": items()}}),
        )
        .unwrap();
    let asked = r.card_of(&asked);
    *r.question.lock().unwrap() = json!({"items": [{"question": "Delete the repo?",
        "options": ["Postgres", "SQLite"]}]});
    assert_eq!(r.allow(&asked, "mac").state.as_str(), "withdrawn");
    let again = r
        .call(
            ops::SESSION_CHOOSE,
            json!({"id": "kid", "answers": [[1]], "question": {"items": items()}}),
        )
        .unwrap();
    let again = r.card_of(&again);
    *r.question.lock().unwrap() = json!({"items": []});
    assert_eq!(r.allow(&again, "mac").state.as_str(), "withdrawn");
    assert!(acted(&r).is_empty());
}

#[test]
fn bad_answers_and_missing_dialogs_are_usage_errors() {
    let r = rig("acts-usage");
    with_screen(&r);
    for answers in [
        json!([]),
        json!([[3]]),
        json!([[1, 2]]),
        json!(["a", "b"]),
        json!([[0]]),
    ] {
        let e = r
            .call(
                ops::SESSION_CHOOSE,
                json!({"id": "kid", "answers": answers, "question": {"items": items()}}),
            )
            .unwrap_err();
        assert_eq!(e.code, ErrorCode::BadRequest, "{answers}");
    }
    let heard = r
        .call(ops::SESSION_CHOOSE, json!({"id": "kid", "answers": [[1]]}))
        .unwrap();
    r.card_of(&heard);
    *r.question.lock().unwrap() = json!({"items": []});
    let none = r
        .call(ops::SESSION_CHOOSE, json!({"id": "kid", "answers": [[1]]}))
        .unwrap_err();
    assert_eq!(reason(&none), "no_question");
    for args in [
        json!({"id": "kid"}),
        json!({"id": "kid", "number": 1, "dismiss": true}),
        json!({"id": "kid", "number": 10}),
    ] {
        let e = r.call(ops::SESSION_PICK, args).unwrap_err();
        assert_eq!(e.code, ErrorCode::BadRequest);
    }
    let missing = r
        .call(ops::SESSION_PICK, json!({"id": "kid", "number": 7}))
        .unwrap_err();
    assert_eq!(reason(&missing), "bad_value");
    let e = r
        .call(ops::SESSION_BROWSE, json!({"id": "kid"}))
        .unwrap_err();
    assert_eq!(e.code, ErrorCode::BadRequest);
    let e = r
        .call(
            ops::SESSION_RUN,
            json!({"id": "nobody", "command": "/compact"}),
        )
        .unwrap_err();
    assert_eq!(reason(&e), "no_session");
}
