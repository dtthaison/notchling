//! Session control (E11) through the daemon's ops: the planners' commands reach a fake launcher, a
//! lane is named by its folder, and a slash command is typed like a reply unless it opens a menu.
//! Nothing is started: the launcher is `FakeLauncher` and the terminal the tests' fake.

use crate::acting_ops_tests::{app, code, sessions, setup_in, Setup, PROMPT, SESSION};
use notchling_engine::control::{
    plan_trust_folder, Call, Command, FakeLauncher, LaunchOutput, Launcher,
};
use notchling_engine::reply::keys;
use notchling_engine::sessions::Roots;
use notchling_local_api::{ops, ErrorCode};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::Arc;

struct Dir(PathBuf);

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A data folder whose `lanes.json` has one lane, `/lanes/b`.
fn roots(tag: &str) -> (Dir, Roots) {
    let dir = std::env::temp_dir().join(format!("nl-ctl-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
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
    (Dir(dir), roots)
}

fn control(tag: &str, on: bool) -> (Dir, Setup, Arc<FakeLauncher>) {
    let (dir, roots) = roots(tag);
    let fake = Arc::new(FakeLauncher::new());
    let launcher: Arc<dyn Launcher> = fake.clone();
    let s = setup_in(on, PROMPT, sessions(), roots, Some(launcher));
    (dir, s, fake)
}

fn call(s: &Setup, op: &str, args: Value) -> Result<Value, (ErrorCode, String)> {
    s.ops
        .call(&app(), op, &args)
        .map_err(|e| (e.code, e.message))
}

fn main_login(argv: &[&str]) -> Command {
    Command::new(argv).for_config_dir(None)
}

#[test]
fn nothing_runs_while_acting_is_off() {
    let (_d, s, fake) = control("off", false);
    for op in crate::acting_ops::CONTROL_OPS {
        let args = json!({"id": "job-a", "folder": "/w", "to_config_dir": "/lanes/b", "command": "/compact"});
        assert_eq!(
            code(s.ops.call(&app(), op, &args)).0,
            ErrorCode::Forbidden,
            "{op}"
        );
    }
    assert!(fake.calls().is_empty());
}

#[test]
fn stop_and_remove_run_claude_for_the_job() {
    let (_d, s, fake) = control("stop", true);
    assert_eq!(
        call(&s, ops::SESSION_STOP, json!({"id": "job-a"})),
        Ok(json!({"stopped": true}))
    );
    assert_eq!(
        call(&s, ops::SESSION_REMOVE, json!({"id": "job-a"})),
        Ok(json!({"removed": true}))
    );
    assert_eq!(
        fake.commands(),
        [
            main_login(&["claude", "stop", "abcd1234"]),
            main_login(&["claude", "rm", "abcd1234"])
        ]
    );
    // A terminal session isn't a job; an unknown id is no session.
    let (c, m) = call(&s, ops::SESSION_STOP, json!({"id": "term"})).unwrap_err();
    assert_eq!(c, ErrorCode::BadRequest);
    assert!(m.contains("only a background session"), "{m}");
    assert_eq!(
        call(&s, ops::SESSION_REMOVE, json!({"id": "nope"}))
            .unwrap_err()
            .0,
        ErrorCode::BadRequest
    );
    assert_eq!(fake.calls().len(), 2);
}

#[test]
fn a_failed_stop_is_failed_with_what_claude_said() {
    let (_d, s, fake) = control("stopfail", true);
    fake.push_output(Ok(LaunchOutput::failed(1, "no such job")));
    assert_eq!(
        call(&s, ops::SESSION_STOP, json!({"id": "job-a"})),
        Err((ErrorCode::Failed, "no such job".into()))
    );
}

#[test]
fn new_session_starts_claude_bg_and_names_the_job() {
    let (_d, s, fake) = control("new", true);
    fake.push_output(Ok(LaunchOutput::ok(
        "backgrounded · 6ce4c19c · api (idle)\n",
    )));
    assert_eq!(
        call(
            &s,
            ops::SESSION_NEW,
            json!({"folder": "/w/api", "flags": ["--model", "opus"]})
        ),
        Ok(json!({"jobId": "6ce4c19c"}))
    );
    fake.push_output(Ok(LaunchOutput::ok("backgrounded · 1234abcd (idle)\n")));
    assert_eq!(
        call(
            &s,
            ops::SESSION_NEW,
            json!({"folder": "/w/api", "config_dir": "/lanes/b/"})
        ),
        Ok(json!({"jobId": "1234abcd"}))
    );
    assert_eq!(
        fake.commands(),
        [
            main_login(&["claude", "--model", "opus", "--bg"]).in_dir("/w/api"),
            Command::new(&["claude", "--bg"])
                .in_dir("/w/api")
                .for_config_dir(Some("/lanes/b")),
        ]
    );
}

#[test]
fn new_session_refuses_a_folder_it_cannot_use() {
    let (_d, s, fake) = control("newbad", true);
    for args in [
        json!({}),
        json!({"folder": ""}),
        json!({"folder": "/w", "flags": "--bg"}),
        json!({"folder": "/w", "config_dir": "/somewhere/else"}),
    ] {
        assert_eq!(
            call(&s, ops::SESSION_NEW, args.clone()).unwrap_err().0,
            ErrorCode::BadRequest,
            "{args}"
        );
    }
    assert!(fake.calls().is_empty());
    // Claude Code's own refusal comes back as it said it, which the app reads as untrusted.
    fake.push_output(Ok(LaunchOutput::failed(1, "Workspace not trusted: /w/new")));
    let (c, m) = call(&s, ops::SESSION_NEW, json!({"folder": "/w/new"})).unwrap_err();
    assert_eq!(c, ErrorCode::Failed);
    assert!(m.contains("not trusted"), "{m}");
}

#[test]
fn trust_plays_the_trust_script_in_a_hidden_pty() {
    let (_d, s, fake) = control("trust", true);
    assert_eq!(
        call(&s, ops::FOLDER_TRUST, json!({"folder": "/w/new"})),
        Ok(json!({"trusted": true}))
    );
    assert_eq!(
        call(
            &s,
            ops::FOLDER_TRUST,
            json!({"folder": "/w/new", "config_dir": "/lanes/b"})
        ),
        Ok(json!({"trusted": true}))
    );
    assert_eq!(
        fake.calls(),
        [
            Call::Pty(plan_trust_folder("/w/new", None)),
            Call::Pty(plan_trust_folder("/w/new", Some("/lanes/b")))
        ]
    );
    fake.push_pty(Ok(false));
    let (c, m) = call(&s, ops::FOLDER_TRUST, json!({"folder": "/w/new"})).unwrap_err();
    assert_eq!(c, ErrorCode::Failed);
    assert!(m.starts_with("Couldn't trust the folder"), "{m}");
}

#[test]
fn move_stops_then_resumes_in_the_target_folder() {
    let (d, s, fake) = control("move", true);
    fake.push_output(Ok(LaunchOutput::ok("")));
    fake.push_output(Ok(LaunchOutput::ok("backgrounded · 9f8e7d6c (idle)\n")));
    assert_eq!(
        call(
            &s,
            ops::SESSION_MOVE,
            json!({"id": "job-a", "to_config_dir": "/lanes/b"})
        ),
        Ok(json!({"jobId": "9f8e7d6c"}))
    );
    let commands = fake.commands();
    assert_eq!(commands[0], main_login(&["claude", "stop", "abcd1234"]));
    let argv = &commands[1].argv;
    assert_eq!(argv[0], "claude");
    for word in ["--bg", "--resume", SESSION] {
        assert!(argv.iter().any(|a| a == word), "{word}: {argv:?}");
    }
    assert_eq!(
        commands[1].env.get("CLAUDE_CONFIG_DIR"),
        Some(&Some("/lanes/b".into()))
    );
    // The main login is named by its folder (or left out), and the variable is removed.
    let main = d.0.join("claude");
    let _ = call(
        &s,
        ops::SESSION_MOVE,
        json!({"id": "job-a", "to_config_dir": main}),
    );
    assert_eq!(fake.commands()[3].env.get("CLAUDE_CONFIG_DIR"), Some(&None));
    // Nowhere else, and only a job with a transcript.
    assert_eq!(
        call(
            &s,
            ops::SESSION_MOVE,
            json!({"id": "job-a", "to_config_dir": "/tmp/x"})
        )
        .unwrap_err()
        .0,
        ErrorCode::BadRequest
    );
    let (c, m) = call(
        &s,
        ops::SESSION_MOVE,
        json!({"id": "job-idle", "to_config_dir": "/lanes/b"}),
    )
    .unwrap_err();
    assert_eq!(c, ErrorCode::BadRequest);
    assert!(m.contains("transcript"), "{m}");
    assert_eq!(fake.commands().len(), 4);
}

#[test]
fn a_command_is_typed_like_a_reply_unless_it_opens_a_menu() {
    let (_d, s, fake) = control("cmd", true);
    assert_eq!(
        call(
            &s,
            ops::SESSION_COMMAND,
            json!({"id": "job-a", "command": "/compact"})
        ),
        Ok(json!({"sent": true}))
    );
    assert_eq!(
        *s.writes.lock().unwrap(),
        [
            keys::clear_input(),
            "\x1b[200~/compact\x1b[201~".to_string(),
            "\r".to_string()
        ]
    );
    for command in ["/plugin", "/mcp", "compact", ""] {
        let (c, _) = call(
            &s,
            ops::SESSION_COMMAND,
            json!({"id": "job-a", "command": command}),
        )
        .unwrap_err();
        assert_eq!(c, ErrorCode::BadRequest, "{command}");
    }
    let (_, m) = call(
        &s,
        ops::SESSION_COMMAND,
        json!({"id": "job-a", "command": "/plugin"}),
    )
    .unwrap_err();
    assert!(m.contains("menu"), "{m}");
    assert_eq!(
        s.attached.lock().unwrap().len(),
        1,
        "only /compact attached"
    );
    assert!(fake.calls().is_empty(), "a command starts no process");
}
