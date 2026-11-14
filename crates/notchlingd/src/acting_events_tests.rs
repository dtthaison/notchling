use crate::acting_ops_tests::{app, code, sessions, setup, setup_in, Setup, PROMPT, SESSION};
use crate::event_assert::{of, results, text};
use crate::late_emit::Events;
use notchling_engine::control::{FakeLauncher, LaunchOutput, Launcher};
use notchling_engine::sessions::Roots;
use notchling_events::envelope::{Actor, ActorTier};
use notchling_events::scope::Scope;
use notchling_events::{Device, MemorySink, Ref, Salt};
use notchling_local_api::{ops, Caller, ClientKind, ErrorCode, Tier};
use serde_json::{json, Value};
use std::sync::Arc;

fn watched(on: bool, screen: &[&str]) -> (Setup, Arc<MemorySink>) {
    let mut s = setup(on, screen, sessions());
    let sink = Arc::new(MemorySink::new());
    s.ops = s
        .ops
        .with_events(Events::fixed(sink.clone(), Salt([1; 32])));
    (s, sink)
}

fn agent() -> Caller {
    Caller {
        tier: Tier::Agent,
        client: ClientKind::Cli,
        peer: None,
        pid: None,
    }
}

fn phone(device: &str) -> Scope {
    Scope {
        op_id: None,
        actor: Some(Actor {
            tier: ActorTier::Phone,
            via: None,
            exe: None,
            device: Some(Ref::<Device>::new(device)),
            session: None,
        }),
    }
}

fn reply(s: &Setup, id: &str, text: &str) -> Result<Value, notchling_local_api::ApiError> {
    s.ops
        .call(&app(), ops::SESSION_REPLY, &json!({"id": id, "text": text}))
}

#[test]
fn reply_records_chars_not_text() {
    let (s, sink) = watched(true, PROMPT);
    let secret = "marker-text-4711";
    reply(&s, "job-idle", secret).unwrap();
    let sent = of(&sink, "session.reply");
    assert_eq!(sent.len(), 1);
    assert_eq!(text(&sent[0], "chars").as_deref(), Some("16"));
    assert_eq!(text(&sent[0], "via").as_deref(), Some("chat"));
    assert_eq!(text(&sent[0], "result").as_deref(), Some("sent"));
    assert_eq!(text(&sent[0], "images").as_deref(), Some("0"));
    assert!(!format!("{:?}", sink.events()).contains(secret));
}

#[test]
fn a_reply_from_the_notch_and_a_phone_say_so() {
    let (s, sink) = watched(true, PROMPT);
    s.ops
        .call(
            &app(),
            ops::SESSION_REPLY,
            &json!({"id": "job-idle", "text": "a", "from": "notch"}),
        )
        .unwrap();
    let (s, phone_sink) = watched(true, PROMPT);
    phone("d1").run(|| reply(&s, "job-idle", "b").unwrap());
    assert_eq!(
        text(&of(&sink, "session.reply")[0], "via").as_deref(),
        Some("notch")
    );
    assert_eq!(
        text(&of(&phone_sink, "session.reply")[0], "via").as_deref(),
        Some("phone")
    );
}

#[test]
fn reply_failure_maps_why() {
    let (s, sink) = watched(true, &["Do you want to proceed?", " 1. Yes"]);
    let e = reply(&s, "job-idle", "1").unwrap_err();
    assert_eq!(e.code, ErrorCode::Failed);
    let sent = of(&sink, "session.reply");
    assert_eq!(text(&sent[0], "result").as_deref(), Some("failed"));
    assert_eq!(text(&sent[0], "why").as_deref(), Some("dialog_open"));
}

#[test]
fn a_session_that_takes_no_replies_is_other() {
    let (s, sink) = watched(true, PROMPT);
    reply(&s, "term", "hi").unwrap_err();
    let sent = of(&sink, "session.reply");
    assert_eq!(text(&sent[0], "why").as_deref(), Some("other"));
}

#[test]
fn a_reply_while_acting_is_off_is_recorded_as_acting_off() {
    let (s, sink) = watched(false, PROMPT);
    assert_eq!(
        code(reply(&s, "job-idle", "hi")).0,
        notchling_local_api::ErrorCode::Forbidden
    );
    let sent = of(&sink, "session.reply");
    assert_eq!(text(&sent[0], "why").as_deref(), Some("acting_off"));
}

#[test]
fn a_reply_to_a_session_that_does_not_exist_never_records_the_id_it_was_given() {
    let (s, sink) = watched(false, PROMPT);
    reply(&s, "made-up-id-9d2e", "hi").unwrap_err();
    let sent = of(&sink, "session.reply");
    assert_eq!(text(&sent[0], "session").as_deref(), Some("unknown"));
    assert!(!format!("{:?}", sink.events()).contains("made-up-id-9d2e"));
}

#[test]
fn agent_reply_commits_first_and_fails_closed() {
    let (s, sink) = watched(true, PROMPT);
    sink.fail_commits(true);
    let e = s
        .ops
        .call(
            &agent(),
            ops::SESSION_REPLY,
            &json!({"id": "job-idle", "text": "hi"}),
        )
        .unwrap_err();
    assert_eq!(e.code, ErrorCode::Failed);
    assert_eq!(e.message, "the audit log can't be written");
    assert!(s.writes.lock().unwrap().is_empty(), "nothing typed");
    assert!(s.attached.lock().unwrap().is_empty());
    sink.fail_commits(false);
    s.ops
        .call(
            &agent(),
            ops::SESSION_REPLY,
            &json!({"id": "job-idle", "text": "hi"}),
        )
        .unwrap();
    assert_eq!(sink.commits(), 1);
    assert_eq!(results(&sink, "session.reply"), ["sent"]);
    assert_eq!(
        text(&of(&sink, "session.reply")[0], "via").as_deref(),
        Some("agent")
    );
}

#[test]
fn permission_answer_records_session_answered() {
    let (s, sink) = watched(true, PROMPT);
    let event = json!({"hook_event_name": "PermissionRequest", "session_id": SESSION,
        "tool_name": "Bash", "cwd": "/w", "tool_input": {"command": "ls"}});
    let (prompt, _rx) = s.ops.prompts().open(event, None, 1).unwrap();
    phone("d9").run(|| {
        s.ops
            .call(
                &app(),
                ops::SESSION_PERMISSION,
                &json!({"id": prompt.id, "decision": "always"}),
            )
            .unwrap()
    });
    let answered = of(&sink, "session.answered");
    assert_eq!(answered.len(), 1);
    assert_eq!(text(&answered[0], "prompt").as_deref(), Some("permission"));
    assert_eq!(text(&answered[0], "tool").as_deref(), Some("Bash"));
    assert_eq!(
        text(&answered[0], "answer").as_deref(),
        Some("allow_always")
    );
    assert_eq!(text(&answered[0], "by").as_deref(), Some("phone"));
    assert_eq!(text(&answered[0], "device").as_deref(), Some("d9"));
    assert_eq!(text(&answered[0], "session").as_deref(), Some(SESSION));
}

#[test]
fn held_then_sent_records_reply_held_and_reply_via_held() {
    let (s, sink) = watched(true, PROMPT);
    let held = reply(&s, "job-a", "two").unwrap();
    assert_eq!(held["state"], "queued");
    assert_eq!(sink.names(), ["session.reply_held"]);
    assert_eq!(
        text(&of(&sink, "session.reply_held")[0], "then").as_deref(),
        Some("sent_at_turn_end")
    );
    {
        let mut list = s.ops.sessions.lock().unwrap();
        for row in list.as_array_mut().unwrap() {
            if row["id"] == json!("job-a") {
                row["group"] = json!("completed");
            }
        }
    }
    s.ops.flush_held(5_000);
    let sent = of(&sink, "session.reply");
    assert_eq!(sent.len(), 1);
    assert_eq!(text(&sent[0], "via").as_deref(), Some("held"));
    assert_eq!(text(&sent[0], "chars").as_deref(), Some("3"));
}

#[test]
fn set_acting_records_switch_changed() {
    let (s, sink) = watched(false, PROMPT);
    s.ops
        .call(&app(), ops::SETTINGS_SET_ACTING, &json!({"on": true}))
        .unwrap();
    s.ops
        .call(
            &app(),
            ops::SETTINGS_SET_ACTING,
            &json!({"on": false, "restored": true}),
        )
        .unwrap();
    let changed = of(&sink, "switch.changed");
    assert_eq!(changed.len(), 2);
    assert_eq!(text(&changed[0], "switch").as_deref(), Some("acting"));
    assert_eq!(text(&changed[0], "cause").as_deref(), Some("user"));
    assert_eq!(text(&changed[1], "on").as_deref(), Some("false"));
    assert_eq!(
        text(&changed[1], "cause").as_deref(),
        Some("restored_at_launch")
    );
}

#[test]
fn interrupt_records_session_control() {
    let (s, sink) = watched(true, PROMPT);
    s.ops
        .call(&app(), ops::SESSION_INTERRUPT, &json!({"id": "job-a"}))
        .unwrap();
    let control = of(&sink, "session.control");
    assert_eq!(text(&control[0], "action").as_deref(), Some("interrupt"));
    assert_eq!(text(&control[0], "result").as_deref(), Some("done"));
}

fn controlled() -> (Setup, Arc<MemorySink>, Arc<FakeLauncher>) {
    let dir = std::env::temp_dir().join(format!("nl-acting-events-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("lanes.json"),
        r#"[{"name":"b","path":"/lanes/b","accountID":"acc-b","email":"b@example.com"}]"#,
    )
    .unwrap();
    let roots = Roots {
        main_dir: dir.join("claude"),
        lanes_file: dir.join("lanes.json"),
    };
    let fake = Arc::new(FakeLauncher::new());
    let launcher: Arc<dyn Launcher> = fake.clone();
    let mut s = setup_in(true, PROMPT, sessions(), roots, Some(launcher));
    let sink = Arc::new(MemorySink::new());
    s.ops = s
        .ops
        .with_events(Events::fixed(sink.clone(), Salt([1; 32])));
    (s, sink, fake)
}

#[test]
fn control_new_records_session_control() {
    let (s, sink, fake) = controlled();
    fake.push_output(Ok(LaunchOutput::ok(
        "backgrounded · 6ce4c19c · api (idle)\n",
    )));
    s.ops
        .call(
            &app(),
            ops::SESSION_NEW,
            &json!({"folder": "/w/api", "config_dir": "/lanes/b"}),
        )
        .unwrap();
    fake.push_output(Ok(LaunchOutput::failed(1, "no such job")));
    s.ops
        .call(&app(), ops::SESSION_STOP, &json!({"id": "job-a"}))
        .unwrap_err();
    let control = of(&sink, "session.control");
    assert_eq!(control.len(), 2);
    assert_eq!(text(&control[0], "action").as_deref(), Some("new"));
    assert_eq!(text(&control[0], "folder").as_deref(), Some("/w/api"));
    assert_eq!(text(&control[0], "account").as_deref(), Some("acc-b"));
    assert_eq!(text(&control[0], "result").as_deref(), Some("done"));
    assert_eq!(text(&control[1], "action").as_deref(), Some("stop"));
    assert_eq!(text(&control[1], "result").as_deref(), Some("failed"));
}

#[test]
fn a_slash_command_is_recorded_by_its_word_only() {
    let (s, sink) = watched(true, PROMPT);
    s.ops
        .call(
            &app(),
            ops::SESSION_COMMAND,
            &json!({"id": "job-idle", "command": "/compact keep this private"}),
        )
        .unwrap();
    let control = of(&sink, "session.control");
    assert_eq!(
        text(&control[0], "action").as_deref(),
        Some("slash_command")
    );
    assert_eq!(text(&control[0], "command").as_deref(), Some("compact"));
    assert!(!format!("{:?}", sink.events()).contains("keep this private"));
}

#[test]
fn an_agent_control_commits_first() {
    let (s, sink) = watched(true, PROMPT);
    sink.fail_commits(true);
    let e = s
        .ops
        .call(&agent(), ops::SESSION_INTERRUPT, &json!({"id": "job-a"}))
        .unwrap_err();
    assert_eq!(e.message, "the audit log can't be written");
    assert!(s.writes.lock().unwrap().is_empty());
}
