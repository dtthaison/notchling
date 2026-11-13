use crate::acting_ops_tests::{app, code, sessions, setup_in, Setup, PROMPT, SESSION};
use crate::catalog_ops_tests::seeded;
use notchling_engine::catalog::LiveModel;
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
            json!({"folder": "/w/api", "flags": ["--model", "claude-opus-5-5"]})
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
            main_login(&["claude", "--model", "claude-opus-5-5", "--bg"]).in_dir("/w/api"),
            Command::new(&["claude", "--bg"])
                .in_dir("/w/api")
                .for_config_dir(Some("/lanes/b")),
        ]
    );
}

fn live(id: &str) -> LiveModel {
    LiveModel {
        id: id.into(),
        ..LiveModel::default()
    }
}

#[test]
fn session_new_takes_a_model_the_live_list_reports() {
    let (_d, s, fake) = control("newlive", true);
    let (_tmp, service) = seeded("newlive-cat", &[live("claude-fable-5-3")], None);
    s.ops.attach_catalog(service);
    fake.push_output(Ok(LaunchOutput::ok(
        "backgrounded · 6ce4c19c · api (idle)\n",
    )));
    assert_eq!(
        call(
            &s,
            ops::SESSION_NEW,
            json!({"folder": "/w/api", "flags": ["--model", "claude-fable-5-3"]})
        ),
        Ok(json!({"jobId": "6ce4c19c"}))
    );
    assert_eq!(
        fake.commands(),
        [main_login(&["claude", "--model", "claude-fable-5-3", "--bg"]).in_dir("/w/api")]
    );
}

#[test]
fn session_new_takes_a_built_in_model_with_a_suffix_or_an_older_one() {
    let (_d, s, fake) = control("newsuffix", true);
    for id in [
        "claude-opus-5-5[1m]",
        "claude-sonnet-5-5-20261101",
        "claude-opus-4-6",
    ] {
        fake.push_output(Ok(LaunchOutput::ok("backgrounded · 6ce4c19c (idle)\n")));
        assert_eq!(
            call(
                &s,
                ops::SESSION_NEW,
                json!({"folder": "/w/api", "flags": ["--model", id]})
            ),
            Ok(json!({"jobId": "6ce4c19c"})),
            "{id}"
        );
    }
}

#[test]
fn session_new_refuses_a_model_only_the_override_names() {
    let (_d, s, fake) = control("newoverride", true);
    let (_tmp, service) = seeded(
        "newoverride-cat",
        &[],
        Some(
            r#"{"claude": {"current": ["claude-evil-1"], "prices": [{"match": "claude-evil-1", "input": 1, "output": 1}]}}"#,
        ),
    );
    s.ops.attach_catalog(service);
    let (code, message) = call(
        &s,
        ops::SESSION_NEW,
        json!({"folder": "/w", "flags": ["--model", "claude-evil-1"]}),
    )
    .unwrap_err();
    assert_eq!(code, ErrorCode::BadRequest);
    assert!(message.contains("claude-opus-5-5"), "{message}");
    assert!(!message.contains("claude-evil-1,"), "{message}");
    assert!(fake.calls().is_empty());
}

#[test]
fn new_session_refuses_a_folder_it_cannot_use() {
    let (_d, s, fake) = control("newbad", true);
    for args in [
        json!({}),
        json!({"folder": ""}),
        json!({"folder": "/w", "flags": "--bg"}),
        json!({"folder": "/w", "config_dir": "/somewhere/else"}),
        json!({"folder": "/w", "flags": ["--dangerously-skip-permissions"]}),
        json!({"folder": "/w", "flags": ["--model", "opus"]}),
        json!({"folder": "/w", "flags": ["--model"]}),
        json!({"folder": "/w", "flags": ["--model=claude-opus-5-5"]}),
        json!({"folder": "/w", "flags": ["--model", "claude-opus-5-5", "--settings", "/x"]}),
        json!({"folder": "/w", "flags": ["--settings", "/x", "--model", "claude-opus-5-5"]}),
        json!({"folder": "/w", "flags": ["--model", "claude-opus-9-9"]}),
        json!({"folder": "/w", "flags": ["--model", "claude-opus-5-5[1m] --dangerously-skip-permissions"]}),
        json!({"folder": "/w", "flags": ["--model", "claude-opus-5-5[x]\n/permissions"]}),
    ] {
        assert_eq!(
            call(&s, ops::SESSION_NEW, args.clone()).unwrap_err().0,
            ErrorCode::BadRequest,
            "{args}"
        );
    }
    assert!(fake.calls().is_empty());
    fake.push_output(Ok(LaunchOutput::failed(1, "Workspace not trusted: /w/new")));
    let (c, m) = call(&s, ops::SESSION_NEW, json!({"folder": "/w/new"})).unwrap_err();
    assert_eq!(c, ErrorCode::Failed);
    assert!(m.contains("not trusted"), "{m}");
}

fn refused_new(s: &Setup, fake: &FakeLauncher, folder: &str) {
    fake.push_output(Ok(LaunchOutput::failed(
        1,
        &format!("Workspace not trusted: {folder}"),
    )));
    let (c, _) = call(s, ops::SESSION_NEW, json!({"folder": folder})).unwrap_err();
    assert_eq!(c, ErrorCode::Failed);
}

#[test]
fn trust_plays_the_trust_script_in_a_hidden_pty() {
    let (d, s, fake) = control("trust", true);
    let new = d.0.join("work/new");
    std::fs::create_dir_all(&new).unwrap();
    let folder = new.to_str().unwrap();
    refused_new(&s, &fake, folder);
    assert_eq!(
        call(&s, ops::FOLDER_TRUST, json!({"folder": folder})),
        Ok(json!({"trusted": true}))
    );
    refused_new(&s, &fake, folder);
    assert_eq!(
        call(
            &s,
            ops::FOLDER_TRUST,
            json!({"folder": folder, "config_dir": "/lanes/b"})
        ),
        Ok(json!({"trusted": true}))
    );
    let ptys: Vec<Call> = fake
        .calls()
        .into_iter()
        .filter(|c| matches!(c, Call::Pty(_)))
        .collect();
    assert_eq!(
        ptys,
        [
            Call::Pty(plan_trust_folder(folder, None)),
            Call::Pty(plan_trust_folder(folder, Some("/lanes/b")))
        ]
    );
    refused_new(&s, &fake, folder);
    fake.push_pty(Ok(false));
    let (c, m) = call(&s, ops::FOLDER_TRUST, json!({"folder": folder})).unwrap_err();
    assert_eq!(c, ErrorCode::Failed);
    assert!(m.starts_with("Couldn't trust the folder"), "{m}");
}

#[test]
fn trust_takes_only_a_real_folder_that_session_new_was_just_refused() {
    let (d, s, fake) = control("trustbad", true);
    let home = d.0.clone();
    let work = d.0.join("work/api");
    std::fs::create_dir_all(&work).unwrap();
    let file = d.0.join("work/notes.txt");
    std::fs::write(&file, "x").unwrap();
    let folder = work.to_str().unwrap().to_string();
    let (c, m) = call(&s, ops::FOLDER_TRUST, json!({"folder": folder})).unwrap_err();
    assert_eq!(c, ErrorCode::BadRequest, "{m}");
    let dotted = format!("{}/work/..", home.display());
    for bad in [
        "work/api".to_string(),
        format!("{}/work/missing", home.display()),
        file.to_string_lossy().into_owned(),
        "/".to_string(),
        home.to_string_lossy().into_owned(),
        dotted,
    ] {
        refused_new(&s, &fake, &bad);
        let (c, m) = call(&s, ops::FOLDER_TRUST, json!({"folder": bad})).unwrap_err();
        assert_eq!(c, ErrorCode::BadRequest, "{bad}: {m}");
    }
    let other = d.0.join("work/other");
    std::fs::create_dir_all(&other).unwrap();
    refused_new(&s, &fake, other.to_str().unwrap());
    assert_eq!(
        call(&s, ops::FOLDER_TRUST, json!({"folder": folder}))
            .unwrap_err()
            .0,
        ErrorCode::BadRequest
    );
    refused_new(&s, &fake, &folder);
    s.ops.age_refusals(std::time::Duration::from_secs(11 * 60));
    assert_eq!(
        call(&s, ops::FOLDER_TRUST, json!({"folder": folder}))
            .unwrap_err()
            .0,
        ErrorCode::BadRequest
    );
    assert!(
        !fake.calls().iter().any(|c| matches!(c, Call::Pty(_))),
        "nothing was trusted"
    );
    refused_new(&s, &fake, &folder);
    let spelled = format!("{}/work/../work/api/", home.display());
    assert_eq!(
        call(&s, ops::FOLDER_TRUST, json!({"folder": spelled})),
        Ok(json!({"trusted": true}))
    );
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
    let main = d.0.join("claude");
    let _ = call(
        &s,
        ops::SESSION_MOVE,
        json!({"id": "job-a", "to_config_dir": main}),
    );
    assert_eq!(fake.commands()[3].env.get("CLAUDE_CONFIG_DIR"), Some(&None));
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

fn control_in(tag: &str, cwd: &str) -> (Dir, Setup, Arc<FakeLauncher>) {
    let (dir, roots) = roots(tag);
    let mut list = sessions();
    list[0]["cwd"] = json!(cwd);
    let fake = Arc::new(FakeLauncher::new());
    let launcher: Arc<dyn Launcher> = fake.clone();
    let s = setup_in(true, PROMPT, list, roots, Some(launcher));
    (dir, s, fake)
}

fn work_folder(tag: &str) -> (Dir, PathBuf, String) {
    let base = std::env::temp_dir().join(format!("nl-ctl-work-{tag}-{}", std::process::id()));
    let folder = base.join("api");
    std::fs::create_dir_all(&folder).unwrap();
    let real = std::fs::canonicalize(&folder).unwrap();
    let parent = real.parent().unwrap().to_path_buf();
    (Dir(base), parent, real.to_string_lossy().into_owned())
}

#[test]
fn move_trusts_the_folder_for_an_account_that_never_did_then_moves() {
    let (_w, _, folder) = work_folder("move-trust");
    let (_d, s, fake) = control_in("move-trust", &folder);
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
    assert_eq!(
        fake.calls()[0],
        Call::Pty(plan_trust_folder(&folder, Some("/lanes/b")))
    );
    assert_eq!(
        fake.commands()[0],
        main_login(&["claude", "stop", "abcd1234"])
    );
}

#[test]
fn move_to_an_account_that_trusts_a_parent_folder_asks_nothing() {
    let (_w, parent, folder) = work_folder("move-trusted");
    let (d, s, fake) = control_in("move-trusted", &folder);
    let mut projects = serde_json::Map::new();
    projects.insert(
        parent.to_string_lossy().into_owned(),
        json!({"hasTrustDialogAccepted": true}),
    );
    std::fs::write(
        d.0.join(".claude.json"),
        json!({ "projects": projects }).to_string(),
    )
    .unwrap();
    fake.push_output(Ok(LaunchOutput::ok("")));
    fake.push_output(Ok(LaunchOutput::ok("backgrounded · 9f8e7d6c (idle)\n")));
    let main = d.0.join("claude");
    assert!(call(
        &s,
        ops::SESSION_MOVE,
        json!({"id": "job-a", "to_config_dir": main}),
    )
    .is_ok());
    assert!(
        !fake.calls().iter().any(|c| matches!(c, Call::Pty(_))),
        "the main login trusts it already"
    );
}

#[test]
fn a_refused_trust_stops_nothing() {
    let (_w, _, folder) = work_folder("move-refused");
    let (_d, s, fake) = control_in("move-refused", &folder);
    fake.push_pty(Ok(false));
    assert!(call(
        &s,
        ops::SESSION_MOVE,
        json!({"id": "job-a", "to_config_dir": "/lanes/b"})
    )
    .is_err());
    assert!(fake.commands().is_empty(), "the session keeps running");
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
