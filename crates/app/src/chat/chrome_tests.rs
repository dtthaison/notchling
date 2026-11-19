use notchling_engine::chrome::flow::{Done, Need, Phase, Step, Wait};
use notchling_engine::sessions::{Agent, AgentGroup, AgentTarget, NodeRef};
use serde_json::{json, Value};

use super::chrome::{
    command, offered, parse, Act, Applied, Sheet, State, Status, Task, NOTE_MS, POLL_MS,
};
use super::data::{Session, Tool};
use crate::acting::{ops, Request};

fn agent(target: AgentTarget) -> Agent {
    Agent {
        id: "s1".into(),
        name: "job".into(),
        project: "api".into(),
        cwd: "/srv/api".into(),
        group: AgentGroup::Working,
        label: "Working".into(),
        detail: None,
        since_ms: 1,
        target,
        suggested_reply: None,
        transcript_path: None,
        host: "Background".into(),
        headless: false,
        config_dir: None,
        command: None,
        resume_session_id: None,
        at_prompt: false,
        node: None,
        worktree_path: None,
    }
}

fn session(tool: Tool, agent: Agent) -> Session {
    Session {
        agent,
        tool,
        transcript: None,
    }
}

fn reply(task: Value, on: Option<bool>) -> Value {
    json!({"offered": true, "task": task,
        "detected": {"on": on, "basis": "flag", "default_enabled": null}})
}

fn status(task: Value, on: Option<bool>) -> Status {
    parse(&reply(task, on)).unwrap()
}

#[test]
fn the_menu_item_is_only_for_claude_code_sessions_on_this_mac() {
    let job = || agent(AgentTarget::Job("j1".into()));
    assert!(offered(&session(Tool::Claude, job())));
    assert!(!offered(&session(Tool::Codex, job())));
    assert!(!offered(&session(Tool::Antigravity, job())));
    assert!(!offered(&session(
        Tool::Claude,
        agent(AgentTarget::Process(42))
    )));
    let mut headless = job();
    headless.headless = true;
    assert!(!offered(&session(Tool::Claude, headless)));
    let mut shell = job();
    shell.command = Some("npm run dev".into());
    assert!(!offered(&session(Tool::Claude, shell)));
    let mut remote = job();
    remote.node = Some(NodeRef {
        id: "n1".into(),
        name: "worker".into(),
        online: true,
    });
    assert!(!offered(&session(Tool::Claude, remote)));
}

#[test]
fn the_requests_go_through_the_command_op_and_the_quiet_read() {
    let on = Request::chrome("s1", true, false);
    assert_eq!(on.op, ops::SESSION_COMMAND);
    assert_eq!(on.args, json!({"id": "s1", "command": "/chrome"}));
    let off = Request::chrome("s1", false, false);
    assert_eq!(off.args["command"], "/chrome off");
    let forced = Request::chrome("s1", true, true);
    assert_eq!(forced.args["force"], true);
    assert_eq!(command(true), "/chrome");
    let read = Request::chrome_state("s1", false);
    assert_eq!(read.op, ops::SESSION_CHROME);
    assert_eq!(read.args, json!({"id": "s1"}));
    assert_eq!(Request::chrome_state("s1", true).args["ack"], true);
}

#[test]
fn the_daemons_state_is_read_with_its_task_and_detected_choice() {
    let parsed = status(
        json!({"phase": "working", "step": "restarting", "on": true, "since_ms": 7}),
        Some(false),
    );
    assert!(parsed.offered);
    assert_eq!(parsed.on, Some(false));
    assert_eq!(
        parsed.task,
        Some(Task {
            on: true,
            phase: Phase::Working {
                step: Step::Restarting
            },
            since_ms: 7
        })
    );
    let idle = parse(&json!({"offered": true, "task": null, "detected": {"on": null}})).unwrap();
    assert_eq!(idle.task, None);
    assert_eq!(idle.on, None);
    assert_eq!(parse(&json!({"nope": 1})), None);
}

#[test]
fn the_notes_say_what_is_happening_in_plain_words() {
    let note = |phase: Phase, on: bool| {
        Task {
            on,
            phase,
            since_ms: 0,
        }
        .note()
        .unwrap()
    };
    assert_eq!(
        note(
            Phase::Working {
                step: Step::Checking
            },
            true
        ),
        "Turning on Chrome…"
    );
    assert_eq!(
        note(
            Phase::Working {
                step: Step::Checking
            },
            false
        ),
        "Turning off Chrome…"
    );
    assert!(note(
        Phase::Working {
            step: Step::Restarting
        },
        true
    )
    .contains("restarting"));
    assert_eq!(
        note(Phase::Waiting { why: Wait::Turn }, true),
        "Waiting for the turn to end"
    );
    assert_eq!(
        note(
            Phase::Done {
                result: Done::AlreadyOn
            },
            true
        ),
        "Chrome is on for this session"
    );
    assert_eq!(note(Phase::Done { result: Done::On }, true), "Chrome is on");
    assert_eq!(
        note(Phase::Done { result: Done::Off }, false),
        "Chrome is off"
    );
}

#[test]
fn chrome_already_on_is_a_note_and_never_a_sheet() {
    let mut state = State::default();
    let done = status(
        json!({"phase": "done", "result": "already_on", "on": true, "since_ms": 1}),
        Some(true),
    );
    assert_eq!(state.apply("s1", &done, 1_000), Applied::Finished);
    assert_eq!(
        state.note("s1", 1_000).as_deref(),
        Some("Chrome is on for this session")
    );
    assert_eq!(state.note("s1", 1_000 + NOTE_MS + 1), None);
    assert_eq!(state.on.get("s1"), Some(&true));
    assert!(state.active().is_empty());
}

#[test]
fn chrome_off_turns_on_with_progress_and_no_sheet() {
    let mut state = State::default();
    state.begin("s1", true, 10);
    assert_eq!(state.note("s1", 11).as_deref(), Some("Turning on Chrome…"));
    assert_eq!(state.active(), vec!["s1".to_owned()]);
    let restarting = status(
        json!({"phase": "working", "step": "restarting", "on": true, "since_ms": 12}),
        Some(false),
    );
    assert_eq!(state.apply("s1", &restarting, 20), Applied::Working);
    assert!(state.note("s1", 21).unwrap().contains("restarting"));
    let done = status(
        json!({"phase": "done", "result": "on", "on": true, "since_ms": 30}),
        Some(true),
    );
    assert_eq!(state.apply("s1", &done, 40), Applied::Finished);
    assert_eq!(state.note("s1", 41).as_deref(), Some("Chrome is on"));
    assert!(state.active().is_empty());
    assert_eq!(state.on.get("s1"), Some(&true));
}

#[test]
fn a_turn_in_the_way_is_waited_out_like_a_queued_reply() {
    let mut state = State::default();
    state.begin("s1", true, 0);
    let waiting = status(
        json!({"phase": "waiting", "why": "turn", "on": true, "since_ms": 1}),
        Some(false),
    );
    assert_eq!(state.apply("s1", &waiting, 5), Applied::Working);
    assert_eq!(
        state.note("s1", 6).as_deref(),
        Some("Waiting for the turn to end")
    );
    assert!(state.due(POLL_MS + 1));
    assert!(!state.due(0));
}

#[test]
fn only_something_for_the_person_opens_the_sheet_with_its_fix() {
    let mut state = State::default();
    state.begin("s1", true, 0);
    let needs = status(
        json!({"phase": "needs_you", "need": {"need": "not_connected"}, "on": true, "since_ms": 9}),
        Some(false),
    );
    let Applied::Needs(sheet) = state.apply("s1", &needs, 10) else {
        panic!("the sheet opens");
    };
    assert_eq!(sheet.need, Some(Need::NotConnected));
    assert!(sheet.wanted_on);
    assert_eq!(sheet.acts(), vec![Act::TryAgain]);
    assert!(sheet.problem().unwrap().contains("isn't connected"));
    assert!(state.active().is_empty());
    assert_eq!(state.note("s1", 11), None);
}

#[test]
fn each_need_names_its_fix_and_its_buttons() {
    let sheet = |need: Option<Need>, on: Option<bool>| Sheet {
        session: "s1".into(),
        on,
        wanted_on: true,
        need,
    };
    let no_extension = sheet(Some(Need::NoExtension), Some(false));
    assert!(no_extension.problem().unwrap().contains("Web Store"));
    assert_eq!(no_extension.acts(), vec![Act::TryAgain]);
    let tasks = sheet(Some(Need::Tasks { count: 2 }), Some(false));
    assert!(tasks.problem().unwrap().contains("2 background tasks"));
    assert_eq!(tasks.acts(), vec![Act::TurnOnAnyway, Act::TryAgain]);
    let mcp = sheet(Some(Need::TurnedOffInMcp), Some(false));
    assert_eq!(mcp.acts(), vec![Act::OpenTerminal, Act::TryAgain]);
    let failed = sheet(
        Some(Need::Failed {
            text: "Claude Code started without Chrome.".into(),
        }),
        Some(false),
    );
    assert!(failed.problem().unwrap().contains("started without Chrome"));
    assert_eq!(failed.acts(), vec![Act::TryAgain, Act::OpenTerminal]);
    let busy = sheet(Some(Need::Busy { why: Wait::Box }), None);
    assert!(busy.problem().unwrap().contains("message box"));
    assert_eq!(sheet(None, Some(true)).acts(), vec![Act::TurnOff]);
    assert_eq!(sheet(None, Some(false)).acts(), vec![Act::TurnOn]);
    assert_eq!(sheet(None, Some(true)).status(), "On for this session");
    assert_eq!(sheet(None, Some(false)).status(), "Off for this session");
    assert_eq!(sheet(None, None).status(), "Not known yet");
}

#[test]
fn the_help_lines_say_install_sign_in_and_one_profile() {
    let sheet = Sheet {
        session: "s1".into(),
        on: Some(false),
        wanted_on: true,
        need: None,
    };
    let [install, account, profiles] = sheet.help();
    assert!(install.contains("Chrome Web Store"));
    assert!(account.contains("same Claude account"));
    assert!(profiles.contains("Several Chrome profiles"));
}
