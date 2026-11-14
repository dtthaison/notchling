use crate::acting_ops_tests::{setup, FakeRoom};
use crate::bridge_backend::{Approvals, DaemonSources};
use crate::daemon_wiring::Ops;
use crate::every_session::{EverySession, CACHE_MS};
use crate::inbox_ops::InboxOps;
use crate::layout::Layout;
use crate::limits_ops_tests::TempDir;
use notchling_engine::bridge::backend::BridgeBackend;
use notchling_engine::bridge::live::{ActingOff, LiveBackend};
use notchling_engine::sessions::{Agent, AgentGroup, AgentTarget};
use notchling_engine::tools::agy::summaries::DB_NAME;
use notchling_local_api::{ops, Caller, ClientKind, ErrorCode, Handler, Tier};
use rusqlite::Connection;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub(crate) const CODEX_ID: &str = "019aaaaa-0000-7000-8000-000000000001";
pub(crate) const CODEX_CHILD_ID: &str = "019aaaaa-0000-7000-8000-000000000002";
pub(crate) const AGY_ID: &str = "c0ffee00-1111-4222-8333-444455556666";

pub(crate) struct Fixture {
    pub dir: TempDir,
    pub layout: Layout,
}

fn civil(days: i64) -> (i64, i64, i64) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    (y, m, d)
}

fn summaries_date(secs: i64) -> String {
    let (days, rem) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    let (y, m, d) = civil(days);
    format!(
        "{y:04}-{m:02}-{d:02} {:02}:{:02}:{:02}.000000+00:00",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}

pub(crate) fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

fn rollout(thread: &str, parent: Option<&str>, title: &str) -> String {
    let mut meta = json!({"id": thread, "cwd": "/work/codex-app"});
    if let Some(parent) = parent {
        meta["parent_thread_id"] = json!(parent);
        meta["thread_source"] = json!("subagent");
    }
    let lines = [
        json!({"type": "session_meta", "payload": meta}),
        json!({"type": "response_item", "payload": {
            "type": "message", "role": "user",
            "content": [{"type": "input_text", "text": title}]}}),
    ];
    lines
        .iter()
        .map(|l| l.to_string())
        .collect::<Vec<_>>()
        .join("\n")
        + "\n"
}

impl Fixture {
    pub(crate) fn new(tag: &str) -> Fixture {
        let dir = TempDir::new(tag);
        let layout = Layout::resolve(None, Some(dir.root().as_os_str()), || None).unwrap();
        Fixture { dir, layout }
    }

    pub(crate) fn codex(&self, thread: &str, parent: Option<&str>, title: &str) -> PathBuf {
        let name = format!("rollout-2026-11-14T10-00-00-{thread}.jsonl");
        let path = self
            .layout
            .codex_dir()
            .join("sessions/2026/11/14")
            .join(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, rollout(thread, parent, title)).unwrap();
        path
    }

    pub(crate) fn antigravity(&self, id: &str, title: &str, status: &str, age_secs: i64) {
        let root = self.layout.antigravity_dir();
        std::fs::create_dir_all(&root).unwrap();
        let db = Connection::open(root.join(DB_NAME)).unwrap();
        db.execute_batch(
            "CREATE TABLE IF NOT EXISTS conversation_summaries (conversation_id TEXT, \
             title TEXT, preview TEXT, workspace_uris TEXT, status TEXT, \
             last_modified_time TEXT, nesting_depth INTEGER);",
        )
        .unwrap();
        let uris = json!(["file:///work/agy-app"]).to_string();
        let when = summaries_date(now_ms() / 1000 - age_secs);
        db.execute(
            "INSERT INTO conversation_summaries VALUES (?1, ?2, '', ?3, ?4, ?5, 0)",
            rusqlite::params![id, title, uris, status, when],
        )
        .unwrap();
    }

    pub(crate) fn every_tool(&self) {
        self.codex(CODEX_ID, None, "Fix the codex bug");
        self.antigravity(AGY_ID, "Edit the agy file", "IDLE", 60);
    }
}

pub(crate) fn claude_agent() -> Agent {
    Agent {
        id: "job-1".into(),
        name: "Claude Code job".into(),
        project: "claude-app".into(),
        cwd: "/work/claude-app".into(),
        group: AgentGroup::Working,
        label: "Working".into(),
        detail: None,
        since_ms: now_ms(),
        target: AgentTarget::Job("job0001".into()),
        suggested_reply: None,
        transcript_path: None,
        host: "Background".into(),
        headless: false,
        config_dir: None,
        command: None,
        resume_session_id: None,
    }
}

fn every(fixture: &Fixture) -> Arc<EverySession> {
    Arc::new(EverySession::new(&fixture.layout))
}

fn cli() -> Caller {
    Caller {
        tier: Tier::Agent,
        client: ClientKind::Cli,
        peer: None,
        pid: None,
    }
}

pub(crate) fn claude_list() -> Arc<Mutex<Value>> {
    Arc::new(Mutex::new(json!([claude_agent()])))
}

fn sources(dir: &Path, list: Arc<Mutex<Value>>, every: Arc<EverySession>) -> DaemonSources {
    let room = Arc::new(FakeRoom::default());
    DaemonSources {
        sessions: list,
        acting: Arc::new(setup(false, &[], json!([])).ops),
        approvals: Arc::new(Approvals::new(room.clone(), Duration::from_secs(1))),
        inbox: InboxOps::new(dir.join("inbox"), room),
        every,
        projects_dir: dir.join("projects"),
    }
}

#[test]
fn foreign_rows_reach_the_bridge_backend() {
    let fixture = Fixture::new("every-bridge");
    fixture.every_tool();
    let backend = LiveBackend::new(
        sources(fixture.dir.root(), claude_list(), every(&fixture)),
        ActingOff,
    );
    let listed: Vec<(String, String)> = backend
        .sessions()
        .into_iter()
        .map(|s| (s.id, s.tool))
        .collect();
    assert!(
        listed.contains(&("job-1".into(), "claude".into())),
        "{listed:?}"
    );
    assert!(
        listed.contains(&(format!("codex:{CODEX_ID}"), "codex".into())),
        "{listed:?}"
    );
    assert!(
        listed.contains(&(format!("agy:{AGY_ID}"), "agy".into())),
        "{listed:?}"
    );
}

fn ops_over(fixture: &Fixture) -> Ops {
    Ops::new(claude_list()).with_every(every(fixture))
}

fn ids(rows: &Value) -> Vec<(String, String)> {
    rows.as_array()
        .unwrap()
        .iter()
        .map(|r| {
            (
                r["id"].as_str().unwrap().to_string(),
                r["tool"].as_str().unwrap_or("none").to_string(),
            )
        })
        .collect()
}

#[test]
fn no_argument_lists_claude_code_only() {
    let fixture = Fixture::new("every-default");
    fixture.every_tool();
    let ops = ops_over(&fixture);
    let cached = json!([claude_agent()]);
    for args in [
        Value::Null,
        json!({}),
        json!({"tools": null}),
        json!({"tools": "claude"}),
    ] {
        let got = ops.call(&cli(), ops::SESSIONS_LIST, &args).unwrap();
        assert_eq!(got.as_array().unwrap().len(), 1, "{args}");
        assert_eq!(got[0]["id"], cached[0]["id"], "{args}");
        assert!(got[0].get("tool").is_none(), "{args}: {got}");
    }
}

#[test]
fn all_lists_every_tool_with_the_bridge_off() {
    let fixture = Fixture::new("every-all");
    fixture.every_tool();
    let ops = ops_over(&fixture);
    let (got, extras) = ops.call_with_extras(&cli(), ops::SESSIONS_LIST, &json!({"tools": "all"}));
    assert!(extras.is_empty(), "{extras:?}");
    let got = got.unwrap();
    let listed = ids(&got);
    assert!(
        listed.contains(&("job-1".into(), "claude".into())),
        "{listed:?}"
    );
    assert!(
        listed.contains(&(format!("codex:{CODEX_ID}"), "codex".into())),
        "{listed:?}"
    );
    assert!(
        listed.contains(&(format!("agy:{AGY_ID}"), "antigravity".into())),
        "{listed:?}"
    );
    let agents: Vec<Agent> = serde_json::from_value(got).unwrap();
    let codex = agents
        .iter()
        .find(|a| a.id == format!("codex:{CODEX_ID}"))
        .unwrap();
    assert_eq!(codex.name, "Fix the codex bug");
    assert_eq!(codex.cwd, "/work/codex-app");
    assert!(codex.transcript_path.is_some());
    let agy = agents
        .iter()
        .find(|a| a.id == format!("agy:{AGY_ID}"))
        .unwrap();
    assert_eq!(agy.name, "Edit the agy file");
    assert_eq!(agy.cwd, "/work/agy-app");
}

#[test]
fn a_codex_child_is_not_listed() {
    let fixture = Fixture::new("every-child");
    fixture.codex(CODEX_ID, None, "The parent");
    fixture.codex(CODEX_CHILD_ID, Some(CODEX_ID), "A child task");
    let every = every(&fixture);
    let listed: Vec<String> = every.agents(now_ms()).into_iter().map(|a| a.id).collect();
    assert_eq!(listed, vec![format!("codex:{CODEX_ID}")]);
    let rows: Vec<String> = every.rows(now_ms()).into_iter().map(|r| r.id).collect();
    assert_eq!(rows, listed);
}

#[test]
fn a_running_antigravity_row_is_working() {
    let fixture = Fixture::new("every-running");
    fixture.antigravity(AGY_ID, "Busy", "RUNNING", 30);
    let rows = every(&fixture).rows(now_ms());
    assert_eq!(rows.len(), 1);
    assert!(rows[0].working);
    assert_eq!(rows[0].project, "agy-app");
}

#[test]
fn the_rows_are_cached_for_a_moment() {
    let fixture = Fixture::new("every-cache");
    fixture.codex(CODEX_ID, None, "First");
    let every = every(&fixture);
    let now = now_ms();
    assert_eq!(every.rows(now).len(), 1);
    fixture.codex(CODEX_CHILD_ID, None, "Second");
    assert_eq!(every.rows(now + CACHE_MS - 1).len(), 1);
    assert_eq!(every.rows(now + CACHE_MS).len(), 2);
}

#[test]
fn a_machine_without_the_tools_lists_nothing() {
    let fixture = Fixture::new("every-none");
    let every = every(&fixture);
    assert!(every.rows(now_ms()).is_empty());
    assert!(EverySession::none().rows(now_ms()).is_empty());
}

#[test]
fn an_unknown_tools_value_is_refused() {
    let fixture = Fixture::new("every-bad");
    let ops = ops_over(&fixture);
    for args in [json!({"tools": "codex"}), json!({"tools": 3})] {
        let e = ops.call(&cli(), ops::SESSIONS_LIST, &args).unwrap_err();
        assert_eq!(e.code, ErrorCode::BadRequest, "{args}");
    }
}
