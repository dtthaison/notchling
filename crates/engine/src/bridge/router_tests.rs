//! Port of `BridgeRouterTests` (the router, the hub and the wire format).

use super::backend::Devices;
use super::model::*;
use super::router::{validated_folder, MAX_TEXT_CHARACTERS};
use super::test_support::*;
use notchling_protocol::generated::ChatRole;
use serde_json::{json, Value};
use std::sync::Arc;

fn codes(io: &RecordingIo) -> Option<String> {
    error_code(&io.last_json()).map(String::from)
}

// MARK: Auth gating

#[test]
fn nothing_is_served_before_hello() {
    let rig = Rig::new();
    let (connection, io) = rig.connect("192.168.1.20");
    rig.send(&connection, r#"{"id":7,"op":"sessions.list"}"#);
    let reply = io.json(0);
    assert_eq!(error_code(&reply), Some("unauthorized"));
    assert_eq!(reply["id"], json!(7));
    assert!(io.is_closed());
    assert_eq!(
        rig.backend.with(|s| s.session_calls),
        0,
        "the backend was never asked"
    );
}

#[test]
fn garbage_before_auth_closes_the_connection() {
    let rig = Rig::new();
    let (connection, io) = rig.connect("192.168.1.20");
    rig.send(&connection, "not json");
    assert_eq!(error_code(&io.json(0)), Some("invalid"));
    assert!(io.is_closed());
}

#[test]
fn hello_with_a_wrong_token_is_rejected_and_the_right_one_works() {
    let rig = Rig::new();
    let (_, token) = rig.devices.register("phone", "ios").expect("register");
    let (bad, bad_io) = rig.connect("192.168.1.20");
    rig.send(&bad, r#"{"id":"a","op":"hello","token":"nope"}"#);
    assert_eq!(error_code(&bad_io.json(0)), Some("unauthorized"));
    assert_eq!(bad_io.json(0)["id"], json!("a"));
    assert!(bad_io.is_closed());

    let (good, good_io) = rig.connect("192.168.1.21");
    rig.send(
        &good,
        &format!(r#"{{"id":1,"op":"hello","token":"{token}"}}"#),
    );
    assert!(is_ok(&good_io.json(0)));
    assert!(good.is_authenticated());
    rig.send(&good, r#"{"id":2,"op":"sessions.list"}"#);
    assert!(is_ok(&good_io.json(1)));
}

#[test]
fn five_bad_hellos_lock_the_address() {
    let rig = Rig::new();
    for _ in 0..5 {
        let (connection, _) = rig.connect("192.168.1.50");
        rig.send(&connection, r#"{"id":1,"op":"hello","token":"bad"}"#);
    }
    let (locked, io) = rig.connect("192.168.1.50");
    rig.send(&locked, r#"{"id":1,"op":"hello","token":"bad"}"#);
    assert!(
        io.texts().is_empty(),
        "a locked address gets silence, then the door"
    );
    assert!(io.is_closed());

    let (other, other_io) = rig.connect("192.168.1.51");
    rig.send(&other, r#"{"id":1,"op":"hello","token":"bad"}"#);
    assert_eq!(
        error_code(&other_io.json(0)),
        Some("unauthorized"),
        "another address is unaffected"
    );
}

#[test]
fn pair_with_the_code_needs_approval_and_returns_the_token_once() {
    let rig = Rig::new();
    let code = rig.codes.issue(NOW_MS);
    let (connection, io) = rig.connect("192.168.1.20");
    let pair = format!(
        r#"{{"id":1,"op":"pair","code":"{code}","device":{{"name":"p4-test","platform":"ios"}}}}"#
    );
    rig.send(&connection, &pair);
    let reply = io.json(0);
    assert!(is_ok(&reply), "{reply}");
    let token = reply["data"]["token"].as_str().expect("token").to_string();
    assert_eq!(rig.backend.with(|s| s.approval_calls), 1);
    assert_eq!(rig.devices.names(), ["p4-test"]);
    assert!(rig.devices.authenticate(&token).is_some());
    assert!(connection.is_authenticated());

    let (second, second_io) = rig.connect("192.168.1.20");
    rig.send(&second, &pair.replace("p4-test", "again"));
    assert_eq!(
        error_code(&second_io.json(0)),
        Some("unauthorized"),
        "the code was one-time"
    );
    assert_eq!(rig.devices.names().len(), 1);
    assert_eq!(rig.audit.recent()[0].op, "pair");
}

#[test]
fn pair_denied_on_the_computer_registers_nothing() {
    let rig = Rig::new();
    rig.backend.with(|s| s.refuse_approval = true);
    let code = rig.codes.issue(NOW_MS);
    let (connection, io) = rig.connect("192.168.1.20");
    rig.send(
        &connection,
        &format!(
            r#"{{"id":1,"op":"pair","code":"{code}","device":{{"name":"x","platform":"ios"}}}}"#
        ),
    );
    assert_eq!(error_code(&io.json(0)), Some("unauthorized"));
    assert!(io.is_closed());
    assert!(rig.devices.names().is_empty());
}

#[test]
fn pair_with_a_wrong_or_expired_code_fails_without_asking() {
    let rig = Rig::new();
    let code = rig.codes.issue(NOW_MS);
    let (wrong, wrong_io) = rig.connect("192.168.1.20");
    rig.send(
        &wrong,
        r#"{"id":1,"op":"pair","code":"ZZZZZZZZ","device":{"name":"x","platform":"ios"}}"#,
    );
    assert_eq!(error_code(&wrong_io.json(0)), Some("unauthorized"));

    rig.set_clock(NOW_MS + 301_000.0);
    let (late, late_io) = rig.connect("192.168.1.60");
    rig.send(
        &late,
        &format!(
            r#"{{"id":1,"op":"pair","code":"{code}","device":{{"name":"x","platform":"ios"}}}}"#
        ),
    );
    assert_eq!(error_code(&late_io.json(0)), Some("unauthorized"));
    assert_eq!(rig.backend.with(|s| s.approval_calls), 0);
}

#[test]
fn a_revoked_device_is_disconnected_and_its_token_refused() {
    let rig = Rig::new();
    let (connection, io, token) = rig.login("test-phone");
    let device = connection.device().expect("device");
    rig.devices.revoke(&device.id);
    assert_eq!(rig.hub.disconnect(&device.id), 1);
    assert!(io.is_closed());

    let (again, again_io) = rig.connect("192.168.1.70");
    rig.send(
        &again,
        &format!(r#"{{"id":1,"op":"hello","token":"{token}"}}"#),
    );
    assert_eq!(error_code(&again_io.json(0)), Some("unauthorized"));
}

// MARK: Reads

#[test]
fn sessions_list_encodes_subagents_and_pending() {
    let rig = Rig::new();
    let mut first = session("s1", Control::ConsolePty, SessionState::NeedsInput);
    first.pending = Some(permission("r1", "s1"));
    rig.backend.with(|s| {
        s.sessions = vec![
            first,
            session("s2", Control::ViewOnly, SessionState::Running),
        ]
    });
    let (connection, io, _) = rig.login("test-phone");
    rig.send(&connection, r#"{"id":5,"op":"sessions.list"}"#);
    let sessions = data(&io.last_json())["sessions"].clone();
    assert_eq!(sessions.as_array().map(Vec::len), Some(2));
    assert_eq!(sessions[0]["state"], "needsInput");
    assert_eq!(sessions[0]["pending"]["requestId"], "r1");
    assert_eq!(sessions[1]["control"], "viewOnly");
}

#[test]
fn chat_get_validation_limit_and_before() {
    let rig = Rig::new();
    let chat: Vec<_> = (0..10)
        .map(|n| message(n, &format!("m{n}"), ChatRole::User))
        .collect();
    let before = chat[5].id.clone();
    rig.backend.with(|s| {
        s.sessions = vec![session("s1", Control::ConsolePty, SessionState::Running)];
        s.chat = chat;
    });
    let (connection, io, _) = rig.login("test-phone");

    rig.send(&connection, r#"{"id":1,"op":"chat.get"}"#);
    assert_eq!(codes(&io).as_deref(), Some("invalid"));
    rig.send(
        &connection,
        r#"{"id":2,"op":"chat.get","sessionId":"nope"}"#,
    );
    assert_eq!(codes(&io).as_deref(), Some("not_found"));

    rig.send(
        &connection,
        r#"{"id":3,"op":"chat.get","sessionId":"s1","limit":3}"#,
    );
    let page = data(&io.last_json());
    let texts: Vec<_> = page["messages"]
        .as_array()
        .expect("messages")
        .iter()
        .map(|m| m["text"].clone())
        .collect();
    assert_eq!(texts, [json!("m7"), json!("m8"), json!("m9")]);
    assert_eq!(page["hasMore"], true);

    rig.send(
        &connection,
        &format!(r#"{{"id":4,"op":"chat.get","sessionId":"s1","limit":999,"before":"{before}"}}"#),
    );
    let page = data(&io.last_json());
    assert_eq!(
        page["messages"].as_array().map(Vec::len),
        Some(5),
        "limit is capped at 200, and `before` cuts the list"
    );
    assert_eq!(page["hasMore"], false);

    rig.send(
        &connection,
        r#"{"id":5,"op":"chat.get","sessionId":"s1","before":"missing"}"#,
    );
    assert_eq!(codes(&io).as_deref(), Some("not_found"));
}

#[test]
fn viz_get_returns_the_suggestion_and_web_url() {
    let rig = Rig::new();
    let mut viz = super::fixtures_data::viz(false);
    viz.suggested_tab = "web".into();
    viz.web_url = Some("http://localhost:5180".into());
    viz.web_candidates.clear();
    viz.specs.remove("diagram");
    rig.backend.with(|s| {
        s.sessions = vec![session("s1", Control::ConsolePty, SessionState::Running)];
        s.viz = Some(viz);
    });
    let (connection, io, _) = rig.login("test-phone");
    rig.send(&connection, r#"{"id":1,"op":"viz.get","sessionId":"s1"}"#);
    let answer = data(&io.last_json());
    assert_eq!(answer["suggested"], "web");
    assert_eq!(answer["webURL"], "http://localhost:5180");
    let mut keys: Vec<_> = answer
        .as_object()
        .expect("object")
        .keys()
        .cloned()
        .collect();
    keys.sort();
    assert_eq!(
        keys,
        ["chart", "flow", "suggested", "webURL"],
        "no entity unless asked"
    );
}

#[test]
fn web_snapshot_rejects_remote_urls_and_sends_json_then_binary() {
    let rig = Rig::new();
    rig.backend
        .with(|s| s.sessions = vec![session("s1", Control::ConsolePty, SessionState::Running)]);
    let (connection, io, _) = rig.login("test-phone");
    for url in [
        "https://example.com",
        "http://169.254.169.254/",
        "http://localhost@evil.example/",
        "ftp://localhost/",
    ] {
        rig.send(
            &connection,
            &format!(r#"{{"id":1,"op":"web.snapshot","sessionId":"s1","url":"{url}"}}"#),
        );
        assert_eq!(codes(&io).as_deref(), Some("invalid"), "{url}");
    }
    rig.send(&connection, r#"{"id":3,"op":"web.snapshot","sessionId":"s1","url":"http://localhost:3000","width":400}"#);
    let frames = io.binaries();
    assert_eq!(frames.len(), 1);
    assert_eq!(&frames[0][..4], b"WEB1");
    assert_eq!(&frames[0][4..8], [0, 0, 0, 3], "request id, big-endian");
    assert_eq!(&frames[0][8..], JPEG);
    let texts = io.texts();
    let announce: Value = serde_json::from_str(&texts[texts.len() - 2]).expect("json");
    assert_eq!(announce["data"]["binary"], true);
}

// MARK: Writes

#[test]
fn a_message_to_a_view_only_session_never_reaches_the_backend() {
    let rig = Rig::new();
    rig.backend
        .with(|s| s.sessions = vec![session("v", Control::ViewOnly, SessionState::Running)]);
    let (connection, io, _) = rig.login("test-phone");
    rig.send(
        &connection,
        r#"{"id":1,"op":"session.message","sessionId":"v","text":"hello"}"#,
    );
    assert_eq!(codes(&io).as_deref(), Some("view_only"));
    assert!(rig.backend.with(|s| s.sent.is_empty()));
    assert!(
        rig.audit.recent().is_empty(),
        "a refused message is not an executed one"
    );
}

#[test]
fn a_message_to_an_owned_session_is_sent_and_audited_without_its_text() {
    let rig = Rig::new();
    rig.backend.with(|s| {
        s.sessions = vec![
            session("p", Control::ConsolePty, SessionState::Running),
            session("j", Control::Job("abc".into()), SessionState::Running),
        ]
    });
    let (connection, io, _) = rig.login("test-phone");
    let secret = "run the secret migration";
    rig.send(
        &connection,
        &format!(r#"{{"id":1,"op":"session.message","sessionId":"p","text":"{secret}"}}"#),
    );
    assert!(is_ok(&io.last_json()));
    rig.send(
        &connection,
        r#"{"id":2,"op":"session.message","sessionId":"j","text":"to the job"}"#,
    );
    let targets: Vec<String> = rig
        .backend
        .with(|s| s.sent.iter().map(|x| x.0.clone()).collect());
    assert_eq!(targets, ["p", "j"]);

    let entries = rig.audit.recent();
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[1].op, "message");
    assert_eq!(entries[1].length, Some(secret.chars().count()));
    assert_eq!(entries[1].device, "test-phone");
    assert!(!format!("{entries:?}").contains(secret));
}

#[test]
fn message_validation() {
    let rig = Rig::new();
    rig.backend
        .with(|s| s.sessions = vec![session("p", Control::ConsolePty, SessionState::Running)]);
    let (connection, io, _) = rig.login("test-phone");
    for body in [
        r#"{"id":1,"op":"session.message","sessionId":"p"}"#,
        r#"{"id":1,"op":"session.message","sessionId":"p","text":""}"#,
        r#"{"id":1,"op":"session.message","text":"hi"}"#,
    ] {
        rig.send(&connection, body);
        assert_eq!(codes(&io).as_deref(), Some("invalid"), "{body}");
    }
    rig.send(
        &connection,
        r#"{"id":2,"op":"session.message","sessionId":"ghost","text":"hi"}"#,
    );
    assert_eq!(codes(&io).as_deref(), Some("not_found"));
    let huge = "x".repeat(MAX_TEXT_CHARACTERS + 1);
    rig.send(
        &connection,
        &format!(r#"{{"id":3,"op":"session.message","sessionId":"p","text":"{huge}"}}"#),
    );
    assert_eq!(codes(&io).as_deref(), Some("invalid"));
    assert!(rig.backend.with(|s| s.sent.is_empty()));
}

#[test]
fn a_second_message_to_the_same_session_while_one_is_sending_is_busy() {
    let rig = Arc::new(Rig::new());
    rig.backend.with(|s| {
        s.sessions = vec![session("p", Control::ConsolePty, SessionState::Running)];
        s.send_delay_ms = 300;
    });
    let (connection, io, _) = rig.login("test-phone");
    let first = {
        let (rig, connection) = (Arc::clone(&rig), Arc::clone(&connection));
        std::thread::spawn(move || {
            rig.send(
                &connection,
                r#"{"id":1,"op":"session.message","sessionId":"p","text":"one"}"#,
            )
        })
    };
    std::thread::sleep(std::time::Duration::from_millis(100));
    rig.send(
        &connection,
        r#"{"id":2,"op":"session.message","sessionId":"p","text":"two"}"#,
    );
    first.join().expect("first send");
    let replies: Vec<Value> = io
        .texts()
        .iter()
        .map(|t| serde_json::from_str(t).expect("json"))
        .collect();
    let by_id = |n: i64| {
        replies
            .iter()
            .find(|r| r["id"] == json!(n))
            .cloned()
            .expect("reply")
    };
    assert_eq!(error_code(&by_id(2)), Some("busy"));
    assert!(is_ok(&by_id(1)));
    let sent: Vec<String> = rig
        .backend
        .with(|s| s.sent.iter().map(|x| x.1.clone()).collect());
    assert_eq!(sent, ["one"]);
}

#[test]
fn prompt_answer_for_permission_and_questions() {
    let rig = Rig::new();
    let question = |id: &str, options: Vec<&str>, multi: bool| PendingPrompt {
        id: id.into(),
        session_id: "s1".into(),
        kind: PromptKind::Question(vec![PromptQuestion {
            header: "H".into(),
            question: "Q?".into(),
            options: options.into_iter().map(String::from).collect(),
            multi_select: multi,
        }]),
        tool_name: "AskUserQuestion".into(),
        summary: "Q?".into(),
        created_ms: NOW_MS,
    };
    rig.backend.with(|s| {
        s.prompts.insert("perm".into(), permission("perm", "s1"));
        s.prompts.insert(
            "ask".into(),
            question("ask", vec!["main", "develop"], false),
        );
        s.prompts
            .insert("multi".into(), question("multi", vec!["a"], true));
    });
    let (connection, io, _) = rig.login("test-phone");

    rig.send(
        &connection,
        r#"{"id":1,"op":"prompt.answer","requestId":"perm"}"#,
    );
    assert_eq!(codes(&io).as_deref(), Some("invalid"));
    rig.send(
        &connection,
        r#"{"id":2,"op":"prompt.answer","requestId":"perm","allow":false}"#,
    );
    assert!(is_ok(&io.last_json()));
    rig.send(
        &connection,
        r#"{"id":3,"op":"prompt.answer","requestId":"ask","choice":2}"#,
    );
    rig.send(
        &connection,
        r#"{"id":4,"op":"prompt.answer","requestId":"ask","choice":"main"}"#,
    );
    rig.send(
        &connection,
        r#"{"id":5,"op":"prompt.answer","requestId":"ask","text":"something else"}"#,
    );
    let responses: Vec<PromptResponse> = rig
        .backend
        .with(|s| s.responses.iter().map(|r| r.1.clone()).collect());
    assert_eq!(
        responses,
        [
            PromptResponse::Deny,
            PromptResponse::Answers(vec!["develop".into()]),
            PromptResponse::Answers(vec!["main".into()]),
            PromptResponse::Answers(vec!["something else".into()]),
        ]
    );
    for body in [
        r#"{"id":6,"op":"prompt.answer","requestId":"ask","choice":3}"#,
        r#"{"id":7,"op":"prompt.answer","requestId":"ask","choice":"nope"}"#,
        r#"{"id":8,"op":"prompt.answer","requestId":"ask"}"#,
        r#"{"id":9,"op":"prompt.answer","requestId":"multi","choice":1}"#,
        r#"{"id":10,"op":"prompt.answer","requestId":"ask","allow":true}"#,
    ] {
        rig.send(&connection, body);
        assert_eq!(codes(&io).as_deref(), Some("invalid"), "{body}");
    }
    rig.send(
        &connection,
        r#"{"id":11,"op":"prompt.answer","requestId":"gone","allow":true}"#,
    );
    assert_eq!(codes(&io).as_deref(), Some("not_found"));
    assert_eq!(
        rig.audit
            .recent()
            .iter()
            .filter(|e| e.op == "answer")
            .count(),
        4
    );
}

#[test]
fn session_new_checks_the_folder_and_model() {
    let rig = Rig::new();
    let home = rig.home();
    let project = home.join("work/app");
    std::fs::create_dir_all(&project).expect("project");
    std::fs::write(home.join("file.txt"), "x").expect("file");
    let outside = rig.dir.path("outside");
    std::fs::create_dir_all(&outside).expect("outside");
    #[cfg(unix)]
    std::os::unix::fs::symlink(&outside, home.join("escape")).expect("symlink");
    let (connection, io, _) = rig.login("test-phone");

    let call = |cwd: &str, model: Option<&str>| {
        let model = model
            .map(|m| format!(r#","model":"{m}""#))
            .unwrap_or_default();
        rig.send(
            &connection,
            &format!(r#"{{"id":1,"op":"session.new","cwd":"{cwd}"{model}}}"#),
        );
        io.last_json()
    };
    let path = |p: &std::path::Path| p.to_string_lossy().into_owned();
    let home_text = path(&home);
    let mut cases = vec![
        ("invalid", "relative/path".to_string(), None),
        ("invalid", path(&outside), None),
        ("invalid", format!("{home_text}/../elsewhere"), None),
        ("not_found", format!("{home_text}/missing"), None),
        ("not_found", path(&home.join("file.txt")), None),
        ("invalid", path(&project), Some("gpt")),
    ];
    if cfg!(unix) {
        cases.push(("invalid", format!("{home_text}/escape"), None));
    }
    for (code, cwd, model) in cases {
        assert_eq!(error_code(&call(&cwd, model)), Some(code), "{cwd}");
    }
    assert!(rig.backend.with(|s| s.new_sessions.is_empty()));

    for model in [None, Some("default"), Some("opus")] {
        assert!(is_ok(&call(&path(&project), model)));
    }
    let started: Vec<_> = rig.backend.with(|s| s.new_sessions.clone());
    assert!(started.iter().all(|s| s.0.ends_with("work/app")));
    let models: Vec<_> = started.into_iter().map(|s| s.1).collect();
    assert_eq!(models, [None, None, Some("opus".to_string())]);
    assert_eq!(
        rig.audit
            .recent()
            .iter()
            .filter(|e| e.op == "newSession")
            .count(),
        3
    );
    assert!(validated_folder("/", &home).is_err());
}

#[test]
fn projects_recent_and_unknown_ops() {
    let rig = Rig::new();
    let (connection, io, _) = rig.login("test-phone");
    rig.send(&connection, r#"{"id":1,"op":"projects.recent"}"#);
    assert_eq!(
        data(&io.last_json())["projects"],
        json!(["/Users/example/src/app", "/Users/example/src/notes"])
    );
    rig.send(&connection, r#"{"id":2,"op":"rm.everything"}"#);
    assert_eq!(codes(&io).as_deref(), Some("invalid"));
    rig.send(&connection, r#"{"id":3,"op":"ping"}"#);
    assert!(is_ok(&io.last_json()));
}
