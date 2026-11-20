use crate::acting_ops::ActingOps;
use crate::acting_ops_tests::{app, caller, code, sessions, setup_in, Setup, PROMPT};
use crate::outbox_ops::OutboxOps;
use crate::reload_ops::{Hosts, LiveHosts, ReloadOps};
use notchling_engine::chrome::status;
use notchling_engine::control::{FakeLauncher, LaunchOutput, Launcher};
use notchling_engine::outbox::{OutboxStore, State};
use notchling_engine::reload::flow::{Host, POLL_MS};
use notchling_engine::sessions::{Agent, Roots};
use notchling_events::kinds::choices::DoneOrFailed;
use notchling_local_api::{ops, ClientKind, ErrorCode};
use serde_json::{json, Value};
use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

struct Dir(PathBuf);

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[derive(Default)]
struct Log {
    turns: VecDeque<bool>,
    dialogs: VecDeque<bool>,
    tasks: VecDeque<Option<u32>>,
    restarts: u32,
    sleeps: Vec<u64>,
    hold: bool,
    now: u64,
    moved_to: Option<String>,
}

#[derive(Clone, Default)]
struct Fake(Arc<Mutex<Log>>);

struct FakeHost(Fake);

impl Host for FakeHost {
    fn turn_running(&mut self) -> bool {
        let mut log = (self.0).0.lock().unwrap();
        log.hold || log.turns.pop_front().unwrap_or(false)
    }
    fn dialog_open(&mut self) -> bool {
        (self.0).0.lock().unwrap().dialogs.pop_front().unwrap_or(false)
    }
    fn tasks_in_flight(&mut self) -> Option<u32> {
        (self.0)
            .0
            .lock()
            .unwrap()
            .tasks
            .pop_front()
            .unwrap_or(Some(0))
    }
    fn restart(&mut self) -> Result<bool, String> {
        (self.0).0.lock().unwrap().restarts += 1;
        Ok(true)
    }
    fn sleep(&mut self, ms: u64) {
        let hold = {
            let mut log = (self.0).0.lock().unwrap();
            log.sleeps.push(ms);
            log.now += ms;
            log.hold
        };
        if hold {
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
    }
    fn now_ms(&mut self) -> u64 {
        (self.0).0.lock().unwrap().now
    }
}

impl Hosts for Fake {
    fn host(&self, _agent: &Agent) -> Box<dyn Host + Send> {
        Box::new(FakeHost(self.clone()))
    }
    fn current(&self, _agent: &Agent) -> Option<String> {
        self.0.lock().unwrap().moved_to.clone()
    }
}

fn roots(tag: &str) -> (Dir, Roots) {
    let dir = std::env::temp_dir().join(format!("nl-reload-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("claude/jobs/abcd1234")).unwrap();
    std::fs::create_dir_all(dir.join("claude/sessions")).unwrap();
    let roots = Roots {
        main_dir: dir.join("claude"),
        lanes_file: dir.join("lanes.json"),
    };
    (Dir(dir), roots)
}

fn faked(tag: &str, fake: &Fake, inline: bool) -> (Dir, Setup) {
    let (dir, roots) = roots(tag);
    let s = setup_in(true, PROMPT, sessions(), roots, None);
    let reload = if inline {
        ReloadOps::inline(Arc::new(fake.clone()))
    } else {
        ReloadOps::new(Arc::new(fake.clone()))
    };
    s.ops.attach_reload(reload);
    (dir, s)
}

fn reload(s: &Setup, id: &str, after_turn: bool) -> Result<Value, (ErrorCode, String)> {
    s.ops
        .call(
            &app(),
            ops::SESSION_RELOAD,
            &json!({"id": id, "afterTurn": after_turn}),
        )
        .map_err(|e| (e.code, e.message))
}

fn phase_of(s: &Setup, id: &str) -> Value {
    s.ops
        .call(&app(), ops::SESSION_RELOAD_STATE, &json!({"id": id}))
        .unwrap()["task"]
        .clone()
}

fn until_over(s: &Setup, id: &str) -> Value {
    for _ in 0..2000 {
        let task = phase_of(s, id);
        if matches!(
            task["phase"].as_str(),
            Some("done" | "needs_you" | "cancelled")
        ) {
            return task;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    panic!("the reload never ended");
}

#[test]
fn an_idle_session_is_reloaded_and_the_op_is_agent_tier() {
    let fake = Fake::default();
    let (_d, s) = faked("idle", &fake, true);
    assert_eq!(
        s.ops.spec(ops::SESSION_RELOAD),
        Some(notchling_local_api::OpSpec::AGENT)
    );
    assert_eq!(
        s.ops.spec(ops::SESSION_RELOAD_STATE),
        Some(notchling_local_api::OpSpec::APP)
    );
    let reply = reload(&s, "job-a", true).unwrap();
    assert_eq!(reply["offered"], true);
    assert_eq!(reply["task"]["phase"], "done");
    assert_eq!(fake.0.lock().unwrap().restarts, 1);
}

#[test]
fn it_never_restarts_mid_turn_and_goes_after_the_turn() {
    let fake = Fake::default();
    fake.0.lock().unwrap().turns = VecDeque::from([true, true, false]);
    let (_d, s) = faked("midturn", &fake, true);
    let reply = reload(&s, "job-a", true).unwrap();
    assert_eq!(reply["task"]["phase"], "done");
    let log = fake.0.lock().unwrap();
    assert_eq!(log.sleeps, [POLL_MS, POLL_MS]);
    assert_eq!(log.restarts, 1);
}

#[test]
fn without_after_turn_a_running_turn_is_refused_for_the_card() {
    let fake = Fake::default();
    fake.0.lock().unwrap().turns = VecDeque::from([true]);
    let (_d, s) = faked("refuse", &fake, true);
    let reply = reload(&s, "job-a", false).unwrap();
    assert_eq!(reply["task"]["phase"], "needs_you");
    assert_eq!(reply["task"]["need"]["need"], "busy");
    assert_eq!(reply["task"]["need"]["by"]["why"], "turn");
    assert_eq!(fake.0.lock().unwrap().restarts, 0);
}

#[test]
fn background_helpers_hold_it_and_the_card_can_say_how_many() {
    let fake = Fake::default();
    fake.0.lock().unwrap().tasks = VecDeque::from(vec![Some(3); 60]);
    fake.0.lock().unwrap().hold = false;
    let (_d, s) = faked("tasks", &fake, false);
    reload(&s, "job-a", true).unwrap();
    let mut seen = Value::Null;
    for _ in 0..400 {
        let task = phase_of(&s, "job-a");
        if task["by"]["why"] == "tasks" {
            seen = task;
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert_eq!(seen["by"]["why"], "tasks");
    assert_eq!(seen["by"]["count"], 3);
    assert_eq!(fake.0.lock().unwrap().restarts, 0);
    let cancelled = s
        .ops
        .call(
            &app(),
            ops::SESSION_RELOAD,
            &json!({"id": "job-a", "cancel": true}),
        )
        .unwrap();
    assert!(cancelled["task"]["phase"].is_string());
    assert_eq!(until_over(&s, "job-a")["phase"], "cancelled");
    assert_eq!(fake.0.lock().unwrap().restarts, 0);
}

#[test]
fn an_open_dialog_waits_before_the_restart() {
    let fake = Fake::default();
    fake.0.lock().unwrap().dialogs = VecDeque::from([true, true]);
    let (_d, s) = faked("dialog", &fake, true);
    let reply = reload(&s, "job-a", true).unwrap();
    assert_eq!(reply["task"]["phase"], "done");
    assert_eq!(fake.0.lock().unwrap().sleeps.len(), 2);
}

#[test]
fn a_terminal_session_the_user_owns_is_never_restarted() {
    let fake = Fake::default();
    let (_d, s) = faked("terminal", &fake, true);
    let (error, message) = code(
        s.ops
            .call(&app(), ops::SESSION_RELOAD, &json!({"id": "term"})),
    );
    assert_eq!(error, ErrorCode::BadRequest);
    assert!(message.contains("claude --resume term"), "{message}");
    assert_eq!(fake.0.lock().unwrap().restarts, 0);
    let state = s
        .ops
        .call(&app(), ops::SESSION_RELOAD_STATE, &json!({"id": "term"}))
        .unwrap();
    assert_eq!(state["offered"], false);
    assert_eq!(state["kind"], "terminal");
}

#[test]
fn it_is_refused_while_acting_is_off() {
    let fake = Fake::default();
    let (_d, s) = faked("off", &fake, true);
    s.ops.acting_switch().set(false);
    assert_eq!(
        reload(&s, "job-a", true).unwrap_err().0,
        ErrorCode::Forbidden
    );
    assert_eq!(fake.0.lock().unwrap().restarts, 0);
}

fn inside(s: &Setup, dir: &Dir, session: &str) {
    let file = dir.0.join("claude/sessions/4242.json");
    std::fs::write(file, json!({"sessionId": session}).to_string()).unwrap();
    let _ = s;
}

#[test]
fn an_agent_may_reload_only_itself_and_always_after_the_turn() {
    let fake = Fake::default();
    fake.0.lock().unwrap().turns = VecDeque::from([true, false]);
    let (d, s) = faked("agent", &fake, true);
    inside(&s, &d, "job-a");
    let agent = caller(ClientKind::Cli);
    let other = s
        .ops
        .call(&agent, ops::SESSION_RELOAD, &json!({"id": "job-idle"}));
    assert_eq!(code(other).0, ErrorCode::Forbidden);
    let own = s
        .ops
        .call(
            &agent,
            ops::SESSION_RELOAD,
            &json!({"id": "self", "afterTurn": false}),
        )
        .unwrap();
    assert_eq!(own["task"]["phase"], "done");
    assert_eq!(own["task"]["after_turn"], true);
    assert_eq!(fake.0.lock().unwrap().sleeps, [POLL_MS]);
    assert_eq!(fake.0.lock().unwrap().restarts, 1);
}

#[test]
fn a_reload_is_audited_when_it_ends() {
    let fake = Fake::default();
    let (_d, s) = faked("audit", &fake, true);
    let words: Arc<Mutex<Vec<(String, bool)>>> = Arc::new(Mutex::new(Vec::new()));
    let seen = words.clone();
    s.ops.reload().unwrap().set_audit(Arc::new(move |_, word, result| {
        seen.lock()
            .unwrap()
            .push((word.to_string(), result == DoneOrFailed::Done));
    }));
    reload(&s, "job-a", true).unwrap();
    assert_eq!(*words.lock().unwrap(), [("reload".to_string(), true)]);
}

#[test]
fn a_session_that_took_a_new_id_is_followed() {
    let fake = Fake::default();
    fake.0.lock().unwrap().moved_to = Some("new-id".to_string());
    let (_d, s) = faked("followed", &fake, true);
    let reply = reload(&s, "job-a", true).unwrap();
    assert_eq!(reply["task"]["session"], "new-id");
}

#[test]
fn a_finished_task_is_cleared_when_read_with_ack() {
    let fake = Fake::default();
    let (_d, s) = faked("ack", &fake, true);
    reload(&s, "job-a", true).unwrap();
    assert_eq!(phase_of(&s, "job-a")["phase"], "done");
    s.ops
        .call(
            &app(),
            ops::SESSION_RELOAD_STATE,
            &json!({"id": "job-a", "ack": true}),
        )
        .unwrap();
    assert!(phase_of(&s, "job-a").is_null());
}

fn live(tag: &str, launcher: Arc<FakeLauncher>, state_text: &str) -> (Dir, Arc<ActingOps>, PathBuf) {
    let (dir, roots) = roots(tag);
    let state = roots.main_dir.join("jobs/abcd1234/state.json");
    std::fs::write(&state, state_text).unwrap();
    let launcher: Arc<dyn Launcher> = launcher;
    let s = setup_in(true, PROMPT, sessions(), roots, Some(launcher));
    let ops = Arc::new(s.ops);
    ops.attach_reload(ReloadOps::inline(LiveHosts::new(&ops)));
    (dir, ops, state)
}

fn job_a(ops: &ActingOps) -> Agent {
    ops.agent(&json!({"id": "job-a"})).unwrap()
}

const IDLE: &str = r#"{"state":"done","respawnFlags":["--model","opus","--bg","--resume","old","--permission-mode","plan"],"inFlight":{"tasks":0}}"#;

#[test]
fn the_live_host_respawns_the_same_job_with_its_own_flags_untouched() {
    let launcher = Arc::new(FakeLauncher::new());
    let (_d, ops, state) = live("respawn", launcher.clone(), IDLE);
    let hosts = LiveHosts::new(&ops);
    let mut host = hosts.host(&job_a(&ops));
    assert_eq!(host.restart(), Ok(true));
    let commands = launcher.commands();
    assert_eq!(commands.len(), 1);
    assert_eq!(commands[0].argv, ["claude", "respawn", "abcd1234"]);
    let after: Value = serde_json::from_str(&std::fs::read_to_string(&state).unwrap()).unwrap();
    assert_eq!(
        after["respawnFlags"],
        json!(["--model", "opus", "--bg", "--resume", "old", "--permission-mode", "plan"])
    );
}

#[test]
fn background_tasks_from_the_jobs_file_hold_the_restart() {
    let launcher = Arc::new(FakeLauncher::new());
    let (_d, ops, _state) = live(
        "helpers",
        launcher.clone(),
        r#"{"state":"done","respawnFlags":["--model","opus"],"inFlight":{"tasks":3}}"#,
    );
    let hosts = LiveHosts::new(&ops);
    let mut host = hosts.host(&job_a(&ops));
    assert_eq!(host.tasks_in_flight(), Some(3));
    assert_eq!(host.restart(), Ok(false));
    assert!(launcher.commands().is_empty());
}

#[test]
fn a_working_session_is_not_restarted_by_the_live_host() {
    let launcher = Arc::new(FakeLauncher::new());
    let (_d, ops, _state) = live(
        "working",
        launcher.clone(),
        r#"{"state":"working","respawnFlags":["--model","opus"],"inFlight":{"tasks":0}}"#,
    );
    let hosts = LiveHosts::new(&ops);
    let mut host = hosts.host(&job_a(&ops));
    assert!(host.turn_running());
    assert_eq!(host.restart(), Ok(false));
    assert!(launcher.commands().is_empty());
}

#[test]
fn a_failed_respawn_is_reported_and_the_flags_stay() {
    let launcher = Arc::new(FakeLauncher::new());
    launcher.push_output(Ok(LaunchOutput::failed(1, "no such job")));
    let (_d, ops, state) = live("fails", launcher, IDLE);
    let hosts = LiveHosts::new(&ops);
    let mut host = hosts.host(&job_a(&ops));
    let failed = host.restart().unwrap_err();
    assert!(failed.contains("no such job"), "{failed}");
    assert_eq!(status::read_flags(&state).unwrap().len(), 7);
}

#[test]
fn the_live_host_stops_when_acting_is_switched_off() {
    let launcher = Arc::new(FakeLauncher::new());
    let (_d, ops, _state) = live("acting", launcher.clone(), IDLE);
    let hosts = LiveHosts::new(&ops);
    let mut host = hosts.host(&job_a(&ops));
    assert!(!host.cancelled());
    ops.acting_switch().set(false);
    assert!(host.cancelled());
    assert!(host.restart().is_err());
    assert!(launcher.commands().is_empty());
}

#[test]
fn a_mode_changed_during_the_session_is_carried_and_restored_on_failure() {
    let launcher = Arc::new(FakeLauncher::new());
    let (dir, ops, state) = live("mode", launcher.clone(), IDLE);
    let transcript = dir.0.join("t.jsonl");
    std::fs::write(
        &transcript,
        "{\"type\":\"permission-mode\",\"permissionMode\":\"acceptEdits\"}\n",
    )
    .unwrap();
    let text = IDLE.replace(
        "\"state\":\"done\"",
        &format!(
            "\"state\":\"done\",\"linkScanPath\":\"{}\"",
            transcript.display()
        ),
    );
    std::fs::write(&state, text).unwrap();
    let hosts = LiveHosts::new(&ops);
    let mut host = hosts.host(&job_a(&ops));
    assert_eq!(host.restart(), Ok(true));
    let flags = status::read_flags(&state).unwrap();
    assert_eq!(&flags[flags.len() - 2..], ["--permission-mode", "acceptEdits"]);
    assert!(status::read_pending(&state).is_none());
    launcher.push_output(Ok(LaunchOutput::failed(1, "no such job")));
    std::fs::write(&state, std::fs::read_to_string(&state).unwrap().replace("acceptEdits", "plan"))
        .unwrap();
    let before = status::read_flags(&state).unwrap();
    assert!(host.restart().is_err());
    assert_eq!(status::read_flags(&state).unwrap(), before);
    assert!(status::read_pending(&state).is_none());
}

#[test]
fn a_queued_reply_stays_queued_through_a_reload() {
    let fake = Fake::default();
    fake.0.lock().unwrap().turns = VecDeque::from([true, false]);
    let (_d, s) = faked("queued", &fake, true);
    let store = Arc::new(OutboxStore::in_memory_for_tests());
    let outbox = Arc::new(OutboxOps::new(
        store.clone(),
        Arc::new(Mutex::new(sessions())),
        s.ops.roots.clone(),
        s.ops.acting_switch(),
        s.room.clone(),
        None,
    ));
    outbox
        .call(
            &app(),
            ops::SESSION_SEND,
            &json!({"id": "job-idle", "text": "after the reload"}),
        )
        .unwrap();
    s.ops.attach_outbox(outbox);
    reload(&s, "job-a", true).unwrap();
    let rows = store.list(Some("idle0001")).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].text, "after the reload");
    assert_eq!(rows[0].state, State::Queued);
    assert!(s.writes.lock().unwrap().is_empty());
    assert!(s.attached.lock().unwrap().is_empty());
}
