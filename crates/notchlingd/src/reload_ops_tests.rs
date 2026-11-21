use crate::acting_ops::{ActingOps, Room};
use crate::acting_ops_tests::{app, caller, code, sessions, setup_in, Setup, GONE, PROMPT};
use crate::op_group::OpGroup;
use crate::outbox_ops::OutboxOps;
use crate::reload_ops::{Hosts, LiveHosts, ReloadOps};
use notchling_engine::chrome::status;
use notchling_engine::control::{FakeLauncher, LaunchOutput, Launcher};
use notchling_engine::outbox::{OutboxStore, State};
use notchling_engine::reload::flow::{Host, POLL_MS};
use notchling_engine::sessions::{Agent, Roots};
use notchling_events::kinds::choices::DoneOrFailed;
use notchling_local_api::Handler;
use notchling_local_api::{ops, Caller, ClientKind, ErrorCode, Tier};
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

type Probe = Arc<dyn Fn() + Send + Sync>;

#[derive(Default)]
struct Log {
    on_tasks: Option<Probe>,
    crash: bool,
    hosts: u32,
    most_hosts: u32,
    turns: VecDeque<bool>,
    dialogs: VecDeque<bool>,
    tasks: VecDeque<Option<u32>>,
    restarts: u32,
    sleeps: Vec<u64>,
    hold: bool,
    slow: bool,
    gate: bool,
    now: u64,
    moved_to: Option<String>,
}

#[derive(Clone, Default)]
struct Fake(Arc<Mutex<Log>>);

struct FakeHost(Fake);

impl Drop for FakeHost {
    fn drop(&mut self) {
        let mut log = (self.0).0.lock().unwrap_or_else(|e| e.into_inner());
        log.hosts -= 1;
    }
}

impl Host for FakeHost {
    fn turn_running(&mut self) -> bool {
        let crash = (self.0).0.lock().unwrap().crash;
        assert!(!crash, "the fake host crashed");
        let mut log = (self.0).0.lock().unwrap();
        log.hold || log.turns.pop_front().unwrap_or(false)
    }
    fn dialog_open(&mut self) -> bool {
        (self.0)
            .0
            .lock()
            .unwrap()
            .dialogs
            .pop_front()
            .unwrap_or(false)
    }
    fn tasks_in_flight(&mut self) -> Option<u32> {
        let probe = (self.0).0.lock().unwrap().on_tasks.clone();
        if let Some(probe) = probe {
            probe();
        }
        (self.0)
            .0
            .lock()
            .unwrap()
            .tasks
            .pop_front()
            .unwrap_or(Some(0))
    }
    fn restart(&mut self) -> Result<bool, String> {
        loop {
            {
                let mut log = (self.0).0.lock().unwrap();
                if !log.gate {
                    log.restarts += 1;
                    return Ok(true);
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
    }
    fn sleep(&mut self, ms: u64) {
        let hold = {
            let mut log = (self.0).0.lock().unwrap();
            log.sleeps.push(ms);
            log.now += ms;
            log.hold || log.slow
        };
        if hold {
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
    }
    fn now_ms(&mut self) -> u64 {
        (self.0).0.lock().unwrap().now
    }
    fn real_time(&self) -> bool {
        (self.0).0.lock().unwrap().slow
    }
}

impl Hosts for Fake {
    fn host(&self, _agent: &Agent) -> Box<dyn Host + Send> {
        let mut log = self.0.lock().unwrap();
        log.hosts += 1;
        log.most_hosts = log.most_hosts.max(log.hosts);
        drop(log);
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
    reload.set_publish(crate::reload_ops::publisher(s.room.clone()));
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
        Some(notchling_local_api::OpSpec::AGENT_ACTS)
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
    fake.0.lock().unwrap().tasks = VecDeque::from(vec![Some(3); 600]);
    fake.0.lock().unwrap().slow = true;
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
    assert!(message.contains("claude --resume 'term'"), "{message}");
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
    s.ops
        .reload()
        .unwrap()
        .set_audit(Arc::new(move |_, word, result| {
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
fn a_finished_task_is_cleared_only_by_an_ack_of_the_rev_that_was_read() {
    let fake = Fake::default();
    let (_d, s) = faked("ack", &fake, true);
    let done = reload(&s, "job-a", true).unwrap();
    assert_eq!(phase_of(&s, "job-a")["phase"], "done");
    let rev = done["rev"].as_u64().unwrap();
    let ack = |rev: u64| {
        s.ops
            .call(
                &app(),
                ops::SESSION_RELOAD_STATE,
                &json!({"id": "job-a", "ack": true, "rev": rev}),
            )
            .unwrap()
    };
    ack(rev - 1);
    assert_eq!(phase_of(&s, "job-a")["phase"], "done");
    let cleared = ack(rev);
    assert!(cleared["task"].is_null());
    assert!(cleared["rev"].as_u64().unwrap() > rev);
    assert!(phase_of(&s, "job-a").is_null());
}

fn live(
    tag: &str,
    launcher: Arc<FakeLauncher>,
    state_text: &str,
) -> (Dir, Arc<ActingOps>, PathBuf) {
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
        json!([
            "--model",
            "opus",
            "--bg",
            "--resume",
            "old",
            "--permission-mode",
            "plan"
        ])
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
    assert_eq!(
        &flags[flags.len() - 2..],
        ["--permission-mode", "acceptEdits"]
    );
    assert!(status::read_pending(&state).is_none());
    launcher.push_output(Ok(LaunchOutput::failed(1, "no such job")));
    std::fs::write(
        &state,
        std::fs::read_to_string(&state)
            .unwrap()
            .replace("acceptEdits", "plan"),
    )
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

fn agent_tier(pid: Option<u32>) -> Caller {
    Caller {
        tier: Tier::Agent,
        client: ClientKind::Cli,
        peer: None,
        pid,
    }
}

#[test]
fn an_agent_whose_session_cannot_be_resolved_may_reload_nothing() {
    let fake = Fake::default();
    let (_d, s) = faked("unresolved", &fake, true);
    for who in [
        agent_tier(None),
        agent_tier(Some(GONE)),
        caller(ClientKind::Cli),
    ] {
        for args in [
            json!({"id": "job-a", "afterTurn": false}),
            json!({"id": "self"}),
            json!({"id": "job-a", "afterTurn": true}),
            json!({"id": "job-a", "cancel": true}),
        ] {
            let refused = code(s.ops.call(&who, ops::SESSION_RELOAD, &args));
            assert_eq!(refused.0, ErrorCode::Forbidden, "{args}");
        }
    }
    assert_eq!(fake.0.lock().unwrap().restarts, 0);
    assert!(phase_of(&s, "job-a").is_null());
}

#[test]
fn an_agent_naming_another_session_is_refused_even_with_its_own_resolved() {
    let fake = Fake::default();
    let (d, s) = faked("another", &fake, true);
    inside(&s, &d, "job-a");
    let refused = code(s.ops.call(
        &caller(ClientKind::Cli),
        ops::SESSION_RELOAD,
        &json!({"id": "job-idle", "afterTurn": false}),
    ));
    assert_eq!(refused.0, ErrorCode::Forbidden);
    assert_eq!(fake.0.lock().unwrap().restarts, 0);
}

#[test]
fn the_app_may_reload_any_session_it_lists() {
    let fake = Fake::default();
    let (_d, s) = faked("app-any", &fake, true);
    let reply = s
        .ops
        .call(
            &app(),
            ops::SESSION_RELOAD,
            &json!({"id": "job-a", "afterTurn": true}),
        )
        .unwrap();
    assert_eq!(reply["task"]["phase"], "done");
}

fn with_mode_in_transcript(dir: &Dir, state: &PathBuf, flags: &str, mode: &str) {
    let transcript = dir.0.join("t.jsonl");
    std::fs::write(
        &transcript,
        format!("{{\"type\":\"permission-mode\",\"permissionMode\":\"{mode}\"}}\n"),
    )
    .unwrap();
    std::fs::write(
        state,
        format!(
            "{{\"state\":\"done\",\"linkScanPath\":\"{}\",\"respawnFlags\":{flags},\"inFlight\":{{\"tasks\":0}}}}",
            transcript.display()
        ),
    )
    .unwrap();
}

#[test]
fn skipping_permissions_is_kept_exactly_whatever_the_transcript_says() {
    let launcher = Arc::new(FakeLauncher::new());
    let (dir, ops, state) = live("skip", launcher.clone(), IDLE);
    let flags = r#"["--model","opus","--bg","--dangerously-skip-permissions"]"#;
    with_mode_in_transcript(&dir, &state, flags, "default");
    let before = status::read_flags(&state).unwrap();
    let hosts = LiveHosts::new(&ops);
    let mut host = hosts.host(&job_a(&ops));
    assert_eq!(host.restart(), Ok(true));
    assert_eq!(status::read_flags(&state).unwrap(), before);
    assert!(status::read_pending(&state).is_none());
    assert_eq!(launcher.commands().len(), 1);
}

#[test]
fn a_respawn_that_times_out_restores_the_flags_and_clears_pending() {
    let launcher = Arc::new(FakeLauncher::new());
    launcher.push_output(Err(notchling_engine::control::LaunchError::Timeout(60)));
    let (dir, ops, state) = live("timeout", launcher, IDLE);
    let flags = r#"["--model","opus","--bg","--permission-mode","plan"]"#;
    with_mode_in_transcript(&dir, &state, flags, "acceptEdits");
    let before = status::read_flags(&state).unwrap();
    let hosts = LiveHosts::new(&ops);
    let mut host = hosts.host(&job_a(&ops));
    let failed = host.restart().unwrap_err();
    assert!(
        failed.to_lowercase().contains("time") || failed.contains("slow"),
        "{failed}"
    );
    assert_eq!(status::read_flags(&state).unwrap(), before);
    assert!(status::read_pending(&state).is_none());
}

#[test]
fn a_cancel_is_written_by_the_worker_and_a_new_start_runs_one_worker_at_a_time() {
    let fake = Fake::default();
    fake.0.lock().unwrap().tasks = VecDeque::from(vec![Some(2); 400]);
    fake.0.lock().unwrap().slow = true;
    let (_d, s) = faked("again", &fake, false);
    reload(&s, "job-a", true).unwrap();
    let asked = cancel(&s, "job-a").unwrap();
    assert_eq!(asked["task"]["cancelling"], true);
    assert!(wait_over(&s, "job-a"));
    assert_eq!(phase_of(&s, "job-a")["phase"], "cancelled");
    let again = reload(&s, "job-a", true).unwrap();
    assert_eq!(again["task"]["phase"], "waiting");
    assert!(again["task"].get("cancelling").is_none());
    std::thread::sleep(std::time::Duration::from_millis(100));
    assert_eq!(phase_of(&s, "job-a")["phase"], "waiting");
    assert_eq!(fake.0.lock().unwrap().restarts, 0);
    assert_eq!(fake.0.lock().unwrap().most_hosts, 1);
    cancel(&s, "job-a").unwrap();
    assert!(wait_over(&s, "job-a"));
}

fn wait_over(s: &Setup, id: &str) -> bool {
    s.ops
        .reload()
        .unwrap()
        .wait_over(id, std::time::Duration::from_secs(10))
}

fn until_phase(s: &Setup, id: &str, phase: &str) {
    for _ in 0..1000 {
        if phase_of(s, id)["phase"] == phase {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    panic!("never reached {phase}");
}

fn cancel(s: &Setup, id: &str) -> Result<Value, (ErrorCode, String)> {
    s.ops
        .call(
            &app(),
            ops::SESSION_RELOAD,
            &json!({"id": id, "cancel": true}),
        )
        .map_err(|e| (e.code, e.message))
}

#[test]
fn a_reload_inside_the_restart_cannot_be_cancelled_and_finishes() {
    let fake = Fake::default();
    fake.0.lock().unwrap().gate = true;
    let (_d, s) = faked("working-cancel", &fake, false);
    reload(&s, "job-a", true).unwrap();
    until_phase(&s, "job-a", "working");
    let refused = cancel(&s, "job-a").unwrap_err();
    assert_eq!(refused.0, ErrorCode::BadRequest);
    assert_eq!(refused.1, crate::reload_ops::RESTARTING);
    assert_eq!(phase_of(&s, "job-a")["phase"], "working");
    fake.0.lock().unwrap().gate = false;
    assert_eq!(until_over(&s, "job-a")["phase"], "done");
    assert_eq!(fake.0.lock().unwrap().restarts, 1);
}

#[test]
fn a_second_start_while_a_worker_is_inside_the_restart_starts_nothing() {
    let fake = Fake::default();
    fake.0.lock().unwrap().gate = true;
    let (_d, s) = faked("working-start", &fake, false);
    reload(&s, "job-a", true).unwrap();
    until_phase(&s, "job-a", "working");
    let seen = s
        .ops
        .call(&app(), ops::SESSION_RELOAD_STATE, &json!({"id": "job-a"}))
        .unwrap();
    let again = reload(&s, "job-a", true).unwrap();
    assert_eq!(again["task"]["phase"], "working");
    assert_eq!(again["rev"], seen["rev"]);
    assert_eq!(again["task"]["since_ms"], seen["task"]["since_ms"]);
    assert_eq!(fake.0.lock().unwrap().most_hosts, 1);
    fake.0.lock().unwrap().gate = false;
    assert_eq!(until_over(&s, "job-a")["phase"], "done");
    std::thread::sleep(std::time::Duration::from_millis(100));
    assert_eq!(fake.0.lock().unwrap().restarts, 1);
}

#[test]
fn the_flags_swap_never_writes_a_pending_record() {
    let launcher = Arc::new(FakeLauncher::new());
    launcher.set_delay(std::time::Duration::from_millis(300));
    let (dir, ops, state) = live("no-pending", launcher, IDLE);
    let flags = r#"["--model","opus","--bg","--permission-mode","plan"]"#;
    with_mode_in_transcript(&dir, &state, flags, "acceptEdits");
    let watched = state.clone();
    let seen = Arc::new(Mutex::new(false));
    let flag = seen.clone();
    let watcher = std::thread::spawn(move || {
        for _ in 0..150 {
            if status::read_pending(&watched).is_some() {
                *flag.lock().unwrap() = true;
            }
            std::thread::sleep(std::time::Duration::from_millis(3));
        }
    });
    let hosts = LiveHosts::new(&ops);
    let mut host = hosts.host(&job_a(&ops));
    assert_eq!(host.restart(), Ok(true));
    watcher.join().unwrap();
    assert!(!*seen.lock().unwrap());
    let after = status::read_flags(&state).unwrap();
    assert_eq!(
        &after[after.len() - 2..],
        ["--permission-mode", "acceptEdits"]
    );
}

#[test]
fn cancel_wakes_a_waiting_worker_at_once() {
    let fake = Fake::default();
    fake.0.lock().unwrap().tasks = VecDeque::from(vec![Some(2); 50]);
    fake.0.lock().unwrap().slow = true;
    let (_d, s) = faked("wake", &fake, false);
    reload(&s, "job-a", true).unwrap();
    until_phase(&s, "job-a", "waiting");
    let started = std::time::Instant::now();
    cancel(&s, "job-a").unwrap();
    assert!(wait_over(&s, "job-a"));
    assert!(started.elapsed() < std::time::Duration::from_millis(1_500));
    assert_eq!(phase_of(&s, "job-a")["phase"], "cancelled");
}

#[test]
fn the_app_reads_every_task_with_its_rev_and_each_change_is_pushed_in_order() {
    let fake = Fake::default();
    fake.0.lock().unwrap().turns = VecDeque::from([true]);
    let (_d, s) = faked("all", &fake, true);
    let none = s
        .ops
        .call(&app(), ops::SESSION_RELOAD_STATE, &json!({"all": true}))
        .unwrap();
    assert_eq!(none["tasks"], json!([]));
    reload(&s, "job-a", true).unwrap();
    let pushed = s.room.named(notchling_local_api::topics::RELOAD_CHANGED);
    let phases: Vec<&str> = pushed
        .iter()
        .map(|e| e["task"]["phase"].as_str().unwrap_or("gone"))
        .collect();
    assert_eq!(phases, ["waiting", "working", "done"]);
    let revs: Vec<u64> = pushed.iter().map(|e| e["rev"].as_u64().unwrap()).collect();
    assert!(revs.windows(2).all(|w| w[0] < w[1]), "{revs:?}");
    assert!(pushed.iter().all(|e| e["id"] == "job-a"));
    let all = s
        .ops
        .call(&app(), ops::SESSION_RELOAD_STATE, &json!({"all": true}))
        .unwrap();
    assert_eq!(all["tasks"][0]["id"], "job-a");
    assert_eq!(all["tasks"][0]["task"]["phase"], "done");
    assert_eq!(all["tasks"][0]["rev"], json!(revs[2]));
}

#[test]
fn a_cancel_that_lands_just_before_the_restart_wins_and_nothing_restarts() {
    let fake = Fake::default();
    let (_d, s) = faked("gate", &fake, false);
    let reload_ops = s.ops.reload().unwrap().clone();
    let agent = s.ops.agent(&json!({"id": "job-a"})).unwrap();
    let answer = Arc::new(Mutex::new(None));
    let seen = answer.clone();
    fake.0.lock().unwrap().on_tasks = Some(Arc::new(move || {
        let mut seen = seen.lock().unwrap();
        if seen.is_none() {
            *seen = Some(reload_ops.cancel(&agent).map_err(|e| e.message));
        }
    }));
    reload(&s, "job-a", true).unwrap();
    assert!(wait_over(&s, "job-a"));
    let answer = answer.lock().unwrap().clone().unwrap().unwrap();
    assert_eq!(answer["task"]["cancelling"], true);
    assert_eq!(phase_of(&s, "job-a")["phase"], "cancelled");
    assert_eq!(fake.0.lock().unwrap().restarts, 0);
}

#[test]
fn a_worker_that_crashes_leaves_a_failure_and_the_next_start_runs() {
    let fake = Fake::default();
    fake.0.lock().unwrap().crash = true;
    let (_d, s) = faked("crash", &fake, false);
    reload(&s, "job-a", true).unwrap();
    assert!(wait_over(&s, "job-a"));
    let task = phase_of(&s, "job-a");
    assert_eq!(task["phase"], "needs_you");
    assert_eq!(task["need"]["text"], crate::reload_ops::STOPPED);
    fake.0.lock().unwrap().crash = false;
    reload(&s, "job-a", true).unwrap();
    assert!(wait_over(&s, "job-a"));
    assert_eq!(phase_of(&s, "job-a")["phase"], "done");
    assert_eq!(fake.0.lock().unwrap().restarts, 1);
}

#[derive(Default)]
struct Topics(Mutex<Vec<(String, String)>>);

impl Room for Topics {
    fn publish(&self, topic: &str, event: &str, _: &Value) {
        self.0
            .lock()
            .unwrap()
            .push((topic.to_string(), event.to_string()));
    }
    fn can_answer(&self) -> bool {
        true
    }
    fn can_follow(&self, _: &str) -> bool {
        true
    }
}

#[test]
fn reload_changes_go_on_their_own_app_tier_topic_never_on_prompts() {
    let room = Arc::new(Topics::default());
    let publish = crate::reload_ops::publisher(room.clone());
    publish(&json!({"id": "job-a", "rev": 1, "task": null}));
    assert_eq!(
        *room.0.lock().unwrap(),
        [("reloads".to_string(), "reload.changed".to_string())]
    );
    let launcher = Arc::new(FakeLauncher::new());
    let (_d, acting, _) = live("topic", launcher, IDLE);
    let daemon = crate::daemon::Ops::with_acting(Arc::new(Mutex::new(json!([]))), acting);
    assert!(daemon.topics().contains(&"reloads"));
    assert_eq!(daemon.topic_tier("reloads"), Tier::App);
    assert_eq!(daemon.topic_tier("prompts"), Tier::App);
}

#[test]
fn an_ack_without_a_rev_still_clears_a_finished_task_for_older_callers() {
    let fake = Fake::default();
    let (_d, s) = faked("ack-any", &fake, true);
    reload(&s, "job-a", true).unwrap();
    s.ops
        .call(
            &app(),
            ops::SESSION_RELOAD_STATE,
            &json!({"id": "job-a", "ack": true}),
        )
        .unwrap();
    assert!(phase_of(&s, "job-a").is_null());
}

#[test]
fn the_full_read_names_the_daemon_run_so_the_app_can_tell_a_restart() {
    let fake = Fake::default();
    let (_d, s) = faked("boot", &fake, true);
    let all = s
        .ops
        .call(&app(), ops::SESSION_RELOAD_STATE, &json!({"all": true}))
        .unwrap();
    let boot = all["boot"].as_str().unwrap();
    assert!(
        boot.starts_with(&format!("{}-", std::process::id())),
        "{boot}"
    );
}
