use notchling_engine::catalog::{Catalog, ModelInfo, Tool as CatalogTool};
use notchling_engine::reload::{Need, Phase, Wait};
use notchling_engine::sessions::{Agent, AgentGroup, AgentTarget, NodeRef};
use serde_json::{json, Value};

use super::data::{Session, Tool};
use super::reload_feed::ReloadFeed;
use super::session_reload::{
    lead, mode, parse_task, Act, Card, Intent, Mode, Settle, Stage, State, Task, ACK_RETRY_MS,
    NOTE_MS,
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
    Card::open(&agent(AgentTarget::Job("j1".into())), "Lead.".into(), 0).unwrap()
}

fn card_for(id: &str, floor: u64) -> Card {
    let mut a = agent(AgentTarget::Job("j1".into()));
    a.id = id.into();
    Card::open(&a, "Lead.".into(), floor).unwrap()
}

fn push(feed: &mut ReloadFeed, id: &str, rev: u64, task: Value) {
    assert!(feed.apply(&json!({"id": id, "rev": rev, "task": task})));
}

fn waiting() -> Value {
    json!({"phase": "waiting", "by": {"why": "turn"}, "after_turn": true, "since_ms": 1})
}

fn working() -> Value {
    json!({"phase": "working", "step": "restarting"})
}

fn done() -> Value {
    json!({"phase": "done", "result": "reloaded"})
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
    assert!(
        text.starts_with("Reloads the session to pick up new tools."),
        "{text}"
    );
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
    let card = Card::open(&agent(AgentTarget::Process(42)), "Lead.".into(), 0).unwrap();
    assert_eq!(card.stage, Stage::Terminal);
    assert_eq!(card.command, "claude --resume 's1'");
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
fn the_terminal_command_quotes_the_id_and_prefers_the_resume_id() {
    let mut a = agent(AgentTarget::Process(7));
    a.id = "it's".into();
    assert_eq!(
        Card::open(&a, String::new(), 0).unwrap().command,
        "claude --resume 'it'\\''s'"
    );
    a.resume_session_id = Some("old-conv".into());
    assert_eq!(
        Card::open(&a, String::new(), 0).unwrap().command,
        "claude --resume 'old-conv'"
    );
}

#[test]
fn the_daemons_task_is_read_with_its_phase_new_id_and_a_cancel_asked() {
    let task = parse_task(&json!({
        "phase": "waiting", "by": {"why": "tasks", "count": 2},
        "after_turn": true, "since_ms": 9, "cancelling": true
    }));
    assert_eq!(
        task,
        Some(Task {
            phase: Phase::Waiting {
                by: Wait::Tasks { count: 2 }
            },
            since_ms: 9,
            session: None,
            cancelling: true,
        })
    );
    let moved = parse_task(&json!({"phase": "done", "result": "reloaded", "session": "n2"}));
    assert_eq!(moved.unwrap().session.as_deref(), Some("n2"));
    assert_eq!(parse_task(&Value::Null), None);
    assert_eq!(parse_task(&json!({"nope": 1})), None);
}

#[test]
fn the_requests_are_the_reload_op_its_cancel_and_an_ack_of_one_rev() {
    let go = Request::reload("s1", true);
    assert_eq!(go.op, ops::SESSION_RELOAD);
    assert_eq!(go.args, json!({"id": "s1", "afterTurn": true}));
    assert_eq!(
        Request::reload_cancel("s1").args,
        json!({"id": "s1", "cancel": true})
    );
    let ack = Request::reload_ack("s1", 7);
    assert_eq!(ack.op, ops::SESSION_RELOAD_STATE);
    assert_eq!(ack.args, json!({"id": "s1", "ack": true, "rev": 7}));
}

#[test]
fn the_feed_keeps_the_newest_rev_and_drops_a_late_older_push() {
    let mut feed = ReloadFeed::seed(&json!({"tasks": [
        {"id": "s1", "rev": 5, "task": waiting()},
        {"id": "s2", "rev": 3, "task": null}
    ]}));
    assert!(feed.live("s1"));
    assert_eq!(feed.rev("s2"), 3);
    assert!(!feed.apply(&json!({"id": "s1", "rev": 4, "task": done()})));
    assert!(feed.live("s1"));
    assert!(feed.absorb("s1", &json!({"offered": true, "rev": 6, "task": working()})));
    assert_eq!(feed.rev("s1"), 6);
    assert!(!feed.absorb("s1", &json!({"offered": true, "rev": 6, "task": done()})));
    push(&mut feed, "s1", 8, done());
    assert!(!feed.live("s1"));
    assert_eq!(
        feed.ended()
            .map(|(id, rev, _)| (id, rev))
            .collect::<Vec<_>>(),
        [("s1", 8)]
    );
}

#[test]
fn the_card_follows_the_daemon_and_done_closes_it_with_a_note_and_one_ack() {
    let mut state = State::default();
    let mut card = state.open(card_for("s1", 0));
    card.begin(0);
    state.card = Some(card);
    let mut feed = ReloadFeed::default();
    assert_eq!(state.sync(&feed, 0).settle, Some(Settle::Changed));
    assert_eq!(state.card.as_ref().unwrap().stage, Stage::Starting);
    push(&mut feed, "s1", 5, waiting());
    state.sync(&feed, 0);
    assert_eq!(
        state.card.as_ref().unwrap().stage,
        Stage::Waiting(Wait::Turn)
    );
    assert_eq!(state.row_line("s1"), Some("Reloading…"));
    push(&mut feed, "s1", 6, working());
    state.sync(&feed, 0);
    assert_eq!(state.card.as_ref().unwrap().stage, Stage::Working);
    assert!(state.card.as_ref().unwrap().acts().is_empty());
    push(&mut feed, "s1", 7, done());
    let synced = state.sync(&feed, 100);
    assert_eq!(synced.settle, Some(Settle::Close));
    assert_eq!(synced.acks, [("s1".to_string(), 7)]);
    assert!(state.card.is_none());
    assert_eq!(state.note("s1", 100).as_deref(), Some("Session reloaded"));
    assert_eq!(state.note("s1", 100 + NOTE_MS), None);
    assert_eq!(state.row_line("s1"), None);
}

#[test]
fn nothing_is_asked_of_the_daemon_once_every_task_is_final() {
    let mut state = State::default();
    let mut feed = ReloadFeed::default();
    push(&mut feed, "s1", 4, done());
    push(&mut feed, "s2", 5, json!({"phase": "cancelled"}));
    assert_eq!(state.sync(&feed, 0).acks.len(), 2);
    assert_eq!(state.sync(&feed, 0), Default::default());
    push(&mut feed, "s1", 9, Value::Null);
    push(&mut feed, "s2", 10, Value::Null);
    assert_eq!(state.sync(&feed, 0), Default::default());
    assert!(!state.live("s1") && !state.live("s2"));
}

#[test]
fn cancel_closes_nothing_until_the_daemon_says_cancelled() {
    let mut state = State::default();
    let mut feed = ReloadFeed::default();
    push(&mut feed, "s1", 5, waiting());
    state.sync(&feed, 0);
    let card = state.open(card_for("s1", feed.rev("s1")));
    assert_eq!(card.stage, Stage::Waiting(Wait::Turn));
    assert_eq!(card.acts(), [Act::CancelReload]);
    let card = state.card.as_mut().unwrap();
    card.intent = Intent::Cancelling;
    assert_eq!(card.settle(5, feed.task("s1")), Settle::Changed);
    assert_eq!(card.stage, Stage::Cancelling);
    card.intent = Intent::Following;
    let mut asked = waiting();
    asked["cancelling"] = json!(true);
    push(&mut feed, "s1", 6, asked);
    assert_eq!(state.sync(&feed, 0).settle, Some(Settle::Same));
    assert_eq!(state.card.as_ref().unwrap().stage, Stage::Cancelling);
    assert!(state.card.as_ref().unwrap().acts().is_empty());
    push(&mut feed, "s1", 7, json!({"phase": "cancelled"}));
    let synced = state.sync(&feed, 0);
    assert_eq!(synced.settle, Some(Settle::Close));
    assert_eq!(state.note("s1", 0), None);
}

#[test]
fn a_refused_cancel_keeps_the_card_on_the_daemons_state_and_says_why() {
    let mut state = State::default();
    let mut feed = ReloadFeed::default();
    push(&mut feed, "s1", 5, working());
    state.sync(&feed, 0);
    state.open(card_for("s1", 5));
    let card = state.card.as_mut().unwrap();
    card.intent = Intent::Refused("Already restarting.".into());
    card.settle(5, feed.task("s1"));
    assert_eq!(card.stage, Stage::Working);
    assert_eq!(card.refusal(), Some("Already restarting."));
    assert_eq!(card.note(), None);
    push(&mut feed, "s1", 6, done());
    assert_eq!(state.sync(&feed, 0).settle, Some(Settle::Close));
}

#[test]
fn a_sheet_belongs_to_one_session_and_another_sessions_end_leaves_it() {
    let mut state = State::default();
    let mut feed = ReloadFeed::default();
    push(&mut feed, "s1", 3, done());
    state.sync(&feed, 0);
    let card = state.open(card_for("s1", feed.rev("s1")));
    assert_eq!(card.stage, Stage::Ask);
    push(&mut feed, "s2", 4, waiting());
    push(&mut feed, "s2", 5, done());
    let synced = state.sync(&feed, 0);
    assert_eq!(synced.settle, Some(Settle::Same));
    assert_eq!(synced.acks, [("s2".to_string(), 5)]);
    assert_eq!(state.card.as_ref().unwrap().session, "s1");
    assert_eq!(state.card.as_ref().unwrap().stage, Stage::Ask);
    state.open(card_for("s2", feed.rev("s2")));
    assert_eq!(state.card.as_ref().unwrap().session, "s2");
    assert_eq!(state.card.as_ref().unwrap().stage, Stage::Ask);
}

#[test]
fn a_reload_already_running_shows_live_when_the_sheet_opens() {
    let mut state = State::default();
    let mut feed = ReloadFeed::default();
    push(&mut feed, "s1", 5, working());
    state.sync(&feed, 0);
    let card = state.open(card_for("s1", 5));
    assert_eq!(card.stage, Stage::Working);
}

#[test]
fn a_problem_is_a_note_with_no_sheet_and_stays_on_the_sheet_after_the_ack() {
    let needs = json!({"phase": "needs_you", "need": {"need": "tasks_unknown"}});
    let mut state = State::default();
    let mut feed = ReloadFeed::default();
    push(&mut feed, "s2", 5, needs.clone());
    state.sync(&feed, 0);
    assert!(state.note("s2", 0).unwrap().contains("background work"));
    let mut card = state.open(card_for("s1", 0));
    card.begin(0);
    state.card = Some(card);
    push(&mut feed, "s1", 6, needs);
    let synced = state.sync(&feed, 0);
    assert_eq!(synced.acks, [("s1".to_string(), 6)]);
    assert_eq!(state.note("s1", 0), None);
    let card = state.card.as_ref().unwrap();
    assert!(card.problem());
    assert_eq!(card.acts(), [Act::TryAgain, Act::Close]);
    push(&mut feed, "s1", 7, Value::Null);
    state.sync(&feed, 0);
    assert!(state.card.as_ref().unwrap().problem());
    let mut card = state.card.clone().unwrap();
    card.begin(feed.rev("s1"));
    card.settle(feed.rev("s1"), feed.task("s1"));
    assert_eq!(card.stage, Stage::Starting);
}

#[test]
fn a_start_the_daemon_refuses_shows_why_with_try_again() {
    let mut card = card_for("s1", 0);
    card.begin(0);
    card.intent = Intent::Ended(Need::Failed {
        text: "Act on Sessions is off.".into(),
    });
    card.settle(0, None);
    assert_eq!(card.status().as_deref(), Some("Act on Sessions is off."));
    assert_eq!(card.acts(), [Act::TryAgain, Act::Close]);
}

#[test]
fn a_session_that_took_a_new_id_is_followed_and_noted_under_it() {
    let mut state = State::default();
    let mut feed = ReloadFeed::default();
    push(
        &mut feed,
        "s1",
        5,
        json!({"phase": "done", "result": "reloaded", "session": "n2"}),
    );
    let synced = state.sync(&feed, 0);
    assert_eq!(synced.follows, [("s1".to_string(), "n2".to_string())]);
    assert!(state.note("n2", 0).unwrap().contains("new session id"));
}

#[test]
fn an_agents_reload_shows_on_its_row_from_the_push_alone() {
    let mut state = State::default();
    let mut feed = ReloadFeed::default();
    push(&mut feed, "s3", 5, waiting());
    state.sync(&feed, 0);
    assert_eq!(state.row_line("s3"), Some("Reloading…"));
    assert_eq!(state.note("s3", 0).as_deref(), Some("Reloading…"));
    push(&mut feed, "s3", 6, done());
    state.sync(&feed, 0);
    assert_eq!(state.row_line("s3"), None);
}

#[test]
fn a_failed_ack_is_tried_again_after_a_pause_and_a_sent_one_never_twice() {
    let mut state = State::default();
    let mut feed = ReloadFeed::default();
    push(&mut feed, "s1", 5, done());
    assert_eq!(state.sync(&feed, 0).acks, [("s1".to_string(), 5)]);
    assert!(state.due_acks(10).is_empty());
    state.ack_failed("s1", 5, 10);
    assert!(state.due_acks(10 + ACK_RETRY_MS - 1).is_empty());
    assert_eq!(state.due_acks(10 + ACK_RETRY_MS), [("s1".to_string(), 5)]);
    assert!(state.due_acks(10 + 3 * ACK_RETRY_MS).is_empty());
    state.ack_failed("s1", 4, 0);
    assert!(state.due_acks(i64::MAX - 1).is_empty());
    assert_eq!(state.note("s1", 20).as_deref(), Some("Session reloaded"));
}

#[test]
fn a_new_daemon_run_starts_the_handled_revs_and_the_card_floor_over() {
    let mut state = State::default();
    let feed = ReloadFeed::seed(&json!({"boot": "run-1", "tasks": [
        {"id": "s1", "rev": 900, "task": done()}
    ]}));
    assert_eq!(state.sync(&feed, 0).acks.len(), 1);
    state.open(card_for("s1", 900));
    assert_eq!(state.card.as_ref().unwrap().floor, 900);
    let fresh = ReloadFeed::seed(&json!({"boot": "run-2", "tasks": [
        {"id": "s1", "rev": 40, "task": waiting()}
    ]}));
    state.sync(&fresh, 0);
    assert_eq!(state.card.as_ref().unwrap().floor, 0);
    assert_eq!(
        state.card.as_ref().unwrap().stage,
        Stage::Waiting(Wait::Turn)
    );
    let mut fresh = fresh;
    push(&mut fresh, "s1", 41, done());
    let synced = state.sync(&fresh, 0);
    assert_eq!(synced.acks, [("s1".to_string(), 41)]);
    assert_eq!(synced.settle, Some(Settle::Close));
}
