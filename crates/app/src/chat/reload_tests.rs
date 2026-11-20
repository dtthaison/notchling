use notchling_engine::catalog::{Catalog, ModelInfo, Tool as CatalogTool};
use notchling_engine::reload::{Need, Phase, Wait};
use notchling_engine::sessions::{Agent, AgentGroup, AgentTarget, NodeRef};
use serde_json::{json, Value};

use super::data::{Session, Tool};
use super::reload::{
    lead, mode, parse, Act, Applied, Card, Mode, Stage, State, Status, Task, NOTE_MS, POLL_MS,
};
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

fn catalog() -> Vec<ModelInfo> {
    Catalog::built_in().models(CatalogTool::Claude, &[])
}

fn open_card() -> Card {
    Card::open(&agent(AgentTarget::Job("j1".into())), "Lead.".into()).unwrap()
}

fn status(task: Value) -> Status {
    parse(&json!({"offered": true, "kind": "job", "task": task})).unwrap()
}

#[test]
fn the_item_is_for_claude_jobs_and_terminals_only() {
    let job = || agent(AgentTarget::Job("j1".into()));
    assert_eq!(mode(&session(Tool::Claude, job())), Some(Mode::Reload));
    assert_eq!(
        mode(&session(Tool::Claude, agent(AgentTarget::Process(42)))),
        Some(Mode::Terminal)
    );
    assert_eq!(mode(&session(Tool::Codex, job())), None);
    assert_eq!(mode(&session(Tool::Antigravity, job())), None);
    let mut headless = job();
    headless.headless = true;
    assert_eq!(mode(&session(Tool::Claude, headless)), None);
    let mut shell = job();
    shell.command = Some("npm run dev".into());
    assert_eq!(mode(&session(Tool::Claude, shell)), None);
    let mut remote = job();
    remote.node = Some(NodeRef {
        id: "n1".into(),
        name: "worker".into(),
        online: true,
    });
    assert_eq!(mode(&session(Tool::Claude, remote)), None);
    let closed = agent(AgentTarget::Resume {
        session_id: "a".into(),
        cwd: "/x".into(),
    });
    assert_eq!(mode(&session(Tool::Claude, closed)), None);
}

#[test]
fn the_card_says_tokens_and_dollars_on_the_model_from_the_catalog_price() {
    let text = lead(180_000, Some("claude-opus-5-5"), &catalog());
    assert!(text.contains("180k tokens"), "{text}");
    assert!(text.contains("about $0.72"), "{text}");
    assert!(text.contains("Opus"), "{text}");
    assert!(text.starts_with("Reloads the session to pick up new tools."), "{text}");
}

#[test]
fn with_no_price_it_gives_tokens_only() {
    let text = lead(180_000, Some("claude-made-up-9"), &catalog());
    assert!(text.contains("180k tokens"), "{text}");
    assert!(!text.contains('$'), "{text}");
    let unknown = lead(180_000, None, &catalog());
    assert!(!unknown.contains('$'), "{unknown}");
    assert!(unknown.contains("180k tokens"));
}

#[test]
fn with_nothing_read_yet_it_says_only_that_the_conversation_is_re_read() {
    let text = lead(0, None, &catalog());
    assert!(text.contains("The conversation is re-read."), "{text}");
    assert!(!text.contains("tokens"));
}

#[test]
fn a_terminal_session_gets_instructions_and_no_reload_button() {
    let card = Card::open(&agent(AgentTarget::Process(42)), "Lead.".into()).unwrap();
    assert_eq!(card.stage, Stage::Terminal);
    assert_eq!(card.command, "claude --resume s1");
    assert!(card.intro().contains("terminal Notchling didn't start"));
    assert_eq!(card.acts(), [Act::CopyCommand, Act::Close]);
    assert!(!card.acts().contains(&Act::Reload));
}

#[test]
fn the_card_asks_first_with_reload_and_cancel() {
    let card = open_card();
    assert_eq!(card.stage, Stage::Ask);
    assert_eq!(card.acts(), [Act::Reload, Act::Cancel]);
    assert_eq!(card.intro(), "Lead.");
    assert_eq!(card.status(), None);
    assert_eq!(Act::Reload.label(), "Reload");
}

#[test]
fn helpers_are_counted_in_the_waiting_line() {
    let mut card = open_card();
    card.stage = Stage::Waiting(Wait::Tasks { count: 3 });
    assert_eq!(
        card.status().unwrap(),
        "3 helpers running: reload when they finish?"
    );
    assert_eq!(card.acts(), [Act::CancelReload]);
    card.stage = Stage::Waiting(Wait::Tasks { count: 1 });
    assert_eq!(
        card.status().unwrap(),
        "1 helper running: reload when it finishes?"
    );
    card.stage = Stage::Waiting(Wait::Turn);
    assert_eq!(card.status().unwrap(), "Waiting for the turn to end…");
    card.stage = Stage::Waiting(Wait::Dialog);
    assert!(card.status().unwrap().contains("open question"));
}

#[test]
fn the_daemons_state_is_read_with_its_phase_and_new_id() {
    let parsed = status(json!({
        "phase": "waiting", "by": {"why": "tasks", "count": 2},
        "after_turn": true, "since_ms": 9
    }));
    assert_eq!(
        parsed.task,
        Some(Task {
            phase: Phase::Waiting {
                by: Wait::Tasks { count: 2 }
            },
            since_ms: 9,
            session: None
        })
    );
    let done = status(json!({"phase": "done", "result": "reloaded", "session": "n2"}));
    assert_eq!(done.task.unwrap().session.as_deref(), Some("n2"));
    let needs = status(json!({
        "phase": "needs_you", "need": {"need": "busy", "by": {"why": "turn"}}
    }));
    assert!(matches!(needs.task.unwrap().phase, Phase::NeedsYou { .. }));
    assert_eq!(parse(&json!({"nope": 1})), None);
    assert_eq!(
        parse(&json!({"offered": false, "kind": "terminal", "task": null}))
            .unwrap()
            .task,
        None
    );
}

#[test]
fn the_requests_are_the_reload_op_and_the_quiet_state_read() {
    let go = Request::reload("s1", true);
    assert_eq!(go.op, ops::SESSION_RELOAD);
    assert_eq!(go.args, json!({"id": "s1", "afterTurn": true}));
    assert_eq!(
        Request::reload_cancel("s1").args,
        json!({"id": "s1", "cancel": true})
    );
    let read = Request::reload_state("s1", false);
    assert_eq!(read.op, ops::SESSION_RELOAD_STATE);
    assert_eq!(read.args, json!({"id": "s1"}));
    assert_eq!(Request::reload_state("s1", true).args["ack"], true);
}

#[test]
fn polling_moves_the_card_along_and_ends_with_a_note() {
    let mut state = State {
        card: Some(open_card()),
        ..State::default()
    };
    let waiting = status(json!({"phase": "waiting", "by": {"why": "turn"}}));
    assert_eq!(state.apply("s1", &waiting, 0), Applied::Changed);
    assert_eq!(state.apply("s1", &waiting, 0), Applied::Nothing);
    assert!(state.card.as_ref().unwrap().live());
    let working = status(json!({"phase": "working", "step": "restarting"}));
    assert_eq!(state.apply("s1", &working, 0), Applied::Changed);
    assert_eq!(
        state.note("s1", 0).unwrap(),
        "Reloading… the conversation is kept"
    );
    let done = status(json!({"phase": "done", "result": "reloaded"}));
    assert_eq!(
        state.apply("s1", &done, 1_000),
        Applied::Finished { followed: None }
    );
    assert!(state.card.is_none());
    assert_eq!(state.note("s1", 1_001).unwrap(), "Session reloaded");
    assert_eq!(state.note("s1", 1_000 + NOTE_MS + 1), None);
}

#[test]
fn a_session_that_took_a_new_id_is_said_to_continue() {
    let mut state = State {
        card: Some(open_card()),
        ..State::default()
    };
    let done = status(json!({"phase": "done", "result": "reloaded", "session": "n2"}));
    assert_eq!(
        state.apply("s1", &done, 0),
        Applied::Finished {
            followed: Some("n2".into())
        }
    );
    assert!(state.note("s1", 1).unwrap().contains("new session id"));
}

#[test]
fn something_for_the_person_becomes_a_problem_with_try_again() {
    let mut state = State {
        card: Some(open_card()),
        ..State::default()
    };
    let needs = status(json!({
        "phase": "needs_you", "need": {"need": "failed", "text": "no such job"}
    }));
    assert_eq!(state.apply("s1", &needs, 0), Applied::Changed);
    let card = state.card.as_ref().unwrap();
    assert!(card.problem());
    assert_eq!(card.status().unwrap(), "no such job");
    assert_eq!(card.acts(), [Act::TryAgain, Act::Close]);
    assert_eq!(
        Card {
            stage: Stage::Needs(Need::Busy { by: Wait::Turn }),
            ..open_card()
        }
        .status()
        .unwrap(),
        "The turn didn't end in time. Try again when it has."
    );
}

#[test]
fn a_cancelled_reload_closes_the_card_without_a_note() {
    let mut state = State {
        card: Some(open_card()),
        ..State::default()
    };
    let cancelled = status(json!({"phase": "cancelled"}));
    assert_eq!(
        state.apply("s1", &cancelled, 0),
        Applied::Finished { followed: None }
    );
    assert!(state.card.is_none());
    assert_eq!(state.note("s1", 1), None);
}

#[test]
fn polls_run_only_while_the_card_is_live_and_a_failing_poll_gives_up() {
    let mut state = State::default();
    assert!(!state.due(10_000));
    let mut card = open_card();
    state.card = Some(card.clone());
    assert!(!state.due(10_000));
    card.stage = Stage::Waiting(Wait::Turn);
    state.card = Some(card);
    assert!(state.due(10_000));
    state.last_poll_ms = 10_000;
    assert!(!state.due(10_000 + POLL_MS - 1));
    assert!(state.due(10_000 + POLL_MS));
    for _ in 0..4 {
        assert!(!state.poll_failed());
    }
    assert!(state.poll_failed());
}
