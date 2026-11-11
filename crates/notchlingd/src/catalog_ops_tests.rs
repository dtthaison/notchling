//! `catalog.get` over fakes: a fake accounts store (in-memory Keychain, scripted HTTP), a temp
//! codex home and cache file, and a spawner the test runs by hand. Nothing here reaches a real
//! login, the network or `~/.claude`, `~/.codex` or `~/.gemini`.

use crate::accounts_ops_tests::{profile, world, World, ID1, ID2};
use crate::acting_ops_tests::{app, FakeRoom};
use crate::catalog_ops::{CatalogService, Spawner};
use crate::limits_ops_tests::{TempDir, NOW};
use crate::op_group::OpGroup;
use notchling_engine::accounts::claude::{LIVE_SERVICE, SAVED_SERVICE};
use notchling_engine::accounts::fake::{MemoryKeychain, PanicHttp, PanicKeychain, ScriptedHttp};
use notchling_engine::accounts::http::{PROFILE_URL, TOKEN_URL};
use notchling_engine::catalog::claude_live::MODELS_URL;
use notchling_engine::catalog::live_cache::LiveCache;
use notchling_engine::catalog::{Catalog, LiveModel, Tool};
use notchling_engine::tools::agy::ConfigOption;
use notchling_local_api::{ops, ErrorCode, OpSpec};
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};

const MODELS_LIST: &str = include_str!("../../engine/tests/fixtures/catalog/models-list.json");
const CODEX_CACHE: &str = r#"{"models":[
  {"slug":"gpt-test","display_name":"GPT Test","visibility":"list","priority":1,
   "context_window":272000,"default_reasoning_level":"high",
   "supported_reasoning_levels":[{"effort":"low"},{"effort":"high"}]}]}"#;

type Job = Box<dyn FnOnce() + Send>;

/// Holds what the service would have run on a thread, until the test runs it.
#[derive(Clone, Default)]
struct Jobs(Arc<Mutex<Vec<Job>>>);

impl Jobs {
    fn spawner(&self) -> Spawner {
        let jobs = self.0.clone();
        Arc::new(move |job| jobs.lock().unwrap().push(job))
    }

    fn waiting(&self) -> usize {
        self.0.lock().unwrap().len()
    }

    fn run(&self) {
        let jobs: Vec<Job> = std::mem::take(&mut *self.0.lock().unwrap());
        for job in jobs {
            job();
        }
    }
}

struct Rig {
    tmp: TempDir,
    world: World,
    chain: Arc<MemoryKeychain>,
    http: Arc<ScriptedHttp>,
    room: Arc<FakeRoom>,
    jobs: Jobs,
    service: Arc<CatalogService>,
    /// The service's clock, in seconds.
    clock: Arc<Mutex<f64>>,
    /// What the service logged.
    logs: Arc<Mutex<Vec<String>>>,
}

fn live_login(expires_at: Option<u64>) -> String {
    let expiry = expires_at.map_or(String::new(), |ms| format!(r#","expiresAt":{ms}"#));
    format!(r#"{{"claudeAiOauth":{{"accessToken":"test-access-1"{expiry}}}}}"#)
}

/// A service over the fake accounts, `on` or off, whose main login is `login`. The model list
/// endpoint answers nothing until a test scripts it.
fn rig(tag: &str, on: bool, login: &str) -> Rig {
    let chain = Arc::new(MemoryKeychain::new());
    let http = Arc::new(ScriptedHttp::new());
    let world = world(tag, on, chain.clone(), http.clone());
    std::fs::write(
        world.d.0.join("home/.claude.json"),
        "{\"oauthAccount\": {\"accountUuid\": \"a1a1\", \"organizationUuid\": \"0101\", \"emailAddress\": \"one@example.com\"}}",
    )
    .unwrap();
    chain.put(LIVE_SERVICE, "me", login);
    http.answer(PROFILE_URL, 200, &profile(ID1, "one@example.com"));
    if on {
        world.ops.store().sync().expect("the fake login is read");
    }
    let tmp = TempDir::new("catalog-ops");
    tmp.write("codex/models_cache.json", CODEX_CACHE);
    let room = Arc::new(FakeRoom::default());
    let jobs = Jobs::default();
    let store = world.ops.store();
    let clock = Arc::new(Mutex::new(NOW));
    let logs = Arc::new(Mutex::new(Vec::new()));
    let (now, log) = (clock.clone(), logs.clone());
    let service = Arc::new(
        CatalogService::new(
            Catalog::built_in(),
            LiveCache::new(tmp.path("state/live-models.json")),
            tmp.path("codex"),
            Some((store.clone(), world.flag.clone())),
            store.env().http.clone(),
        )
        .with_room(room.clone())
        .with_clock(Arc::new(move || *now.lock().unwrap()))
        .with_spawner(jobs.spawner())
        .with_claude_code_config(tmp.path("claude-code/.claude.json"))
        .with_log(Box::new(move |line| {
            log.lock().unwrap().push(line.to_string())
        })),
    );
    Rig {
        tmp,
        world,
        chain,
        http,
        room,
        jobs,
        service,
        clock,
        logs,
    }
}

/// A service over a temp folder, with no accounts and no network: the built-in data, `overrides`
/// as the user's `catalog.json` and `live` as Claude's cached list. The folder must outlive it.
pub(crate) fn seeded(
    tag: &str,
    live: &[LiveModel],
    overrides: Option<&str>,
) -> (TempDir, Arc<CatalogService>) {
    let tmp = TempDir::new(tag);
    if let Some(text) = overrides {
        tmp.write("state/catalog.json", text);
    }
    let cache = LiveCache::new(tmp.path("state/live-models.json"));
    if !live.is_empty() {
        cache.put(Tool::Claude, NOW, live);
    }
    let service = Arc::new(CatalogService::new(
        Catalog::load(&tmp.path("state/catalog.json")),
        cache,
        tmp.path("codex"),
        None,
        Arc::new(PanicHttp),
    ));
    (tmp, service)
}

impl Rig {
    fn get(&self, tool: &str) -> Result<Value, notchling_local_api::ApiError> {
        self.service
            .call(&app(), ops::CATALOG_GET, &json!({"tool": tool}))
    }

    fn model_requests(&self) -> Vec<(String, Option<String>)> {
        self.http
            .seen()
            .into_iter()
            .filter(|(url, _)| url.starts_with("https://api.anthropic.com/v1/models"))
            .collect()
    }

    fn cache(&self) -> LiveCache {
        LiveCache::new(self.tmp.path("state/live-models.json"))
    }
}

fn ids(reply: &Value) -> Vec<String> {
    reply["models"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["id"].as_str().unwrap().to_string())
        .collect()
}

#[test]
fn catalog_get_is_agent_tier_and_read_only() {
    let r = rig("spec", false, &live_login(None));
    assert_eq!(r.service.spec(ops::CATALOG_GET), Some(OpSpec::AGENT));
    assert_eq!(r.service.spec("catalog.set"), None);
    assert!(r.service.topics().contains(&"tools"));
}

#[test]
fn catalog_get_answers_each_tool_from_its_source() {
    let r = rig("sources", true, &live_login(None));
    // Claude's list is a day-fresh cached one, so nothing is fetched.
    r.cache().put(
        Tool::Claude,
        NOW - 100.0,
        &[LiveModel {
            id: "claude-fable-5-3".into(),
            name: Some("Claude Fable 5.3".into()),
            ..LiveModel::default()
        }],
    );
    let claude = r.get("claude").unwrap();
    assert_eq!(claude["tool"], "claude");
    assert_eq!(claude["fetchedAt"], NOW - 100.0);
    let fable = claude["models"]
        .as_array()
        .unwrap()
        .iter()
        .find(|m| m["id"] == "claude-fable-5-3")
        .expect("the live model is listed");
    assert_eq!(fable["borrowed"], true, "newer than the data file's Fable");
    assert!(r.model_requests().is_empty());

    let codex = r.get("codex").unwrap();
    assert_eq!(ids(&codex), ["gpt-test"]);
    let model = &codex["models"][0];
    assert_eq!(model["name"], "GPT Test");
    assert_eq!(model["contextWindow"], 272_000);
    assert_eq!(model["efforts"], json!(["low", "high"]));
    assert_eq!(model["defaultEffort"], "high");
    assert!(
        codex["fetchedAt"].as_f64().is_some(),
        "the cache file's mtime"
    );

    let agy = r.get("antigravity").unwrap();
    assert_eq!(agy["models"], json!([]));
    assert_eq!(agy["fetchedAt"], Value::Null);

    // Codex re-reads its cache when the file changes.
    r.tmp.write(
        "codex/models_cache.json",
        r#"{"models":[{"slug":"gpt-other","visibility":"list"}]}"#,
    );
    assert_eq!(ids(&r.get("codex").unwrap()), ["gpt-other"]);
}

#[test]
fn an_unknown_tool_is_invalid() {
    let r = rig("unknown", false, &live_login(None));
    for args in [json!({"tool": "gemini"}), json!({}), json!({"tool": 5})] {
        let e = r.service.call(&app(), ops::CATALOG_GET, &args).unwrap_err();
        assert_eq!(e.code, ErrorCode::BadRequest, "{args}");
    }
}

#[test]
fn a_due_claude_list_is_fetched_once_with_the_live_token_and_never_renewed() {
    let r = rig("due", true, &live_login(None));
    r.http.answer(MODELS_URL, 200, MODELS_LIST);
    // Nothing cached: the call answers from the data file at once and queues one fetch.
    let before = r.get("claude").unwrap();
    assert_eq!(before["fetchedAt"], Value::Null);
    assert!(!ids(&before).contains(&"claude-fable-5-3".to_string()));
    assert_eq!(r.jobs.waiting(), 1);
    // A second call while it is queued or running starts nothing.
    r.get("claude").unwrap();
    assert_eq!(r.jobs.waiting(), 1);
    assert!(r.model_requests().is_empty(), "off the caller's thread");

    r.jobs.run();
    let seen = r.model_requests();
    assert_eq!(seen.len(), 1);
    assert_eq!(seen[0].1.as_deref(), Some("test-access-1"));
    assert!(
        !r.http.seen().iter().any(|(url, _)| url == TOKEN_URL),
        "no token was renewed"
    );
    assert_eq!(r.cache().get(Tool::Claude).unwrap().0, NOW);
    assert_eq!(r.room.named("catalog.changed"), [json!({"tool": "claude"})]);

    // Within a day: the list is served from the cache and nothing more is asked.
    let after = r.get("claude").unwrap();
    assert!(ids(&after).contains(&"claude-fable-5-3".to_string()));
    assert_eq!(after["fetchedAt"], NOW);
    assert_eq!(r.jobs.waiting(), 0);
    assert_eq!(r.model_requests().len(), 1);
}

#[test]
fn an_expired_live_login_asks_for_nothing_and_a_failed_fetch_keeps_the_old_list() {
    let r = rig("expired", true, &live_login(Some(1_000)));
    // Another saved account has an expired slot, and the token endpoint would renew it: the
    // list fetch must never reach for it, whichever account is main.
    r.chain.put(
        SAVED_SERVICE,
        ID2,
        r#"{"claudeAiOauth":{"accessToken":"test-access-2","refreshToken":"test-refresh-2","expiresAt":1},
            "oauthAccount":{"accountUuid":"a2a2","organizationUuid":"0202","emailAddress":"two@example.com"}}"#,
    );
    std::fs::create_dir_all(r.world.d.0.join("data/daemon")).unwrap();
    std::fs::write(
        r.world.d.0.join("data/daemon/accounts.json"),
        format!(r#"[{{"id":"{ID2}","email":"two@example.com","plan":"Pro"}}]"#),
    )
    .unwrap();
    r.http.answer(
        TOKEN_URL,
        200,
        r#"{"access_token":"test-access-new","refresh_token":"test-refresh-new","expires_in":3600}"#,
    );
    r.get("claude").unwrap();
    r.jobs.run();
    assert!(r.model_requests().is_empty(), "no token, no request");
    assert!(
        !r.http.seen().iter().any(|(url, _)| url == TOKEN_URL),
        "nothing was renewed"
    );
    assert!(r.cache().get(Tool::Claude).is_none());

    // A fetch that fails leaves the cached list as it was.
    let r = rig("failed", true, &live_login(None));
    let old = [LiveModel {
        id: "claude-opus-5-5".into(),
        ..LiveModel::default()
    }];
    r.cache().put(Tool::Claude, NOW - 200_000.0, &old);
    r.http.answer(MODELS_URL, 500, "");
    r.get("claude").unwrap();
    r.jobs.run();
    let cached = r.cache().get(Tool::Claude).unwrap();
    assert_eq!((cached.0, cached.1), (NOW - 200_000.0, old.to_vec()));
    assert!(r.room.named("catalog.changed").is_empty());
    // And the next call does not hammer: a try that failed is not repeated for a while.
    r.get("claude").unwrap();
    assert_eq!(r.jobs.waiting(), 0);
}

#[test]
fn nothing_is_fetched_while_the_accounts_switch_is_off() {
    // The main login is known (the switch was on once), then the switch goes off.
    let r = rig("off", true, &live_login(None));
    r.http.answer(MODELS_URL, 200, MODELS_LIST);
    r.world.flag.set(false);
    let seen_before = r.http.seen().len();
    r.get("claude").unwrap();
    assert_eq!(r.jobs.waiting(), 0);
    r.jobs.run();
    assert_eq!(r.http.seen().len(), seen_before, "no request while off");
    // Not even a store of fakes that panic is touched.
    let w = world(
        "off-panic",
        false,
        Arc::new(PanicKeychain),
        Arc::new(PanicHttp),
    );
    let tmp = TempDir::new("catalog-off");
    let store = w.ops.store();
    let service = CatalogService::new(
        Catalog::built_in(),
        LiveCache::new(tmp.path("live.json")),
        tmp.path("codex"),
        Some((store.clone(), w.flag.clone())),
        store.env().http.clone(),
    );
    service.refresh_claude();
    assert!(
        !service.models(Tool::Claude).is_empty(),
        "the data file's list"
    );
}

impl Rig {
    fn at(&self, now: f64) {
        *self.clock.lock().unwrap() = now;
    }

    /// One `catalog.get` for Claude, running the fetch it starts.
    fn try_claude(&self) {
        self.get("claude").unwrap();
        self.jobs.run();
    }
}

#[test]
fn a_failed_fetch_is_logged_once_by_its_status_alone() {
    let r = rig("logged", true, &live_login(None));
    r.http
        .answer(MODELS_URL, 401, r#"{"error": "test-access-1 leaked"}"#);
    r.try_claude();
    r.at(NOW + 2.0 * 86_400.0);
    r.http.answer(MODELS_URL, 500, "oops");
    r.try_claude();
    assert_eq!(r.model_requests().len(), 2);
    let logs = r.logs.lock().unwrap().clone();
    assert_eq!(logs.len(), 1, "once per run: {logs:?}");
    assert!(logs[0].contains("status 401"), "{logs:?}");
    assert!(!logs[0].contains("test-access-1") && !logs[0].contains("leaked"));
}

#[test]
fn a_refused_login_is_not_asked_again_for_a_day_and_a_failure_for_15_minutes() {
    let r = rig("refused", true, &live_login(None));
    r.http.answer(MODELS_URL, 403, "");
    r.try_claude();
    assert_eq!(r.model_requests().len(), 1);
    // An hour later, still refused: nothing is asked.
    r.at(NOW + 3_600.0);
    r.try_claude();
    assert_eq!(r.model_requests().len(), 1);
    // A day later it is asked again; a 5xx then holds off only 15 minutes.
    r.at(NOW + 86_400.0 + 1.0);
    r.http.answer(MODELS_URL, 503, "");
    r.try_claude();
    assert_eq!(r.model_requests().len(), 2);
    r.at(NOW + 86_400.0 + 600.0);
    r.try_claude();
    assert_eq!(r.model_requests().len(), 2);
    r.at(NOW + 86_400.0 + 1_000.0);
    r.try_claude();
    assert_eq!(r.model_requests().len(), 3, "no answer, so tried again");
}

#[test]
fn a_401_or_a_429_holds_off_only_15_minutes() {
    for status in [401, 429] {
        let r = rig(&format!("short-{status}"), true, &live_login(None));
        r.http.answer(MODELS_URL, status, "");
        r.try_claude();
        assert_eq!(r.model_requests().len(), 1, "{status}");
        r.at(NOW + 600.0);
        r.try_claude();
        assert_eq!(r.model_requests().len(), 1, "{status}: within 15 minutes");
        r.at(NOW + 1_000.0);
        r.try_claude();
        assert_eq!(r.model_requests().len(), 2, "{status}: asked again");
    }
}

#[test]
fn a_new_live_token_ends_a_refusal() {
    let r = rig("refused-new-token", true, &live_login(None));
    r.http.answer(MODELS_URL, 403, "");
    r.try_claude();
    assert_eq!(r.model_requests().len(), 1);
    // The same token an hour later: nothing is sent.
    r.at(NOW + 3_600.0);
    r.try_claude();
    assert_eq!(r.model_requests().len(), 1);
    // Claude Code's login now holds another token: it is sent at the next try.
    r.chain.put(
        LIVE_SERVICE,
        "me",
        r#"{"claudeAiOauth":{"accessToken":"test-access-2"}}"#,
    );
    r.http.answer(MODELS_URL, 200, MODELS_LIST);
    r.at(NOW + 7_200.0);
    r.try_claude();
    let seen = r.model_requests();
    assert_eq!(seen.len(), 2);
    assert_eq!(seen[1].1.as_deref(), Some("test-access-2"));
    assert_eq!(r.cache().get(Tool::Claude).unwrap().0, NOW + 7_200.0);
}

/// Claude Code's main config with its own model list (the shape seen live).
const CLAUDE_CODE_CONFIG: &str = r#"{"oauthAccount": {"emailAddress": "one@example.com"},
  "additionalModelOptionsCache": [
    {"value": "claude-fable-5-3[1m]", "label": "Fable", "description": "Fable 5.3 · Most capable"}]}"#;

#[test]
fn claude_code_s_own_list_stands_in_until_anthropic_s_is_read() {
    let r = rig("fallback", true, &live_login(None));
    r.tmp.write("claude-code/.claude.json", CLAUDE_CODE_CONFIG);
    let config = r.tmp.path("claude-code/.claude.json");
    let before = std::fs::read(&config).unwrap();
    // The fetch fails: Claude Code's list is what shows, and what may start.
    r.http.answer(MODELS_URL, 401, "");
    let reply = r.get("claude").unwrap();
    r.jobs.run();
    let fable = reply["models"]
        .as_array()
        .unwrap()
        .iter()
        .find(|m| m["id"] == "claude-fable-5-3")
        .expect("Claude Code's model is listed")
        .clone();
    assert_eq!(fable["name"], "Fable 5.3");
    assert_eq!(fable["tier"], "current");
    assert!(reply["fetchedAt"].as_f64().is_some(), "the file's mtime");
    assert!(r.service.is_allowed(Tool::Claude, "claude-fable-5-3[1m]"));
    assert_eq!(std::fs::read(&config).unwrap(), before, "never written");

    // Once Anthropic's list is read, it comes first.
    r.at(NOW + 2.0 * 86_400.0);
    r.http.answer(MODELS_URL, 200, MODELS_LIST);
    r.try_claude();
    let reply = r.get("claude").unwrap();
    assert_eq!(reply["fetchedAt"], NOW + 2.0 * 86_400.0);

    // With no token at all, Claude Code's list shows too.
    let r = rig("fallback-expired", true, &live_login(Some(1_000)));
    r.tmp.write("claude-code/.claude.json", CLAUDE_CODE_CONFIG);
    r.try_claude();
    assert!(r.model_requests().is_empty());
    assert!(ids(&r.get("claude").unwrap()).contains(&"claude-fable-5-3".to_string()));
}

#[test]
fn agy_options_seen_are_cached_and_announced() {
    let r = rig("agy", false, &live_login(None));
    let options = ConfigOption::parse(Some(&json!([
        {"id": "model", "name": "Model", "type": "select", "currentValue": "m1",
         "options": [{"value": "m1", "name": "Model One"}, {"value": "m2", "name": "Model Two"}]},
        {"id": "thought_level", "name": "Thinking Level", "type": "select", "currentValue": "medium",
         "options": [{"value": "low", "name": "Low"}, {"value": "medium", "name": "Medium"}]},
        {"id": "mode", "name": "Mode", "type": "select", "currentValue": "default",
         "options": [{"value": "default", "name": "Default"}]},
    ])));
    r.service.record_agy_options(&options);
    let reply = r.get("antigravity").unwrap();
    assert_eq!(ids(&reply), ["m1", "m2"]);
    assert_eq!(reply["models"][1]["name"], "Model Two");
    assert_eq!(reply["models"][0]["efforts"], json!(["low", "medium"]));
    assert_eq!(reply["models"][0]["defaultEffort"], "medium");
    assert_eq!(reply["fetchedAt"], NOW);
    assert_eq!(r.cache().get(Tool::Antigravity).unwrap().1.len(), 2);
    assert_eq!(
        r.room.named("catalog.changed"),
        [json!({"tool": "antigravity"})]
    );

    // The same options again change nothing, so they announce nothing.
    r.service.record_agy_options(&options);
    assert_eq!(r.room.named("catalog.changed").len(), 1);
    // A list with no Model option is not a list: the last one stays.
    r.service.record_agy_options(&[]);
    assert_eq!(ids(&r.get("antigravity").unwrap()), ["m1", "m2"]);
}
