use crate::acting_ops_tests::app;
use crate::catalog_ops::CatalogService;
use crate::install_ops_tests::{self as install_rig, Dir};
use crate::limits_ops_tests::TempDir;
use crate::models_ops::{ModelsOps, Sources};
use crate::op_group::OpGroup;
use notchling_engine::accounts::fake::PanicHttp;
use notchling_engine::catalog::live_cache::LiveCache;
use notchling_engine::catalog::roles::Roles;
use notchling_engine::catalog::{Catalog, LiveModel, Tool};
use notchling_local_api::{ops, Caller, ClientKind, ErrorCode, OpSpec, Tier};
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};

const AGY: &str = "Fetching available models...\ngemini-4-neon-high\tGemini 4 Neon (High)\ngemini-4-neon-xhigh\tGemini 4 Neon (XHigh)\ngemini-4-neon-medium\tGemini 4 Neon (Medium)\ngemini-4-neon-low\tGemini 4 Neon (Low)\ngemini-4-argon-medium\tGemini 4 Argon (Medium)\ngemini-4-argon-high\tGemini 4 Argon (High)\ngemini-4-hydrogen-high\tGemini 4 Hydrogen (High)\ngemini-4-hydrogen-xhigh\tGemini 4 Hydrogen (XHigh)\ngpt-oss-120b-medium\tGPT-OSS 120B (Medium)\n";

const OLD_CODEX: &str = r#"{"models":[
  {"slug":"gpt-6-astra","display_name":"GPT-6-Astra","visibility":"list","priority":1,
   "supported_reasoning_levels":[{"effort":"medium"},{"effort":"high"}]}]}"#;

const NEW_CODEX: &str = r#"{"models":[
  {"slug":"gpt-6.2-astra","display_name":"GPT-6.2-Astra","visibility":"list","priority":1,
   "supported_reasoning_levels":[{"effort":"medium"},{"effort":"xhigh"}]},
  {"slug":"gpt-6-astra","display_name":"GPT-6-Astra","visibility":"list","priority":2,
   "supported_reasoning_levels":[{"effort":"medium"},{"effort":"high"}]},
  {"slug":"gpt-5","display_name":"GPT-5","visibility":"list","priority":3}]}"#;

#[derive(Clone, Default)]
struct Tools {
    agy: Arc<Mutex<Option<String>>>,
    codex: Arc<Mutex<Option<String>>>,
    calls: Arc<Mutex<Vec<String>>>,
}

impl Tools {
    fn sources(&self) -> Sources {
        let (agy, codex, calls) = (self.agy.clone(), self.codex.clone(), self.calls.clone());
        Sources {
            codex: Some("/fake/codex".into()),
            agy: Some("/fake/agy".into()),
            capture: Arc::new(move |program, args| {
                calls
                    .lock()
                    .unwrap()
                    .push(format!("{} {}", program.display(), args.join(" ")));
                match (&*program.to_string_lossy(), args) {
                    ("/fake/agy", ["models"]) => agy.lock().unwrap().clone(),
                    ("/fake/codex", ["debug", "models"]) => codex.lock().unwrap().clone(),
                    _ => None,
                }
            }),
        }
    }
}

struct Rig {
    tmp: TempDir,
    home: Dir,
    catalog: Arc<CatalogService>,
    models: ModelsOps,
    tools: Tools,
}

fn old_agy() -> Vec<LiveModel> {
    ["gemini-4-argon", "gemini-4-hydrogen"]
        .iter()
        .map(|id| LiveModel {
            id: (*id).into(),
            efforts: ["low", "medium", "high"].map(String::from).to_vec(),
            default_effort: Some("medium".into()),
            ..LiveModel::default()
        })
        .collect()
}

fn rig(tag: &str, install_on: bool) -> Rig {
    let tmp = TempDir::new(tag);
    tmp.write("codex/models_cache.json", OLD_CODEX);
    let catalog = Arc::new(
        CatalogService::new(
            Catalog::built_in(),
            LiveCache::new(tmp.path("state/live-models.json")),
            tmp.path("codex"),
            None,
            Arc::new(PanicHttp),
        )
        .with_override_file(tmp.path("state/catalog.json"))
        .with_codex_snapshot(tmp.path("state/codex-models.json")),
    );
    catalog.store_live(Tool::Antigravity, &old_agy());
    let home = Dir::new(tag);
    let (install, _) = install_rig::ops_for(install_rig::rig(&home, None), install_on);
    let tools = Tools::default();
    *tools.agy.lock().unwrap() = Some(AGY.to_string());
    *tools.codex.lock().unwrap() = Some(NEW_CODEX.to_string());
    let models = ModelsOps::new(catalog.clone(), Some(Arc::new(install)), tools.sources());
    Rig {
        tmp,
        home,
        catalog,
        models,
        tools,
    }
}

fn call(rig: &Rig, op: &str, args: Value) -> Value {
    rig.models.call(&app(), op, &args).unwrap()
}

fn agent() -> Caller {
    Caller {
        tier: Tier::Agent,
        client: ClientKind::Cli,
        peer: None,
        pid: None,
    }
}

fn age(path: &std::path::Path, secs: u64) {
    let file = std::fs::OpenOptions::new().write(true).open(path).unwrap();
    file.set_modified(std::time::SystemTime::now() - std::time::Duration::from_secs(secs))
        .unwrap();
}

fn part<'a>(report: &'a Value, tool: &str) -> &'a Value {
    report["tools"]
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["tool"] == tool)
        .unwrap()
}

fn strings(v: &Value) -> Vec<&str> {
    v.as_array()
        .unwrap()
        .iter()
        .map(|s| s.as_str().unwrap())
        .collect()
}

fn skill_text(rig: &Rig, tool_dir: &str, name: &str) -> Option<String> {
    std::fs::read_to_string(
        rig.home
            .path(&format!("home/{tool_dir}/skills/{name}/SKILL.md")),
    )
    .ok()
}

#[test]
fn both_ops_are_agent_tier_and_a_bad_argument_is_refused() {
    let rig = rig("spec", true);
    for op in [ops::MODELS_CHECK, ops::MODELS_UPDATE] {
        assert_eq!(rig.models.spec(op), Some(OpSpec::AGENT_READ), "{op}");
        let e = rig
            .models
            .call(&app(), op, &json!({"skills": "yes"}))
            .unwrap_err();
        assert_eq!(e.code, ErrorCode::BadRequest);
    }
    assert_eq!(rig.models.spec("models.other"), None);
}

#[test]
fn a_check_reports_the_difference_and_writes_nothing() {
    let rig = rig("check", true);
    let report = call(&rig, ops::MODELS_CHECK, json!({}));
    assert_eq!(report["applied"], false);
    assert_eq!(report["changed"], true);

    let agy = part(&report, "antigravity");
    assert_eq!(agy["read"], true);
    assert_eq!(
        strings(&agy["added"]),
        ["gemini-4-neon", "gpt-oss-120b-medium"]
    );
    let hydrogen = agy["efforts"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["model"] == "gemini-4-hydrogen")
        .unwrap();
    assert_eq!(hydrogen["added"], json!(["xhigh"]));
    assert_eq!(agy["written"], false);

    let codex = part(&report, "codex");
    assert_eq!(strings(&codex["added"]), ["gpt-6.2-astra", "gpt-5"]);
    assert!(strings(&codex["unpriced"]).contains(&"gpt-6.2-astra"));
    assert!(
        !strings(&codex["unpriced"]).contains(&"gpt-5"),
        "gpt-5 has a price"
    );

    let claude = part(&report, "claude");
    assert_eq!(claude["read"], false);
    assert!(claude["note"]
        .as_str()
        .unwrap()
        .contains("Claude accounts aren't available"));

    assert_eq!(
        rig.catalog.live_models(Tool::Antigravity),
        old_agy(),
        "the cache is as it was"
    );
    assert!(!rig.tmp.path("state/codex-models.json").exists());
    assert!(skill_text(&rig, ".claude", "notchling-models").is_none());
    let skills = strings(&report["skills"]);
    assert!(
        skills.contains(&"notchling-models (claude): would write"),
        "{skills:?}"
    );
    assert!(report["text"]
        .as_str()
        .unwrap()
        .contains("This was a check: nothing was written."));
}

#[test]
fn an_update_writes_the_lists_so_xhigh_is_offered_only_where_listed() {
    let rig = rig("update", true);
    age(&rig.tmp.path("codex/models_cache.json"), 3_600);
    let report = call(&rig, ops::MODELS_UPDATE, json!({}));
    assert_eq!(report["applied"], true);
    assert_eq!(part(&report, "antigravity")["written"], true);
    assert_eq!(part(&report, "codex")["written"], true);

    let agy = rig.catalog.models(Tool::Antigravity);
    let efforts = |id: &str| agy.iter().find(|m| m.id == id).unwrap().efforts.clone();
    assert_eq!(efforts("gemini-4-neon"), ["low", "medium", "high", "xhigh"]);
    assert_eq!(efforts("gemini-4-hydrogen"), ["high", "xhigh"]);
    assert_eq!(efforts("gemini-4-argon"), ["medium", "high"]);
    assert!(!efforts("gemini-4-argon").contains(&"xhigh".to_string()));
    assert_eq!(
        agy.iter()
            .find(|m| m.id == "gemini-4-argon")
            .unwrap()
            .default_effort
            .as_deref(),
        Some("medium")
    );

    let codex: Vec<String> = rig
        .catalog
        .models(Tool::Codex)
        .into_iter()
        .map(|m| m.id)
        .collect();
    assert_eq!(codex, ["gpt-6.2-astra", "gpt-6-astra", "gpt-5"]);
    assert!(rig.tmp.path("state/codex-models.json").exists());
    assert!(rig.catalog.is_allowed(Tool::Codex, "gpt-6.2-astra"));
    assert!(rig.tmp.path("codex/models_cache.json").exists());
    assert_eq!(
        std::fs::read_to_string(rig.tmp.path("codex/models_cache.json")).unwrap(),
        OLD_CODEX,
        "Codex's own cache is not touched"
    );

    let again = call(&rig, ops::MODELS_UPDATE, json!({}));
    assert_eq!(again["changed"], false, "{}", again["text"]);
    assert_eq!(part(&again, "antigravity")["written"], false);
    assert_eq!(part(&again, "codex")["written"], false);
}

#[test]
fn an_update_rewrites_the_delegate_skill_and_installs_the_models_skill() {
    let rig = rig("skills", true);
    let report = call(&rig, ops::MODELS_UPDATE, json!({}));
    let skills = strings(&report["skills"]);
    assert!(
        skills.contains(&"notchling-models (claude): written"),
        "{skills:?}"
    );
    assert!(
        skills.contains(&"notchling-delegate (codex): written"),
        "{skills:?}"
    );

    let delegate = skill_text(&rig, ".claude", "notchling-delegate").unwrap();
    assert!(delegate.contains("each model offers its own efforts"));
    assert!(!delegate.contains("become high"));
    assert!(delegate.contains("models update"));
    let models = skill_text(&rig, ".claude", "notchling-models").unwrap();
    assert!(models.starts_with("---\nname: notchling-models\n"));
    assert!(skill_text(&rig, ".gemini/config", "notchling-models").is_some());

    let report = call(&rig, ops::MODELS_UPDATE, json!({}));
    let skills = strings(&report["skills"]);
    assert!(
        skills.contains(&"notchling-models (claude): up to date"),
        "{skills:?}"
    );
}

#[test]
fn with_the_install_switch_off_the_skills_are_left_and_said_so() {
    let rig = rig("off", false);
    let report = call(&rig, ops::MODELS_UPDATE, json!({}));
    assert_eq!(
        strings(&report["skills"]),
        ["not rewritten: the Install Hooks and Skills switch is off in Notchling"]
    );
    assert!(skill_text(&rig, ".claude", "notchling-models").is_none());
    assert_eq!(part(&report, "antigravity")["written"], true);

    let none = call(&rig, ops::MODELS_UPDATE, json!({"skills": false}));
    assert_eq!(none["skills"], json!([]));
}

#[test]
fn role_picks_are_suggested_and_never_applied() {
    let rig = rig("roles", true);
    let report = call(&rig, ops::MODELS_UPDATE, json!({}));
    let lines = strings(&report["suggestions"]);
    assert!(
        lines
            .iter()
            .any(|l| l.contains("gpt-6-astra") && l.contains("newer in family: gpt-6.2-astra")),
        "{lines:?}"
    );
    assert!(
        !rig.tmp.path("state/catalog.json").exists(),
        "no override file is made or changed"
    );
    let roles = Roles::load(Some(&rig.tmp.path("state/catalog.json")));
    assert_eq!(roles, Roles::built_in());
    let seen = call(&rig, ops::MODELS_CHECK, json!({}));
    assert_eq!(seen["suggestions"], report["suggestions"]);
}

#[test]
fn a_tool_that_cannot_be_read_keeps_its_list_and_says_why() {
    let rig = rig("down", true);
    *rig.tools.agy.lock().unwrap() = None;
    *rig.tools.codex.lock().unwrap() = Some("not json".into());
    let report = call(&rig, ops::MODELS_UPDATE, json!({"skills": false}));
    let agy = part(&report, "antigravity");
    assert_eq!(agy["read"], false);
    assert!(agy["note"]
        .as_str()
        .unwrap()
        .contains("couldn't run agy models"));
    assert_eq!(agy["written"], false);
    assert_eq!(rig.catalog.live_models(Tool::Antigravity), old_agy());
    let codex = part(&report, "codex");
    assert_eq!(codex["written"], false);
    assert_eq!(codex["total"], 1);
    assert!(!rig.tmp.path("state/codex-models.json").exists());
    let calls = rig.tools.calls.lock().unwrap().clone();
    assert!(calls.contains(&"/fake/agy models".to_string()));
    assert!(calls.contains(&"/fake/codex debug models".to_string()));
}

#[test]
fn the_tools_are_only_asked_for_their_listings() {
    let rig = rig("calls", true);
    call(&rig, ops::MODELS_UPDATE, json!({}));
    let calls = rig.tools.calls.lock().unwrap().clone();
    assert_eq!(calls, ["/fake/codex debug models", "/fake/agy models"]);
}

#[test]
fn an_agent_caller_never_rewrites_skills_but_a_check_says_what_would_change() {
    let rig = rig("agent-skills", true);
    let report = rig
        .models
        .call(&agent(), ops::MODELS_UPDATE, &json!({}))
        .unwrap();
    assert_eq!(
        strings(&report["skills"]),
        ["skills not rewritten (needs the app)"]
    );
    assert!(skill_text(&rig, ".claude", "notchling-models").is_none());
    assert!(skill_text(&rig, ".claude", "notchling-delegate").is_none());
    assert_eq!(
        part(&report, "antigravity")["written"],
        true,
        "the lists are still written"
    );

    let check = rig
        .models
        .call(&agent(), ops::MODELS_CHECK, &json!({}))
        .unwrap();
    assert!(strings(&check["skills"]).contains(&"notchling-models (claude): would write"));
}

fn five_agy_models(rig: &Rig) {
    let models: Vec<LiveModel> = ["m-a", "m-b", "m-c", "m-d", "m-e"]
        .iter()
        .map(|id| LiveModel {
            id: (*id).into(),
            ..LiveModel::default()
        })
        .collect();
    rig.catalog.store_live(Tool::Antigravity, &models);
}

#[test]
fn a_listing_that_drops_most_of_the_old_list_is_refused_unless_forced() {
    let rig = rig("shrink", true);
    five_agy_models(&rig);
    *rig.tools.agy.lock().unwrap() = Some("gemini-4-neon-high\tN\n".into());
    let report = call(&rig, ops::MODELS_UPDATE, json!({"skills": false}));
    let agy = part(&report, "antigravity");
    assert_eq!(agy["read"], false);
    assert!(agy["note"].as_str().unwrap().contains("--force"));
    assert_eq!(agy["written"], false);
    assert_eq!(rig.catalog.live_models(Tool::Antigravity).len(), 5);

    let forced = call(
        &rig,
        ops::MODELS_UPDATE,
        json!({"skills": false, "force": true}),
    );
    assert_eq!(part(&forced, "antigravity")["read"], true);
    assert_eq!(rig.catalog.live_models(Tool::Antigravity).len(), 1);
    assert!(rig
        .models
        .call(&app(), ops::MODELS_CHECK, &json!({"force": 3}))
        .is_err());
}

#[test]
fn a_stray_line_is_not_a_model_and_cannot_replace_the_cache() {
    let rig = rig("stray", true);
    for text in [
        "Unauthorized\n",
        "Error: please log in\n",
        "not signed in\n",
    ] {
        *rig.tools.agy.lock().unwrap() = Some(text.into());
        let report = call(&rig, ops::MODELS_UPDATE, json!({"skills": false}));
        let agy = part(&report, "antigravity");
        assert_eq!(agy["read"], false, "{text}");
        assert!(
            agy["note"].as_str().unwrap().contains("no models"),
            "{text}"
        );
        assert_eq!(rig.catalog.live_models(Tool::Antigravity), old_agy());
    }
}

#[test]
fn codex_is_not_asked_while_a_codex_session_is_live() {
    let mut rig = rig("busy", true);
    let busy = Arc::new(std::sync::atomic::AtomicBool::new(true));
    let flag = busy.clone();
    rig.models = ModelsOps::new(rig.catalog.clone(), None, rig.tools.sources()).with_codex_busy(
        Arc::new(move || flag.load(std::sync::atomic::Ordering::SeqCst)),
    );
    let report = call(&rig, ops::MODELS_UPDATE, json!({"skills": false}));
    let codex = part(&report, "codex");
    assert_eq!(codex["read"], false);
    assert!(codex["note"]
        .as_str()
        .unwrap()
        .contains("Codex session is running"));
    assert!(!rig.tmp.path("state/codex-models.json").exists());
    let calls = rig.tools.calls.lock().unwrap().clone();
    assert_eq!(calls, ["/fake/agy models"], "codex was never run");

    busy.store(false, std::sync::atomic::Ordering::SeqCst);
    let report = call(&rig, ops::MODELS_UPDATE, json!({"skills": false}));
    assert_eq!(part(&report, "codex")["read"], true);
}

#[test]
fn the_tools_are_found_again_on_every_update() {
    let rig = rig("discover", true);
    let found = Arc::new(Mutex::new(0usize));
    let (count, sources) = (found.clone(), rig.tools.sources());
    let models = ModelsOps::new(
        rig.catalog.clone(),
        None,
        Sources {
            codex: None,
            agy: None,
            capture: Arc::new(|_, _| None),
        },
    )
    .with_discovery(Arc::new(move || {
        *count.lock().unwrap() += 1;
        sources.clone()
    }));
    models
        .call(&app(), ops::MODELS_CHECK, &json!({"skills": false}))
        .unwrap();
    let report = models
        .call(&app(), ops::MODELS_CHECK, &json!({"skills": false}))
        .unwrap();
    assert_eq!(*found.lock().unwrap(), 2);
    assert_eq!(part(&report, "antigravity")["read"], true);
}

#[test]
fn the_first_read_of_a_list_says_so_instead_of_calling_everything_new() {
    let rig = rig("first", true);
    let fresh = TempDir::new("first-live");
    let catalog = Arc::new(
        CatalogService::new(
            Catalog::built_in(),
            LiveCache::new(fresh.path("state/live-models.json")),
            rig.tmp.path("codex"),
            None,
            Arc::new(PanicHttp),
        )
        .with_codex_snapshot(fresh.path("state/codex-models.json")),
    );
    let models = ModelsOps::new(catalog.clone(), None, rig.tools.sources());
    let report = models
        .call(&app(), ops::MODELS_UPDATE, &json!({"skills": false}))
        .unwrap();
    let agy = part(&report, "antigravity");
    assert_eq!(agy["first"], true);
    assert_eq!(agy["added"], json!([]));
    assert_eq!(agy["written"], true);
    assert_eq!(catalog.live_models(Tool::Antigravity).len(), 4);
    let text = report["text"].as_str().unwrap();
    assert!(
        text.contains("first read: nothing to compare with"),
        "{text}"
    );
    assert_eq!(report["changed"], true, "codex differs from the old cache");
    let second = models
        .call(&app(), ops::MODELS_UPDATE, &json!({"skills": false}))
        .unwrap();
    assert_eq!(part(&second, "antigravity")["first"], false);
}

#[test]
fn a_changed_codex_upgrade_hint_is_written_though_no_model_changed() {
    let rig = rig("hints", true);
    age(&rig.tmp.path("codex/models_cache.json"), 3_600);
    call(&rig, ops::MODELS_UPDATE, json!({"skills": false}));
    let plain = std::fs::read_to_string(rig.tmp.path("state/codex-models.json")).unwrap();
    assert!(!plain.contains("upgrade"));
    *rig.tools.codex.lock().unwrap() = Some(NEW_CODEX.replace(
        r#"{"slug":"gpt-6-astra","#,
        r#"{"slug":"gpt-6-astra","upgrade":{"model":"gpt-6.2-astra"},"#,
    ));
    let report = call(&rig, ops::MODELS_UPDATE, json!({"skills": false}));
    let codex = part(&report, "codex");
    assert_eq!(codex["written"], true);
    assert_eq!(codex["added"], json!([]));
    let kept = std::fs::read_to_string(rig.tmp.path("state/codex-models.json")).unwrap();
    assert!(kept.contains("gpt-6.2-astra") && kept.contains("upgrade"));
    let again = call(&rig, ops::MODELS_UPDATE, json!({"skills": false}));
    assert_eq!(part(&again, "codex")["written"], false);
}

#[test]
fn the_claude_path_uses_the_fake_token_at_anthropics_host_and_refuses_an_empty_list() {
    let (service, http, _keep) = crate::catalog_ops_tests::claude_catalog(
        "models-claude",
        r#"{"data":[{"id":"claude-test-1","display_name":"Test One"}]}"#,
    );
    let rig = rig("claude", true);
    let models = ModelsOps::new(service.clone(), None, rig.tools.sources());
    let report = models
        .call(&app(), ops::MODELS_UPDATE, &json!({"skills": false}))
        .unwrap();
    let claude = part(&report, "claude");
    assert_eq!(claude["read"], true);
    assert_eq!(claude["first"], true);
    assert_eq!(claude["written"], true);
    assert_eq!(service.live_models(Tool::Claude)[0].id, "claude-test-1");
    for (url, token) in http.seen() {
        if url.contains("/v1/models") {
            assert!(url.starts_with("https://api.anthropic.com/"), "{url}");
            assert!(token.is_some());
        }
    }
    let again = models
        .call(&app(), ops::MODELS_CHECK, &json!({"skills": false}))
        .unwrap();
    let note = part(&again, "claude")["note"].as_str().unwrap().to_string();
    assert!(note.contains("less than a minute"), "{note}");
    assert_eq!(service.live_models(Tool::Claude)[0].id, "claude-test-1");
}
