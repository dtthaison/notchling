//! The Antigravity ops called directly: a fake room, a scripted `agy` (the recorded exchange) and a
//! database in a temp folder. No test runs `agy` or reads a real `~/.gemini`.

use super::*;
use crate::acting::ACTING_OFF;
use crate::acting_ops_tests::{app, setup, FakeRoom};
use notchling_engine::tools::acp::fake::{FakeHandle, FakeSpawner};
use notchling_local_api::ops;
use rusqlite::Connection;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Mutex;
use std::time::Instant;

const RECORDING: &str =
    include_str!("../../../legacy/macos/Tests/NotchlingTests/Fixtures/acp-agy.jsonl");
const SESSION: &str = "d67413c2-a0f3-4381-8a8e-e98b541d6d76";
const ROW: &str = "agy:d67413c2-a0f3-4381-8a8e-e98b541d6d76";

fn scratch(tag: &str) -> PathBuf {
    static N: AtomicU32 = AtomicU32::new(0);
    let dir = std::env::temp_dir().join(format!(
        "nl-agyops-{tag}-{}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::SeqCst)
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// A summaries database with the recording's session in `folder`, modified just now.
fn antigravity_root(folder: &Path) -> PathBuf {
    let root = scratch("root");
    let db = Connection::open(root.join(summaries::DB_NAME)).unwrap();
    db.execute_batch(
        "CREATE TABLE conversation_summaries (conversation_id TEXT, title TEXT, preview TEXT, \
         workspace_uris TEXT, status TEXT, last_modified_time TEXT, nesting_depth INTEGER);",
    )
    .unwrap();
    let uris = json!([format!("file://{}", folder.display())]).to_string();
    let when = {
        // Now in the database's own format.
        let secs = now_secs() as i64;
        let (days, rem) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
        let (y, m, d) = civil(days);
        format!(
            "{y:04}-{m:02}-{d:02} {:02}:{:02}:{:02}.000000+00:00",
            rem / 3600,
            rem % 3600 / 60,
            rem % 60
        )
    };
    db.execute(
        "INSERT INTO conversation_summaries VALUES (?1, 'Edit hello', '', ?2, 'IDLE', ?3, 0)",
        rusqlite::params![SESSION, uris, when],
    )
    .unwrap();
    root
}

/// Days since 1970-01-01 as (year, month, day).
fn civil(days: i64) -> (i64, i64, i64) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (yoe + era * 400 + i64::from(m <= 2), m, d)
}

struct Rig {
    /// Antigravity's folder, with its summaries database.
    root: PathBuf,
    /// Where the daemon makes its copy of that database.
    copies: PathBuf,
    ops: AgyOps,
    room: Arc<FakeRoom>,
    spawner: Arc<FakeSpawner>,
    folder: PathBuf,
}

fn rig_with(on: bool, spawner: FakeSpawner, binary: Option<&str>) -> Rig {
    let folder = scratch("work");
    let room = Arc::new(FakeRoom::default());
    let acting = Arc::new(Acting::in_memory(on));
    let spawner = Arc::new(spawner);
    let root = antigravity_root(&folder);
    let copies = scratch("copy");
    let ops = AgyOps::new(
        acting.clone(),
        room.clone(),
        spawner.clone(),
        AgySetup {
            root: root.clone(),
            copy_dir: copies.clone(),
            home: scratch("home"),
            binary: binary.map(String::from),
            start_wait: Duration::from_secs(10),
            timing: Timing::default(),
        },
    );
    Rig {
        root,
        copies,
        ops,
        room,
        spawner,
        folder,
    }
}

fn rig(on: bool) -> Rig {
    rig_with(on, FakeSpawner::replaying(RECORDING), Some("/fake/agy"))
}

impl Rig {
    fn wait(&self, event: &str) -> Value {
        self.wait_n(event, 1).remove(0)
    }

    fn wait_n(&self, event: &str, n: usize) -> Vec<Value> {
        let end = Instant::now() + Duration::from_secs(10);
        loop {
            let found = self.room.named(event);
            if found.len() >= n {
                return found;
            }
            assert!(Instant::now() < end, "timed out waiting for {event}");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    fn change(&self, change: &str) {
        let end = Instant::now() + Duration::from_secs(10);
        while !self
            .room
            .named(topics::SESSION_CHANGED)
            .iter()
            .any(|d| d["change"] == change)
        {
            assert!(Instant::now() < end, "timed out waiting for {change}");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    fn call(&self, op: &str, args: Value) -> Result<Value, ApiError> {
        self.ops.call(op, &args)
    }

    fn process(&self) -> FakeHandle {
        self.spawner.spawned().remove(0)
    }

    /// Opens the recorded session and waits for the load.
    fn loaded(&self) -> FakeHandle {
        self.call(OPEN, json!({"id": ROW})).unwrap();
        self.change("loaded");
        self.process()
    }
}

fn refused(result: Result<Value, ApiError>) -> (ErrorCode, String) {
    let e = result.unwrap_err();
    (e.code, e.message)
}

#[test]
fn acting_off_refuses_every_acting_op_before_anything_starts() {
    let rig = rig(false);
    for (op, args) in [
        (OPEN, json!({"id": ROW})),
        (SEND, json!({"id": ROW, "text": "hi"})),
        (
            SEND,
            json!({"draft_folder": rig.folder.to_string_lossy(), "text": "hi"}),
        ),
        (CANCEL, json!({"id": ROW})),
        (
            SET_OPTION,
            json!({"id": ROW, "option": "model", "value": "x"}),
        ),
        (
            PERMISSION,
            json!({"id": ROW, "rpc_id": "1", "option_id": "allow"}),
        ),
    ] {
        assert_eq!(
            refused(rig.call(op, args)),
            (ErrorCode::Forbidden, ACTING_OFF.to_string()),
            "{op}"
        );
    }
    assert!(rig.spawner.calls().is_empty(), "no agy was started");
    // The reads work.
    let list = rig.call(SESSIONS, json!({})).unwrap();
    assert_eq!(list[0]["id"], ROW);
    assert_eq!(list[0]["name"], "Edit hello");
    assert_eq!(list[0]["cwd"], rig.folder.to_string_lossy().as_ref());
    assert_eq!(list[0]["working"], false);
    // The session index gets the same list by title, with no acting and no process.
    let titled = rig.ops.titled_sessions();
    assert_eq!(titled.len(), 1);
    assert_eq!(
        (titled[0].id.as_str(), titled[0].title.as_str()),
        (SESSION, "Edit hello")
    );
    assert_eq!(titled[0].folder, rig.folder.to_string_lossy());
    let state = rig.call(STATE, json!({"id": ROW})).unwrap();
    assert_eq!(state["open"], false);
}

#[test]
fn the_tiers() {
    let rig = rig(true);
    assert_eq!(rig.ops.spec(SESSIONS), Some(OpSpec::AGENT));
    for op in [
        STATE,
        CONVERSATION,
        OPEN,
        SEND,
        CANCEL,
        SET_OPTION,
        PERMISSION,
    ] {
        assert_eq!(rig.ops.spec(op), Some(OpSpec::APP), "{op}");
    }
    assert_eq!(rig.ops.spec("agy.run"), None);
    assert_eq!(rig.ops.spec(ops::SESSIONS_LIST), None);
}

#[test]
fn the_conversation_read_works_with_acting_off_and_follows_the_open_runner() {
    // Off: a read, so it answers; nothing is open, so it is empty and starts nothing.
    let off = rig(false);
    let empty = off.call(CONVERSATION, json!({"id": ROW})).unwrap();
    assert_eq!(empty["open"], false);
    assert_eq!(empty["loading"], false);
    assert_eq!(empty["working"], false);
    assert_eq!(empty["conversation"]["items"], json!([]));
    assert!(off.spawner.calls().is_empty());
    for bad_id in [json!({"id": "x"}), json!({"id": ROW, "peek": "yes"})] {
        assert_eq!(
            refused(off.call(CONVERSATION, bad_id)).0,
            ErrorCode::BadRequest
        );
    }

    // On and loaded: the recording's history, decodable as the engine's own type.
    let rig = rig(true);
    rig.loaded();
    let loaded = rig
        .call(CONVERSATION, json!({"id": ROW, "peek": true}))
        .unwrap();
    assert_eq!(loaded["id"], ROW);
    assert_eq!(loaded["open"], true);
    let conversation: notchling_engine::transcript::model::Conversation =
        serde_json::from_value(loaded["conversation"].clone()).unwrap();
    assert!(
        !conversation.items.is_empty(),
        "session/load replayed the history"
    );
    // Not on the daemon's handler as an agent: the app tier only.
    use crate::daemon::Ops;
    use notchling_local_api::Handler;
    let ops = Ops::new(Arc::new(Mutex::new(json!([])))).with_agy(Arc::new(rig.ops));
    assert_eq!(ops.spec(CONVERSATION), Some(OpSpec::APP));
    let read = ops.call(&app(), CONVERSATION, &json!({"id": ROW})).unwrap();
    assert_eq!(read["open"], true);
}

#[test]
fn open_runs_agy_in_the_sessions_own_folder_and_refuses_one_it_does_not_know() {
    let rig = rig(true);
    for bad_id in ["agy:nope", "agy:", "agy:../x", "claude:abc", "x"] {
        assert_eq!(
            refused(rig.call(OPEN, json!({"id": bad_id}))).0,
            ErrorCode::BadRequest,
            "{bad_id}"
        );
    }
    assert!(rig.spawner.calls().is_empty(), "nothing started for those");
    let reply = rig.call(OPEN, json!({"id": ROW})).unwrap();
    assert_eq!(reply, json!({"id": ROW, "loading": true}));
    let (program, args, folder) = rig.spawner.calls().remove(0);
    assert_eq!(program, "/fake/agy");
    assert_eq!(args, vec!["acp".to_string()]);
    assert_eq!(folder, rig.folder.to_string_lossy());
    rig.change("loaded");
    let state = rig.call(STATE, json!({"id": ROW})).unwrap();
    assert_eq!(state["open"], true);
    assert_eq!(state["loading"], false);
    assert!(!state["config"].as_array().unwrap().is_empty());
    // The caller can't send a folder of its own.
    let again = rig
        .call(OPEN, json!({"id": ROW, "folder": "/etc"}))
        .unwrap();
    assert_eq!(again["loading"], true);
    assert_eq!(rig.spawner.calls().len(), 1);
}

#[test]
fn an_uninstalled_agy_says_so() {
    let rig = rig_with(true, FakeSpawner::new(), None);
    let (code, message) = refused(rig.call(OPEN, json!({"id": ROW})));
    assert_eq!(code, ErrorCode::Failed);
    assert!(message.contains("Install Antigravity CLI"));
}

#[test]
fn a_permission_is_held_for_the_app_published_twice_and_answered_by_its_ids() {
    let rig = rig(true);
    let process = rig.loaded();
    let sent = rig
        .call(SEND, json!({"id": ROW, "text": "Change hello.txt"}))
        .unwrap();
    assert_eq!(sent, json!({"id": ROW, "sent": true}));
    assert_eq!(
        refused(rig.call(SEND, json!({"id": ROW, "text": "again"}))).0,
        ErrorCode::Failed,
        "busy"
    );
    let pending = rig.wait(PERMISSION_PENDING);
    let id = format!("agy-perm:{ROW}:1");
    assert_eq!(pending["id"], id.as_str());
    assert_eq!(pending["sessionId"], ROW);
    assert_eq!(pending["tool"], "agy");
    assert_eq!(pending["kind"], "permission");
    assert_eq!(pending["toolName"], "edit_file");
    assert_eq!(pending["summary"], "File hello.txt");
    assert_eq!(pending["rpcId"], "1");
    assert_eq!(pending["options"][0]["id"], "allow");
    assert_eq!(pending["diff"][1]["text"], "hello from Notchling");
    assert_eq!(pending["canAlwaysAllow"], false);
    assert_eq!(
        rig.wait(topics::PROMPT_PENDING)["id"],
        id.as_str(),
        "the notch's own event too"
    );
    // It is listed for an app that attaches later.
    assert!(rig.ops.prompts().iter().any(|p| p["id"] == id.as_str()));

    // Nothing answers it by itself.
    std::thread::sleep(Duration::from_millis(100));
    assert!(process.written().iter().all(|m| m.get("method").is_some()));
    // Wrong rpc id and an option it doesn't have are refused, and it still waits.
    for args in [
        json!({"id": ROW, "rpc_id": "2", "option_id": "allow"}),
        json!({"id": ROW, "rpc_id": "1", "option_id": "yes-always"}),
        json!({"id": ROW, "rpc_id": "1", "option_id": 5}),
        json!({"id": ROW, "option_id": "allow"}),
    ] {
        assert_eq!(refused(rig.call(PERMISSION, args)).0, ErrorCode::BadRequest);
    }
    assert_eq!(rig.ops.prompts().len(), 1);

    let reply = rig
        .call(
            PERMISSION,
            json!({"id": ROW, "rpc_id": "1", "option_id": "allow"}),
        )
        .unwrap();
    assert_eq!(reply, json!({"outcome": "allow_once"}));
    let resolved = rig.wait(PERMISSION_RESOLVED);
    assert_eq!(resolved["id"], id.as_str());
    assert_eq!(resolved["outcome"], "allow_once");
    assert_eq!(rig.wait(topics::PROMPT_RESOLVED)["id"], id.as_str());
    assert!(rig.ops.prompts().is_empty());
    rig.change("turn_ended");
    assert_eq!(
        refused(rig.call(
            PERMISSION,
            json!({"id": ROW, "rpc_id": "1", "option_id": "allow"})
        ))
        .0,
        ErrorCode::BadRequest,
        "answered once only"
    );
}

#[test]
fn cancel_stops_the_turn_and_cancels_what_waits() {
    let rig = rig(true);
    let process = rig.loaded();
    rig.call(SEND, json!({"id": ROW, "text": "edit"})).unwrap();
    rig.wait(PERMISSION_PENDING);
    assert_eq!(
        rig.call(CANCEL, json!({"id": ROW})).unwrap(),
        json!({"cancelled": true})
    );
    let resolved = rig.wait(PERMISSION_RESOLVED);
    assert_eq!(resolved["outcome"], "cancelled");
    assert!(!process.requests("session/cancel").is_empty());
    assert!(rig.ops.prompts().is_empty());
}

#[test]
fn a_draft_starts_a_session_in_its_folder_and_sends_the_first_message() {
    let responder: notchling_engine::tools::acp::fake::Responder =
        Arc::new(|m: &Value, p: &FakeHandle| {
            let reply =
                |result: Value| p.push(&json!({"jsonrpc": "2.0", "id": m["id"], "result": result}));
            match m["method"].as_str() {
                Some("initialize") => reply(json!({"protocolVersion": 1})),
                Some("session/new") => reply(json!({"sessionId": "fresh-1"})),
                Some("session/prompt") => reply(json!({"stopReason": "end_turn"})),
                _ => {}
            }
        });
    let rig = rig_with(
        true,
        FakeSpawner::with_responder(responder),
        Some("/fake/agy"),
    );
    let folder = rig.folder.to_string_lossy().into_owned();
    for bad_folder in ["relative/dir", "/no/such/folder/anywhere"] {
        assert_eq!(
            refused(rig.call(SEND, json!({"draft_folder": bad_folder, "text": "hi"}))).0,
            ErrorCode::BadRequest
        );
    }
    assert_eq!(
        refused(rig.call(SEND, json!({"text": "hi"}))).0,
        ErrorCode::BadRequest
    );
    assert_eq!(
        refused(rig.call(SEND, json!({"id": ROW, "text": "  "}))).0,
        ErrorCode::BadRequest
    );
    assert!(rig.spawner.calls().is_empty());

    let reply = rig
        .call(
            SEND,
            json!({"draft_folder": folder, "text": "Build the thing\nplease"}),
        )
        .unwrap();
    assert_eq!(reply, json!({"id": "agy:fresh-1", "sent": true}));
    assert_eq!(rig.spawner.calls()[0].2, folder);
    rig.change("turn_ended");
    // The change event carries the row as sessions.list gives rows, as Codex's does.
    let ended = rig
        .room
        .named(topics::SESSION_CHANGED)
        .into_iter()
        .find(|d| d["change"] == "turn_ended")
        .unwrap();
    assert_eq!(ended["id"], "agy:fresh-1");
    assert_eq!(ended["session"], "fresh-1");
    assert_eq!(ended["row"]["id"], "agy:fresh-1");
    assert_eq!(ended["row"]["host"], "Antigravity");
    assert_eq!(ended["row"]["group"], "completed");
    // The new session is listed before Antigravity's database has it, titled from the message.
    let list = rig.call(SESSIONS, json!({})).unwrap();
    let fresh = list
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["id"] == "agy:fresh-1")
        .unwrap();
    assert_eq!(fresh["name"], "Build the thing");
    assert_eq!(fresh["cwd"], folder.as_str());
    // And takes its next message without a second process.
    rig.call(SEND, json!({"id": "agy:fresh-1", "text": "more"}))
        .unwrap();
    assert_eq!(rig.spawner.spawned().len(), 1);
}

#[test]
fn options_pass_only_what_the_session_offers() {
    let rig = rig(true);
    let process = rig.loaded();
    let state = rig.call(STATE, json!({"id": ROW})).unwrap();
    let first = &state["config"][0];
    let (option, current) = (
        first["id"].as_str().unwrap(),
        first["current"].as_str().unwrap(),
    );
    let other = first["values"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v["value"].as_str().unwrap())
        .find(|v| *v != current)
        .unwrap();
    assert_eq!(
        refused(rig.call(
            SET_OPTION,
            json!({"id": ROW, "option": option, "value": "made-up"})
        ))
        .0,
        ErrorCode::BadRequest
    );
    assert_eq!(
        refused(rig.call(
            SET_OPTION,
            json!({"id": ROW, "option": "nope", "value": other})
        ))
        .0,
        ErrorCode::BadRequest
    );
    assert_eq!(
        refused(rig.call(SET_OPTION, json!({"id": ROW, "option": option}))).0,
        ErrorCode::BadRequest
    );
    assert!(process.requests("session/set_config_option").is_empty());
    let reply = rig
        .call(
            SET_OPTION,
            json!({"id": ROW, "option": option, "value": other}),
        )
        .unwrap();
    assert_eq!(reply, json!({"requested": true}));
    assert_eq!(process.requests("session/set_config_option").len(), 1);
}

#[test]
fn a_loaded_sessions_options_become_antigravitys_models_in_the_catalog() {
    use notchling_engine::accounts::fake::PanicHttp;
    use notchling_engine::catalog::live_cache::LiveCache;
    use notchling_engine::catalog::{Catalog, Tool};
    let rig = rig(true);
    let state = scratch("catalog");
    let catalog = Arc::new(
        CatalogService::new(
            Catalog::built_in(),
            LiveCache::new(state.join("live-models.json")),
            state.join("codex"),
            None,
            Arc::new(PanicHttp),
        )
        .with_room(rig.room.clone()),
    );
    rig.ops.attach_catalog(catalog.clone());
    assert!(
        catalog.models(Tool::Antigravity).is_empty(),
        "nothing seen yet"
    );
    rig.loaded();
    // The recording's session offers these Model values, and its Thinking Level's as efforts.
    let models = catalog.models(Tool::Antigravity);
    let ids: Vec<_> = models.iter().map(|m| m.id.as_str()).collect();
    assert!(
        ids.contains(&"gemini-4-argon") && ids.contains(&"claude-opus-5-5"),
        "{ids:?}"
    );
    let argon = models.iter().find(|m| m.id == "gemini-4-argon").unwrap();
    assert_eq!(argon.name, "Gemini 4 Argon");
    assert!(!argon.efforts.is_empty(), "{argon:?}");
    assert_eq!(argon.default_effort.as_deref(), Some("medium"));
    // Cached for the next start, and announced on `tools`.
    let cached = LiveCache::new(state.join("live-models.json"));
    assert!(cached.get(Tool::Antigravity).is_some());
    assert!(!rig.room.named(topics::CATALOG_CHANGED).is_empty());
}

#[test]
fn the_daemons_handler_routes_them_with_their_tiers() {
    use crate::daemon::Ops;
    use notchling_local_api::Handler;
    let rig = rig(false);
    let ops = Ops::new(Arc::new(Mutex::new(json!([])))).with_agy(Arc::new(rig.ops));
    assert_eq!(ops.spec(SESSIONS), Some(OpSpec::AGENT));
    assert_eq!(ops.spec(SEND), Some(OpSpec::APP));
    assert!(ops.call(&app(), SESSIONS, &json!({})).is_ok());
    let off = ops.call(&app(), OPEN, &json!({"id": ROW})).unwrap_err();
    assert_eq!(
        (off.code, off.message.as_str()),
        (ErrorCode::Forbidden, ACTING_OFF)
    );
}

#[path = "agy_ops_limits_tests.rs"]
mod limits;
