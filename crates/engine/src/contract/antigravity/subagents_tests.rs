use std::sync::Arc;

use rusqlite::Connection;

use super::subagents::AntigravitySubagents;
use crate::contract::subagents::{Roots, SessionRef, Subagents};
use crate::contract::Tool;
use crate::sessions::testutil::TempDir;
use crate::tools::agy::elsewhere::log_path;
use crate::tools::agy::summaries::DB_NAME;
use crate::transcript::model::ItemKind;
use crate::transcript::subagents::{Origin, Status, Subagent};

const TOP: &str = "00000000-aaaa-4000-8000-000000000001";
const ONE: &str = "00000000-aaaa-4000-8000-000000000002";
const TWO: &str = "00000000-aaaa-4000-8000-000000000003";
const IDLE: &str = "CASCADE_RUN_STATUS_IDLE";
const RUNNING: &str = "CASCADE_RUN_STATUS_RUNNING";

fn user(at: &str) -> String {
    format!(
        r#"{{"step_index":0,"source":"USER_EXPLICIT","type":"USER_INPUT","status":"DONE","created_at":"{at}","content":"<USER_REQUEST>\nDo the part.\n</USER_REQUEST>"}}"#
    )
}

fn planner(index: u32, tokens: u32, calls: &str) -> String {
    format!(
        r#"{{"step_index":{index},"source":"MODEL","type":"PLANNER_RESPONSE","status":"DONE","created_at":"2026-11-14T09:00:0{index}Z","input_tokens":{tokens},"cache_read_tokens":0,"output_tokens":10,"tool_calls":[{calls}]}}"#
    )
}

fn log(dir: &TempDir, session: &str, lines: &[String]) {
    let path = log_path(&dir.path("agy"), session);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, lines.join("\n") + "\n").unwrap();
}

struct Seed<'a> {
    id: &'a str,
    parent: &'a str,
    name: &'a str,
    status: &'a str,
    at: &'a str,
    killed: i64,
}

fn summaries(dir: &TempDir, rows: &[Seed]) {
    std::fs::create_dir_all(dir.path("agy")).unwrap();
    let db = Connection::open(dir.path("agy").join(DB_NAME)).unwrap();
    db.execute_batch(
        "CREATE TABLE conversation_summaries (conversation_id TEXT, title TEXT, preview TEXT, \
         workspace_uris TEXT, status TEXT, last_modified_time TEXT, nesting_depth INTEGER, \
         parent_conversation_id TEXT, agent_name TEXT, killed INTEGER);",
    )
    .unwrap();
    for r in rows {
        let depth = i64::from(!r.parent.is_empty());
        db.execute(
            "INSERT INTO conversation_summaries VALUES (?1, '', '', '[]', ?2, ?3, ?4, ?5, ?6, ?7)",
            rusqlite::params![r.id, r.status, r.at, depth, r.parent, r.name, r.killed],
        )
        .unwrap();
    }
}

fn seed<'a>(id: &'a str, status: &'a str, at: &'a str) -> Seed<'a> {
    Seed {
        id,
        parent: TOP,
        name: "self",
        status,
        at,
        killed: 0,
    }
}

fn roots(dir: &TempDir) -> Roots {
    Roots {
        antigravity: dir.path("agy"),
        antigravity_copy: dir.path("copy"),
        ..Roots::default()
    }
}

fn parent() -> SessionRef {
    SessionRef {
        id: format!("antigravity:{TOP}"),
        tool: Tool::Antigravity,
        transcript: None,
        name: String::new(),
        finished: Arc::default(),
        session: format!("antigravity:{TOP}"),
    }
}

fn list(dir: &TempDir) -> Vec<Subagent> {
    AntigravitySubagents
        .list(&parent(), &roots(dir), 0.0)
        .expect("a list")
}

fn status_of(rows: &[Subagent], id: &str) -> Status {
    rows.iter().find(|r| r.own_id() == id).expect(id).status
}

fn working(dir: &TempDir, session: &str) {
    log(
        dir,
        session,
        &[
            user("2026-11-14T09:00:00Z"),
            planner(1, 700, r#"{"name":"view_file"}"#),
        ],
    );
}

#[test]
fn a_killed_child_is_stopped() {
    let dir = TempDir::new("agy-killed");
    let mut row = seed(ONE, IDLE, "2026-11-14 09:05:00+00:00");
    row.killed = 1;
    summaries(&dir, &[row]);
    working(&dir, ONE);
    assert_eq!(status_of(&list(&dir), ONE), Status::Stopped);
}

#[test]
fn idle_after_send_message_is_completed() {
    let dir = TempDir::new("agy-sent");
    summaries(&dir, &[seed(ONE, IDLE, "2026-11-14 09:05:00+00:00")]);
    log(
        &dir,
        ONE,
        &[
            user("2026-11-14T09:00:00Z"),
            planner(
                1,
                700,
                r#"{"name":"send_message","args":{"Message":"\"done\""}}"#,
            ),
        ],
    );
    assert_eq!(status_of(&list(&dir), ONE), Status::Completed);
}

#[test]
fn idle_after_the_parent_heard_from_it_is_completed() {
    let dir = TempDir::new("agy-heard");
    summaries(&dir, &[seed(ONE, IDLE, "2026-11-14 09:05:00+00:00")]);
    working(&dir, ONE);
    log(
        &dir,
        TOP,
        &[format!(
            r#"{{"step_index":4,"source":"SYSTEM","type":"SYSTEM_MESSAGE","status":"DONE","created_at":"2026-11-14T09:06:00Z","content":"[Message] sender={ONE} text=done"}}"#
        )],
    );
    assert_eq!(status_of(&list(&dir), ONE), Status::Completed);
}

#[test]
fn idle_with_neither_is_quiet() {
    let dir = TempDir::new("agy-quiet");
    summaries(&dir, &[seed(ONE, IDLE, "2026-11-14 09:05:00+00:00")]);
    working(&dir, ONE);
    assert_eq!(status_of(&list(&dir), ONE), Status::Quiet);
}

#[test]
fn an_error_step_is_failed() {
    let dir = TempDir::new("agy-failed");
    summaries(&dir, &[seed(ONE, IDLE, "2026-11-14 09:05:00+00:00")]);
    log(
        &dir,
        ONE,
        &[
            user("2026-11-14T09:00:00Z"),
            r#"{"step_index":1,"source":"SYSTEM","type":"ERROR_MESSAGE","status":"DONE","created_at":"2026-11-14T09:00:03Z","content":"quota"}"#.into(),
        ],
    );
    assert_eq!(status_of(&list(&dir), ONE), Status::Failed);
}

#[test]
fn two_children_with_their_statuses() {
    let dir = TempDir::new("agy-two");
    summaries(
        &dir,
        &[
            seed(TWO, RUNNING, "2026-11-14 09:07:00+00:00"),
            seed(ONE, IDLE, "2026-11-14 09:05:00+00:00"),
        ],
    );
    working(&dir, TWO);
    log(
        &dir,
        ONE,
        &[
            user("2026-11-14T09:00:00Z"),
            planner(1, 500, r#"{"name":"send_message"}"#),
        ],
    );
    let rows = list(&dir);
    assert_eq!(rows.len(), 2);
    assert_eq!(status_of(&rows, TWO), Status::Running);
    assert_eq!(status_of(&rows, ONE), Status::Completed);
    for row in &rows {
        assert_eq!(row.id, format!("antigravity:{}", row.own_id()));
        assert_eq!(row.origin, Origin::native(Tool::Antigravity));
        assert_eq!(row.depth, 1);
        assert!(row.path.ends_with("transcript.jsonl"), "{}", row.path);
    }
}

#[test]
fn a_child_without_a_log_yet_has_no_path() {
    let dir = TempDir::new("agy-nolog");
    summaries(&dir, &[seed(ONE, RUNNING, "2026-11-14 09:05:00+00:00")]);
    let rows = list(&dir);
    assert_eq!(rows[0].path, "");
    assert!(AntigravitySubagents.glance(&rows[0]).is_err());
    assert!(AntigravitySubagents.open(&rows[0]).is_err());
}

#[test]
fn the_role_comes_from_the_parents_invoke_subagent_call() {
    let dir = TempDir::new("agy-role");
    let mut named = seed(TWO, IDLE, "2026-11-14 09:07:00+00:00");
    named.name = "research";
    summaries(&dir, &[seed(ONE, IDLE, "2026-11-14 09:05:00+00:00"), named]);
    log(
        &dir,
        TOP,
        &[
            user("2026-11-14T09:00:00Z"),
            planner(
                1,
                100,
                r#"{"name":"invoke_subagent","args":{"role":"\"Reviewer\""}},{"name":"invoke_subagent","args":{"role":"\"Finder\""}}"#,
            ),
        ],
    );
    working(&dir, ONE);
    working(&dir, TWO);
    let rows = list(&dir);
    let by = |id: &str| rows.iter().find(|r| r.own_id() == id).unwrap();
    assert_eq!(by(ONE).description, "Reviewer");
    assert_eq!(by(TWO).description, "Finder");
    assert_eq!(by(TWO).agent_type, "research");
}

#[test]
fn without_a_parent_log_the_description_is_the_agent_name() {
    let dir = TempDir::new("agy-name");
    let mut named = seed(ONE, IDLE, "2026-11-14 09:05:00+00:00");
    named.name = "image-generator";
    summaries(&dir, &[named, seed(TWO, IDLE, "2026-11-14 09:06:00+00:00")]);
    let rows = list(&dir);
    let by = |id: &str| rows.iter().find(|r| r.own_id() == id).unwrap();
    assert_eq!(by(ONE).description, "image-generator");
    assert_eq!(by(TWO).description, "Antigravity subagent");
}

#[test]
fn a_session_of_another_tool_or_with_no_database_has_none() {
    let dir = TempDir::new("agy-other");
    assert!(list(&dir).is_empty());
    summaries(&dir, &[seed(ONE, IDLE, "2026-11-14 09:05:00+00:00")]);
    let mut codex = parent();
    codex.tool = Tool::Codex;
    assert!(AntigravitySubagents
        .list(&codex, &roots(&dir), 0.0)
        .unwrap()
        .is_empty());
}

#[test]
fn glance_and_open_read_the_childs_log() {
    let dir = TempDir::new("agy-read");
    summaries(&dir, &[seed(ONE, RUNNING, "2026-11-14 09:05:00+00:00")]);
    working(&dir, ONE);
    let row = list(&dir).remove(0);
    let glance = AntigravitySubagents.glance(&row).expect("a glance");
    assert_eq!(glance.tokens, Some(710));
    assert!(glance.recent.is_some());
    let conversation = AntigravitySubagents.open(&row).expect("opened");
    assert!(matches!(
        conversation.items.first().map(|i| &i.kind),
        Some(ItemKind::User { .. })
    ));
}
