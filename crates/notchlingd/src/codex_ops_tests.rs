//! The Codex ops with a scripted tool and a fake launcher: nothing here runs `codex`, reads the
//! real `~/.codex` or touches the real Notchling folder. Rollouts are written into a temp folder.

use super::*;
use crate::acting_ops_tests::{code, FakeRoom};
use notchling_engine::control::{FakeLauncher, LaunchOutput};
use notchling_engine::tools::codex::fake::{FakeRun, FakeSpawner};

struct Dir(PathBuf);

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

struct Setup {
    dir: Dir,
    ops: Arc<CodexOps>,
    spawner: Arc<FakeSpawner>,
    launcher: Arc<FakeLauncher>,
    room: Arc<FakeRoom>,
    acting: Arc<Acting>,
}

const SESSION: &str = "019a5c3e-aaaa-7bbb-8ccc-000000000001";
const STARTED: &str =
    r#"{"type":"thread.started","thread_id":"019a5c3e-new0-7bbb-8ccc-000000000002"}"#;
const NEW_SESSION: &str = "019a5c3e-new0-7bbb-8ccc-000000000002";
const DONE: &str = r#"{"type":"turn.completed","usage":{}}"#;

fn setup(tag: &str, on: bool) -> Setup {
    let dir = std::env::temp_dir().join(format!("nl-codexops-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    for sub in ["home/work", "home/codex/sessions/2026/11/08", "data"] {
        std::fs::create_dir_all(dir.join(sub)).expect("dirs");
    }
    // A rollout for SESSION that records the folder it ran in.
    let work = std::fs::canonicalize(dir.join("home/work")).expect("work");
    let rollout = format!(
        "{{\"timestamp\":\"2026-11-08T09:00:00.000Z\",\"type\":\"session_meta\",\"payload\":{{\"id\":\"{SESSION}\",\"cwd\":\"{}\"}}}}\n",
        work.display()
    );
    std::fs::write(
        dir.join(format!(
            "home/codex/sessions/2026/11/08/rollout-2026-11-08T09-00-00-{SESSION}.jsonl"
        )),
        rollout,
    )
    .expect("rollout");
    let spawner = Arc::new(FakeSpawner::new());
    let launcher = Arc::new(FakeLauncher::new());
    let room = Arc::new(FakeRoom::default());
    let acting = Arc::new(Acting::in_memory(on));
    let home = std::fs::canonicalize(dir.join("home")).expect("home");
    let ops = CodexOps::new(
        acting.clone(),
        CodexSetup {
            codex_home: home.join("codex"),
            home,
            data_dir: dir.join("data"),
            fixture: false,
        },
        spawner.clone(),
        launcher.clone(),
        room.clone(),
    )
    .with_resolver(Box::new(|| {
        Some((
            PathBuf::from("/opt/codex/bin/codex"),
            "/usr/bin".to_string(),
        ))
    }));
    Setup {
        dir: Dir(dir),
        ops: Arc::new(ops),
        spawner,
        launcher,
        room,
        acting,
    }
}

impl Setup {
    fn call(&self, op: &str, args: Value) -> Result<Value, ApiError> {
        self.ops.call(op, &args)
    }

    fn work(&self) -> String {
        std::fs::canonicalize(self.dir.0.join("home/work"))
            .expect("work")
            .to_string_lossy()
            .into_owned()
    }

    /// Waits until `n` events named `name` were published.
    fn wait_named(&self, name: &str, n: usize) -> Vec<Value> {
        let until = Instant::now() + Duration::from_secs(10);
        while Instant::now() < until {
            let got = self.room.named(name);
            if got.len() >= n {
                return got;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        panic!(
            "waited for {n} {name} events, got {:?}",
            self.room.named(name)
        );
    }

    fn idle(&self) {
        let until = Instant::now() + Duration::from_secs(10);
        while self.ops.runs().active() > 0 && Instant::now() < until {
            std::thread::sleep(Duration::from_millis(5));
        }
    }
}

fn bad_request(result: Result<Value, ApiError>) -> String {
    let (c, m) = code(result);
    assert_eq!(c, ErrorCode::BadRequest, "{m}");
    m
}

#[test]
fn a_message_resumes_the_session_in_the_folder_its_rollout_records() {
    let s = setup("resume", true);
    s.spawner.push(FakeRun::ok(&[DONE]));
    let reply = s
        .call(
            ops::CODEX_SEND,
            json!({"id": format!("codex:{SESSION}"), "text": "more please"}),
        )
        .expect("sent");
    assert_eq!(
        reply,
        json!({"id": format!("codex:{SESSION}"), "session": SESSION, "state": "started"})
    );
    s.idle();
    let spec = &s.spawner.specs()[0];
    assert_eq!(spec.program, PathBuf::from("/opt/codex/bin/codex"));
    assert_eq!(spec.cwd, s.work());
    assert_eq!(&spec.args[..2], ["exec", "resume"]);
    assert_eq!(&spec.args[spec.args.len() - 2..], [SESSION, "more please"]);
    assert!(
        spec.args
            .contains(&"sandbox_workspace_write.network_access=true".to_string()),
        "the network is open by default"
    );
    assert!(spec
        .env
        .contains(&("PATH".to_string(), Some("/usr/bin".to_string()))));
}

#[test]
fn a_turn_publishes_codex_turn_and_the_changed_row() {
    let s = setup("events", true);
    s.spawner.push(FakeRun::ok(&[DONE]));
    let id = format!("codex:{SESSION}");
    s.call(ops::CODEX_SEND, json!({"id": id, "text": "hi"}))
        .expect("sent");
    let turns = s.wait_named("codex.turn", 2);
    assert_eq!(
        turns[0],
        json!({"id": id, "session": SESSION, "state": "started"})
    );
    assert_eq!(
        turns[1],
        json!({"id": id, "session": SESSION, "state": "ended"})
    );
    let rows = s.wait_named("session.changed", 2);
    assert_eq!(rows[0]["id"], json!(id));
    assert_eq!(rows[0]["row"]["label"], json!("Working"));
    assert_eq!(rows[0]["row"]["group"], json!("working"));
    assert_eq!(rows[0]["row"]["id"], json!(id));
    assert_eq!(rows[1]["row"]["label"], json!("Idle"));
    assert_eq!(rows[1]["row"]["group"], json!("completed"));
}

#[test]
fn a_failed_turn_says_why() {
    let s = setup("failed", true);
    s.spawner.push(
        FakeRun::ok(&[r#"{"type":"turn.failed","error":{"message":"rate limited"}}"#]).exiting(1),
    );
    s.call(
        ops::CODEX_SEND,
        json!({"id": format!("codex:{SESSION}"), "text": "hi"}),
    )
    .expect("sent");
    let turns = s.wait_named("codex.turn", 2);
    assert_eq!(turns[1]["state"], json!("failed"));
    assert_eq!(turns[1]["error"], json!("rate limited"));
}

#[test]
fn a_draft_folder_starts_a_new_session_and_the_reply_names_it() {
    let s = setup("draft", true);
    s.spawner.push(FakeRun::ok(&[STARTED, DONE]));
    let reply = s
        .call(
            ops::CODEX_SEND,
            json!({"draft_folder": s.work(), "text": "build it"}),
        )
        .expect("sent");
    assert_eq!(
        reply,
        json!({"id": format!("codex:{NEW_SESSION}"), "session": NEW_SESSION, "state": "started"})
    );
    s.idle();
    let args = &s.spawner.specs()[0].args;
    assert_eq!(args[0], "exec");
    assert!(!args.contains(&"resume".to_string()));
    assert_eq!(&args[args.len() - 3..], ["-C", &s.work(), "build it"]);
}

#[test]
fn a_message_behind_a_running_turn_is_queued() {
    let s = setup("queue", true);
    s.spawner.push(FakeRun::held(&[]));
    s.spawner.push(FakeRun::ok(&[DONE]));
    let id = format!("codex:{SESSION}");
    s.call(ops::CODEX_SEND, json!({"id": id, "text": "one"}))
        .expect("sent");
    let queued = s
        .call(ops::CODEX_SEND, json!({"id": id, "text": "two"}))
        .expect("queued");
    assert_eq!(
        queued,
        json!({"id": id, "session": SESSION, "state": "queued", "waiting": 1})
    );
    assert_eq!(s.spawner.specs().len(), 1);
    s.spawner.processes()[0].release();
    s.wait_named("codex.turn", 5);
    s.idle();
    let specs = s.spawner.specs();
    assert_eq!(specs.len(), 2);
    assert_eq!(specs[1].args.last().map(String::as_str), Some("two"));
    assert_eq!(specs[1].cwd, s.work(), "it runs where the session does");
}

#[test]
fn what_a_caller_names_is_checked_before_anything_starts() {
    let s = setup("bad", true);
    let id = format!("codex:{SESSION}");
    let home = s.dir.0.join("home").to_string_lossy().into_owned();
    let file = s.dir.0.join("data/settings.json");
    std::fs::write(&file, "{}").expect("file");
    let cases: Vec<(Value, &str)> = vec![
        (json!({"text": "hi"}), "not both"),
        (
            json!({"id": id, "draft_folder": "/tmp", "text": "hi"}),
            "not both",
        ),
        (json!({"id": id}), "text is needed"),
        (json!({"id": id, "text": "  \n"}), "empty"),
        (
            json!({"id": id, "text": "x".repeat(MAX_PROMPT_BYTES + 1)}),
            "KB",
        ),
        (json!({"id": SESSION, "text": "hi"}), "codex:<session>"),
        (
            json!({"id": "codex:../etc", "text": "hi"}),
            "isn't a Codex session id",
        ),
        (
            json!({"id": "codex:-x", "text": "hi"}),
            "isn't a Codex session id",
        ),
        (
            json!({"id": "codex:nosuch", "text": "hi"}),
            "no Codex session nosuch",
        ),
        (
            json!({"draft_folder": "relative", "text": "hi"}),
            "absolute",
        ),
        (
            json!({"draft_folder": "/no/such/folder", "text": "hi"}),
            "doesn't exist",
        ),
        (
            json!({"draft_folder": file.to_string_lossy(), "text": "hi"}),
            "isn't a folder",
        ),
        (
            json!({"draft_folder": "/", "text": "hi"}),
            "root or the home",
        ),
        (
            json!({"draft_folder": home, "text": "hi"}),
            "root or the home",
        ),
        (
            json!({"id": id, "text": "hi", "model": "a b"}),
            "model name",
        ),
        (json!({"id": id, "text": "hi", "model": "-m"}), "model name"),
        (json!({"id": id, "text": "hi", "model": 5}), "string"),
        (
            json!({"id": id, "text": "hi", "effort": "high\" -c x=\""}),
            "effort is a level",
        ),
    ];
    for (args, wanted) in cases {
        let message = bad_request(s.call(ops::CODEX_SEND, args.clone()));
        assert!(message.contains(wanted), "{args}: {message}");
    }
    assert!(s.spawner.specs().is_empty(), "nothing started");
}

#[test]
fn an_effort_the_models_cache_does_not_list_is_refused() {
    let s = setup("effort", true);
    std::fs::write(
        s.dir.0.join("home/codex/models_cache.json"),
        r#"{"models":[{"slug":"gpt-6","visibility":"list","supported_reasoning_levels":[{"effort":"low"}]}]}"#,
    )
    .expect("cache");
    let id = format!("codex:{SESSION}");
    let message = bad_request(s.call(
        ops::CODEX_SEND,
        json!({"id": id, "text": "hi", "model": "gpt-6", "effort": "xhigh"}),
    ));
    assert!(message.contains("doesn't offer"), "{message}");
    s.spawner.push(FakeRun::ok(&[]));
    s.call(
        ops::CODEX_SEND,
        json!({"id": id, "text": "hi", "model": "gpt-6", "effort": "low"}),
    )
    .expect("a listed effort");
    s.idle();
    let args = &s.spawner.specs()[0].args;
    assert!(args.windows(2).any(|w| w == ["-m", "gpt-6"]));
}

#[test]
fn a_flag_shaped_message_is_passed_as_a_prompt() {
    let s = setup("flag", true);
    s.spawner.push(FakeRun::ok(&[]));
    s.call(
        ops::CODEX_SEND,
        json!({"id": format!("codex:{SESSION}"), "text": "--version"}),
    )
    .expect("sent");
    s.idle();
    let args = &s.spawner.specs()[0].args;
    assert_eq!(&args[args.len() - 3..], ["--", SESSION, "--version"]);
}

#[test]
fn the_network_follows_the_codex_network_setting() {
    let s = setup("net", true);
    notchling_engine::settings::Settings::open(&s.dir.0.join("data/settings.json"))
        .expect("settings")
        .set("codexNetwork", json!(false))
        .expect("set");
    s.spawner.push(FakeRun::ok(&[]));
    s.call(
        ops::CODEX_SEND,
        json!({"id": format!("codex:{SESSION}"), "text": "hi"}),
    )
    .expect("sent");
    s.idle();
    assert!(!s.spawner.specs()[0]
        .args
        .iter()
        .any(|a| a.contains("network_access")));
}

#[test]
fn without_codex_installed_a_send_fails_plainly() {
    let s = setup("missing", true);
    let ops_without = CodexOps::new(
        s.acting.clone(),
        s.ops.setup.clone(),
        s.spawner.clone(),
        s.launcher.clone(),
        s.room.clone(),
    )
    .with_resolver(Box::new(|| None));
    let got = ops_without.call(
        ops::CODEX_SEND,
        &json!({"id": format!("codex:{SESSION}"), "text": "hi"}),
    );
    assert_eq!(
        code(got),
        (
            ErrorCode::Failed,
            "Couldn't find the codex command. Install the Codex CLI first.".to_string()
        )
    );
    assert!(s.spawner.specs().is_empty());
}

#[test]
fn a_start_that_fails_is_failed_and_frees_the_session() {
    let s = setup("spawnfail", true);
    s.spawner.push_error("permission denied");
    let id = format!("codex:{SESSION}");
    let (c, m) = code(s.call(ops::CODEX_SEND, json!({"id": id, "text": "hi"})));
    assert_eq!(c, ErrorCode::Failed);
    assert!(m.contains("permission denied"), "{m}");
    s.spawner.push(FakeRun::ok(&[]));
    assert!(s
        .call(ops::CODEX_SEND, json!({"id": id, "text": "again"}))
        .is_ok());
    s.idle();
}

#[test]
fn stop_interrupts_the_running_turn_only() {
    let s = setup("stop", true);
    let id = format!("codex:{SESSION}");
    assert_eq!(
        s.call(ops::CODEX_STOP, json!({"id": id})),
        Ok(json!({"stopped": false})),
        "nothing running"
    );
    s.spawner.push(FakeRun::held(&[]));
    s.call(ops::CODEX_SEND, json!({"id": id, "text": "go"}))
        .expect("sent");
    assert_eq!(
        s.call(ops::CODEX_STOP, json!({"id": id})),
        Ok(json!({"stopped": true}))
    );
    assert_eq!(s.spawner.processes()[0].interrupts(), 1);
    s.wait_named("codex.turn", 2);
    bad_request(s.call(ops::CODEX_STOP, json!({"id": "codex:a b"})));
    bad_request(s.call(ops::CODEX_STOP, json!({})));
    s.idle();
}

#[test]
fn delete_runs_codex_delete_force_after_stopping_the_turn() {
    let s = setup("delete", true);
    let id = format!("codex:{SESSION}");
    s.spawner.push(FakeRun::held(&[]));
    s.call(ops::CODEX_SEND, json!({"id": id, "text": "go"}))
        .expect("sent");
    s.call(ops::CODEX_SEND, json!({"id": id, "text": "queued"}))
        .expect("queued");
    assert_eq!(
        s.call(ops::CODEX_DELETE, json!({"id": id})),
        Ok(json!({"deleted": true}))
    );
    assert_eq!(s.spawner.processes()[0].interrupts(), 1, "stopped first");
    s.idle();
    assert_eq!(s.spawner.specs().len(), 1, "the queued message was dropped");
    let commands = s.launcher.commands();
    assert_eq!(commands.len(), 1);
    assert_eq!(
        commands[0].argv,
        ["/opt/codex/bin/codex", "delete", "--force", SESSION]
    );
    assert_eq!(
        commands[0].env.get("PATH"),
        Some(&Some("/usr/bin".to_string()))
    );
    let changed = s.room.named("session.changed");
    let last = changed.last().expect("an event");
    assert_eq!(last["deleted"], json!(true));
    assert_eq!(last["row"], Value::Null);
}

#[test]
fn turns_and_delete_run_with_the_codex_home_the_daemon_reads() {
    let s = setup("codexhome", true);
    let home = s.ops.setup.codex_home.to_string_lossy().into_owned();
    s.spawner.push(FakeRun::ok(&[DONE]));
    s.call(
        ops::CODEX_SEND,
        json!({"id": format!("codex:{SESSION}"), "text": "go"}),
    )
    .expect("sent");
    s.idle();
    assert!(
        s.spawner.specs()[0]
            .env
            .contains(&("CODEX_HOME".to_string(), Some(home.clone()))),
        "{:?}",
        s.spawner.specs()[0].env
    );
    s.call(ops::CODEX_DELETE, json!({"id": format!("codex:{SESSION}")}))
        .expect("deleted");
    assert_eq!(
        s.launcher.commands()[0].env.get("CODEX_HOME"),
        Some(&Some(home)),
        "codex delete removes the file from the home the daemon lists"
    );
}

#[test]
fn over_a_fixture_root_a_codex_elsewhere_on_the_machine_is_not_run() {
    let root = std::env::temp_dir().join(format!("nl-codexops-fixture-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let only = |wanted: PathBuf| move |p: &Path| p == wanted;
    let setup = |fixture| CodexSetup {
        codex_home: root.join("codex"),
        home: root.clone(),
        data_dir: root.join("notchling"),
        fixture,
    };
    let own = root.join(".local/bin/codex");
    // The machine has /opt/homebrew/bin/codex (and the PATH names it): a real install, not the root's.
    let found = locate(
        &setup(true),
        "/opt/homebrew/bin",
        only(PathBuf::from("/opt/homebrew/bin/codex")),
    );
    assert_eq!(found, None);
    let found = locate(&setup(true), "/usr/bin", only(own.clone()));
    assert_eq!(found.map(|(codex, _)| codex), Some(own));
    // Over the real folders the usual places still count.
    let found = locate(
        &setup(false),
        "",
        only(PathBuf::from("/opt/homebrew/bin/codex")),
    );
    assert_eq!(
        found.map(|(codex, _)| codex),
        Some(PathBuf::from("/opt/homebrew/bin/codex"))
    );
}

#[test]
fn a_delete_that_fails_says_what_codex_said() {
    let s = setup("deletefail", true);
    s.launcher.push_output(Ok(LaunchOutput {
        code: 1,
        output: String::new(),
        stderr: "warning\nsession is locked\n\n".into(),
    }));
    let got = s.call(ops::CODEX_DELETE, json!({"id": format!("codex:{SESSION}")}));
    assert_eq!(code(got), (ErrorCode::Failed, "session is locked".into()));
    s.launcher.push_output(Ok(LaunchOutput::failed(3, "")));
    let got = s.call(ops::CODEX_DELETE, json!({"id": format!("codex:{SESSION}")}));
    assert_eq!(
        code(got),
        (ErrorCode::Failed, "codex delete exited with code 3.".into())
    );
    // A session with no rollout isn't deleted at all.
    let before = s.launcher.calls().len();
    bad_request(s.call(ops::CODEX_DELETE, json!({"id": "codex:nosuch"})));
    assert_eq!(s.launcher.calls().len(), before);
}

#[test]
fn closing_stops_every_running_turn() {
    let s = setup("close", true);
    s.spawner.push(FakeRun::held(&[]));
    s.call(
        ops::CODEX_SEND,
        json!({"id": format!("codex:{SESSION}"), "text": "go"}),
    )
    .expect("sent");
    s.ops.close();
    assert_eq!(s.spawner.processes()[0].interrupts(), 1);
    s.idle();
}

#[test]
fn the_daemon_routes_the_codex_ops_and_offers_the_tools_topic() {
    use crate::acting_ops_tests::app;
    use crate::daemon::Ops;
    use notchling_local_api::Handler;
    let s = setup("route", false);
    let codex = s.ops.clone();
    let daemon = Ops::new(Arc::new(std::sync::Mutex::new(json!([])))).with_codex(codex);
    assert!(daemon.topics().contains(&topics::TOOLS));
    assert_eq!(
        daemon.topic_tier(topics::TOOLS),
        notchling_local_api::Tier::Agent
    );
    for op in CODEX_OPS {
        assert_eq!(daemon.spec(op), Some(OpSpec::APP), "{op}");
        // Reaches the Codex ops, which refuse while acting is off.
        assert_eq!(
            code(daemon.call(&app(), op, &json!({}))),
            (ErrorCode::Forbidden, ACTING_OFF.to_string())
        );
    }
}

#[path = "codex_acting_tests.rs"]
mod acting_off;
#[path = "codex_slow_tests.rs"]
mod late;
