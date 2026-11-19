use crate::acting_ops::ActingOps;
use crate::acting_ops_tests::{app, code, sessions, setup_in, Setup, PROMPT};
use crate::chrome_ops::{ChromeOps, Hosts, LiveHosts};
use notchling_engine::chrome::drive::{Goal, Reconnected, Refusal, Report};
use notchling_engine::chrome::flow::{Host, POLL_MS};
use notchling_engine::chrome::menu;
use notchling_engine::chrome::status::{self, Detected};
use notchling_engine::control::{FakeLauncher, LaunchOutput, Launcher};
use notchling_engine::outbox::Tool;
use notchling_engine::sessions::{Agent, Roots};
use notchling_local_api::{ops, ErrorCode};
use serde_json::{json, Value};
use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

const MENU_ON: &str = include_str!("../../engine/tests/fixtures/chrome/menu-on.txt");
const MENU_OFF: &str = include_str!("../../engine/tests/fixtures/chrome/menu-off.txt");

struct Dir(PathBuf);

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn menu_of(text: &str) -> menu::Menu {
    let lines: Vec<String> = text.lines().skip(1).map(String::from).collect();
    menu::read(&lines).unwrap()
}

fn report(text: &str) -> Report {
    Report {
        menu: menu_of(text),
        reconnect: Reconnected::Skipped,
    }
}

#[derive(Default)]
struct Log {
    turns: VecDeque<bool>,
    looks: VecDeque<Result<Report, Refusal>>,
    restarts: Vec<bool>,
    sleeps: Vec<u64>,
    asked_turn: usize,
    looked: usize,
}

#[derive(Clone, Default)]
struct Fake(Arc<Mutex<Log>>);

struct FakeHost(Fake);

impl Host for FakeHost {
    fn turn_running(&mut self) -> bool {
        let mut log = (self.0).0.lock().unwrap();
        log.asked_turn += 1;
        log.turns.pop_front().unwrap_or(false)
    }

    fn tasks_in_flight(&mut self) -> u32 {
        0
    }

    fn look(&mut self, _goal: Goal) -> Result<Report, Refusal> {
        let mut log = (self.0).0.lock().unwrap();
        log.looked += 1;
        log.looks.pop_front().unwrap_or(Err(Refusal::Stuck))
    }

    fn restart(&mut self, on: bool) -> Result<(), String> {
        (self.0).0.lock().unwrap().restarts.push(on);
        Ok(())
    }

    fn sleep(&mut self, ms: u64) {
        (self.0).0.lock().unwrap().sleeps.push(ms);
    }

    fn now_ms(&mut self) -> u64 {
        0
    }
}

impl Hosts for Fake {
    fn host(&self, _agent: &Agent) -> Box<dyn Host + Send> {
        Box::new(FakeHost(self.clone()))
    }

    fn detect(&self, _agent: &Agent) -> Detected {
        status::detect(None, None)
    }
}

fn roots(tag: &str) -> (Dir, Roots) {
    let dir = std::env::temp_dir().join(format!("nl-chrome-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("claude/jobs/abcd1234")).unwrap();
    let roots = Roots {
        main_dir: dir.join("claude"),
        lanes_file: dir.join("lanes.json"),
    };
    (Dir(dir), roots)
}

fn faked(tag: &str, fake: &Fake) -> (Dir, Setup) {
    let (dir, roots) = roots(tag);
    let s = setup_in(true, PROMPT, sessions(), roots, None);
    s.ops
        .attach_chrome(ChromeOps::inline(Arc::new(fake.clone())));
    (dir, s)
}

fn command(s: &Setup, id: &str, text: &str) -> Result<Value, (ErrorCode, String)> {
    s.ops
        .call(
            &app(),
            ops::SESSION_COMMAND,
            &json!({"id": id, "command": text}),
        )
        .map_err(|e| (e.code, e.message))
}

fn reading(fake: &Fake, looks: Vec<Result<Report, Refusal>>) {
    fake.0.lock().unwrap().looks = looks.into();
}

#[test]
fn chrome_is_caught_and_never_typed_into_the_session() {
    let fake = Fake::default();
    reading(&fake, vec![Ok(report(MENU_ON))]);
    let (_d, s) = faked("caught", &fake);
    let reply = command(&s, "job-a", "/chrome").unwrap();
    assert_eq!(reply["sent"], false);
    assert!(s.writes.lock().unwrap().is_empty());
    assert!(s.attached.lock().unwrap().is_empty());
}

#[test]
fn chrome_already_on_ends_with_a_note_and_changes_nothing() {
    let fake = Fake::default();
    reading(&fake, vec![Ok(report(MENU_ON))]);
    let (_d, s) = faked("on", &fake);
    let reply = command(&s, "job-a", "/chrome").unwrap();
    assert_eq!(reply["chrome"]["task"]["phase"], "done");
    assert_eq!(reply["chrome"]["task"]["result"], "already_on");
    assert!(fake.0.lock().unwrap().restarts.is_empty());
}

#[test]
fn chrome_off_is_turned_on_at_once_with_no_sheet() {
    let fake = Fake::default();
    reading(&fake, vec![Ok(report(MENU_OFF)), Ok(report(MENU_ON))]);
    let (_d, s) = faked("off", &fake);
    let reply = command(&s, "job-a", "/chrome").unwrap();
    assert_eq!(reply["chrome"]["task"]["phase"], "done");
    assert_eq!(reply["chrome"]["task"]["result"], "on");
    assert_eq!(fake.0.lock().unwrap().restarts, [true]);
}

#[test]
fn chrome_off_command_turns_it_off() {
    let fake = Fake::default();
    reading(&fake, vec![Ok(report(MENU_ON)), Ok(report(MENU_OFF))]);
    let (_d, s) = faked("turnoff", &fake);
    let reply = command(&s, "job-a", "/chrome off").unwrap();
    assert_eq!(reply["chrome"]["task"]["result"], "off");
    assert_eq!(fake.0.lock().unwrap().restarts, [false]);
}

#[test]
fn a_running_turn_is_waited_out_before_anything_else() {
    let fake = Fake::default();
    fake.0.lock().unwrap().turns = VecDeque::from([true, true, false]);
    reading(&fake, vec![Ok(report(MENU_ON))]);
    let (_d, s) = faked("waits", &fake);
    let reply = command(&s, "job-a", "/chrome").unwrap();
    assert_eq!(reply["chrome"]["task"]["phase"], "done");
    let log = fake.0.lock().unwrap();
    assert_eq!(log.sleeps, [POLL_MS, POLL_MS]);
    assert_eq!(log.asked_turn, 3);
    assert_eq!(log.looked, 1);
}

#[test]
fn something_for_the_person_is_handed_back_and_cleared_when_read() {
    let fake = Fake::default();
    reading(&fake, vec![Err(Refusal::Unavailable)]);
    let (_d, s) = faked("needs", &fake);
    let reply = command(&s, "job-a", "/chrome").unwrap();
    assert_eq!(reply["chrome"]["task"]["phase"], "needs_you");
    assert_eq!(reply["chrome"]["task"]["need"]["need"], "unavailable");
    let read = |ack: bool| {
        s.ops
            .call(
                &app(),
                ops::SESSION_CHROME,
                &json!({"id": "job-a", "ack": ack}),
            )
            .unwrap()
    };
    assert_eq!(read(false)["task"]["phase"], "needs_you");
    assert_eq!(read(true)["task"], Value::Null);
    assert_eq!(read(false)["task"], Value::Null);
    assert_eq!(read(false)["offered"], true);
}

#[test]
fn only_a_claude_code_job_on_this_mac_takes_it() {
    let fake = Fake::default();
    let (_d, s) = faked("only", &fake);
    let (code, message) = command(&s, "term", "/chrome").unwrap_err();
    assert_eq!(code, ErrorCode::BadRequest);
    assert!(message.contains("Claude Code"), "{message}");
    assert!(s.writes.lock().unwrap().is_empty());
    let read = s
        .ops
        .call(&app(), ops::SESSION_CHROME, &json!({"id": "term"}))
        .unwrap();
    assert_eq!(read["offered"], false);
}

#[test]
fn agents_and_the_command_line_cannot_use_it() {
    let fake = Fake::default();
    let (_d, s) = faked("tier", &fake);
    for op in [ops::SESSION_COMMAND, ops::SESSION_CHROME] {
        let spec = s.ops.spec(op).unwrap();
        assert_eq!(spec, notchling_local_api::OpSpec::APP, "{op}");
    }
}

#[test]
fn acting_off_refuses_it_like_any_command() {
    let fake = Fake::default();
    let (_d, roots) = roots("actoff");
    let s = setup_in(false, PROMPT, sessions(), roots, None);
    s.ops
        .attach_chrome(ChromeOps::inline(Arc::new(fake.clone())));
    assert_eq!(
        code(s.ops.call(
            &app(),
            ops::SESSION_COMMAND,
            &json!({"id": "job-a", "command": "/chrome"})
        ))
        .0,
        ErrorCode::Forbidden
    );
    assert_eq!(fake.0.lock().unwrap().looked, 0);
}

#[test]
fn a_queued_reply_never_carries_it() {
    assert!(crate::outbox_ops::refuse_menu(Tool::Claude, "/chrome").is_err());
    assert!(crate::outbox_ops::refuse_menu(Tool::Claude, "/chrome off").is_err());
    assert!(crate::outbox_ops::refuse_menu(Tool::Codex, "/chrome").is_ok());
    assert!(crate::outbox_ops::refuse_menu(Tool::Claude, "open /chrome later").is_ok());
}

fn live(tag: &str, launcher: Arc<FakeLauncher>, flags: &str) -> (Dir, Arc<ActingOps>, PathBuf) {
    let (dir, roots) = roots(tag);
    let state = roots.main_dir.join("jobs/abcd1234/state.json");
    std::fs::write(
        &state,
        format!(r#"{{"state":"done","respawnFlags":{flags},"inFlight":{{"tasks":0}}}}"#),
    )
    .unwrap();
    let launcher: Arc<dyn Launcher> = launcher;
    let s = setup_in(true, PROMPT, sessions(), roots, Some(launcher));
    let ops = Arc::new(s.ops);
    ops.attach_chrome(ChromeOps::inline(LiveHosts::new(&ops)));
    (dir, ops, state)
}

fn job_a(ops: &ActingOps) -> Agent {
    ops.agent(&json!({"id": "job-a"})).unwrap()
}

#[test]
fn turning_it_on_edits_the_flags_and_respawns_the_same_job() {
    let launcher = Arc::new(FakeLauncher::new());
    let (_d, ops, state) = live(
        "respawn",
        launcher.clone(),
        r#"["--name","demo","--no-chrome"]"#,
    );
    let hosts = LiveHosts::new(&ops);
    let mut host = hosts.host(&job_a(&ops));
    host.restart(true).unwrap();
    assert_eq!(
        status::read_flags(&state).unwrap(),
        ["--name", "demo", "--chrome"]
    );
    let commands = launcher.commands();
    assert_eq!(commands.len(), 1);
    assert_eq!(commands[0].argv, ["claude", "respawn", "abcd1234"]);
    let detected = hosts.detect(&job_a(&ops));
    assert_eq!(detected.on, Some(true));
}

#[test]
fn a_respawn_that_fails_puts_the_flags_back() {
    let launcher = Arc::new(FakeLauncher::new());
    launcher.push_output(Ok(LaunchOutput::failed(1, "no such job")));
    let (_d, ops, state) = live("rollback", launcher, r#"["--name","demo"]"#);
    let hosts = LiveHosts::new(&ops);
    let mut host = hosts.host(&job_a(&ops));
    let failed = host.restart(true).unwrap_err();
    assert!(failed.contains("no such job"), "{failed}");
    assert_eq!(status::read_flags(&state).unwrap(), ["--name", "demo"]);
}

#[test]
fn background_tasks_are_counted_from_the_jobs_file() {
    let launcher = Arc::new(FakeLauncher::new());
    let (_d, ops, state) = live("tasks", launcher, r#"["--no-chrome"]"#);
    std::fs::write(
        &state,
        r#"{"state":"done","respawnFlags":["--no-chrome"],"inFlight":{"tasks":3}}"#,
    )
    .unwrap();
    let hosts = LiveHosts::new(&ops);
    let mut host = hosts.host(&job_a(&ops));
    assert_eq!(host.tasks_in_flight(), 3);
}
