use super::*;
use crate::acting_ops::Room;
use crate::embed_ops::{EmbedOps, EmbedParts, EmbedSettings, PluggedIn};
use crate::limits_ops_tests::TempDir;
use crate::op_group::OpGroup;
use notchling_engine::search::chunk::{ChunkKind, ChunkText};
use notchling_engine::search::corpus::{Corpus, Doc, DocStamp, Tier as CorpusTier};
use notchling_engine::search::embed::helper_fake::FakeSpawn;
use notchling_engine::search::embed::{Embedder, HashEmbedder, ModelId};
use notchling_engine::search::hybrid::FLOOR_GRANITE;
use notchling_local_api::{Caller, ClientKind, Tier};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

pub(super) const GRANITE: &str = "granite-fake@1/p1";

pub(super) struct Quiet;

impl Room for Quiet {
    fn publish(&self, _topic: &str, _event: &str, _data: &Value) {}

    fn can_answer(&self) -> bool {
        true
    }

    fn can_follow(&self, _topic: &str) -> bool {
        true
    }
}

struct Notes {
    docs: Vec<(String, String)>,
}

impl Corpus for Notes {
    fn id(&self) -> &'static str {
        "sessions"
    }

    fn tier(&self) -> CorpusTier {
        CorpusTier::Agent
    }

    fn scan(&self, _window: Option<f64>, _now: f64) -> Vec<DocStamp> {
        self.docs
            .iter()
            .map(|(k, _)| DocStamp {
                key: k.clone(),
                stamp: "1".to_string(),
                at: now(),
            })
            .collect()
    }

    fn read(&self, doc: &DocStamp, _now: f64) -> Option<Doc> {
        let text = &self.docs.iter().find(|(k, _)| *k == doc.key)?.1;
        Some(Doc {
            key: doc.key.clone(),
            title: doc.key.clone(),
            at: doc.at,
            facets: vec![("tool".to_string(), "claude".to_string())],
            chunks: vec![ChunkText {
                kind: ChunkKind::Head,
                text: text.clone(),
            }],
        })
    }
}

pub(super) fn now() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

pub(super) fn app() -> Caller {
    Caller {
        tier: Tier::App,
        client: ClientKind::Cli,
        peer: None,
        pid: None,
    }
}

pub(super) struct Rig {
    pub(super) dir: TempDir,
    pub(super) ops: PagesOps,
    pub(super) embed: Arc<EmbedOps>,
    pub(super) on: Arc<Mutex<EmbedSettings>>,
    pub(super) search: Arc<Mutex<SearchSettings>>,
}

pub(super) type Make = Box<dyn Fn(&TempDir) -> Vec<Arc<dyn Corpus>>>;

fn settings(on: bool) -> EmbedSettings {
    EmbedSettings {
        on,
        paused: false,
        models: Vec::new(),
        range: IndexRange::All,
        categories: vec![Category::Sessions, Category::Settings],
    }
}

pub(super) fn session(dir: &TempDir, folder: &str, name: &str, text: &str, ago: f64) {
    let path = dir.path(&format!("claude/projects/-scope/{name}.jsonl"));
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(
        &path,
        format!(
            "{}\n",
            json!({"type": "user", "cwd": dir.path(&format!("{folder}/app")),
                "message": {"role": "user", "content": text}})
        ),
    )
    .unwrap();
    let file = std::fs::OpenOptions::new().write(true).open(&path).unwrap();
    file.set_modified(UNIX_EPOCH + std::time::Duration::from_secs_f64(now() - ago))
        .unwrap();
}

pub(super) fn build(
    tag: &str,
    sessions: &[(&str, &str, &str, f64)],
    vectors: Vec<(String, String)>,
    on: bool,
) -> Rig {
    build_with(
        tag,
        sessions,
        Box::new(move |_| {
            vec![Arc::new(Notes {
                docs: vectors.clone(),
            }) as Arc<dyn Corpus>]
        }),
        on,
    )
}

pub(super) fn build_with(
    tag: &str,
    sessions: &[(&str, &str, &str, f64)],
    make: Make,
    on: bool,
) -> Rig {
    let dir = TempDir::new(tag);
    for (folder, name, text, ago) in sessions {
        session(&dir, folder, name, text, *ago);
    }
    let current = Arc::new(Mutex::new(settings(on)));
    let read = current.clone();
    let corpora = make(&dir);
    let corpora: Arc<Mutex<Vec<Arc<dyn Corpus>>>> = Arc::new(Mutex::new(corpora));
    let search = Arc::new(Mutex::new(SearchSettings::default()));
    let search_read = search.clone();
    let embed = EmbedOps::with_parts(EmbedParts {
        data_dir: dir.path("data"),
        room: Arc::new(Quiet),
        settings: Arc::new(move || read.lock().unwrap().clone()),
        corpora: Arc::new(move |_| corpora.lock().unwrap().clone()),
        spawn: Arc::new(FakeSpawn::new(&ModelId(GRANITE.to_string()))),
        granite_id: ModelId(GRANITE.to_string()),
        granite_present: false,
        statics: Arc::new(|| {
            Ok(Some(
                Arc::new(HashEmbedder::new("static-fake@1/p1")) as Arc<dyn Embedder>
            ))
        }),
        fixture: true,
        power: Arc::new(PluggedIn),
        net: crate::embed_fetch::ModelNet::offline(),
        now: Arc::new(now),
    });
    let ops = PagesOps::new(PagesSetup {
        pages_dir: dir.path("data/pages"),
        writable: false,
        roots: Roots {
            main_dir: dir.path("claude"),
            lanes_file: dir.path("data/lanes.json"),
        },
        codex_sessions: dir.path("codex/sessions"),
        cache: Some(dir.path("data/daemon/search.db")),
    })
    .with_search_settings(Arc::new(move || search_read.lock().unwrap().clone()));
    ops.bind_embed(embed.clone());
    for _ in 0..4 {
        embed.tick();
    }
    Rig {
        dir,
        ops,
        embed,
        on: current,
        search,
    }
}

fn two() -> Rig {
    build(
        "two",
        &[
            (
                "work",
                "c1",
                "fix the token refresh in the login flow",
                60.0,
            ),
            ("work", "c2", "tidy the quarterly spreadsheet", 120.0),
        ],
        vec![
            (
                "claude:c1".into(),
                "fix the token refresh in the login flow".into(),
            ),
            (
                "claude:c2".into(),
                "invoice payment ledger reconciliation for the quarter".into(),
            ),
        ],
        true,
    )
}

pub(super) fn keys(reply: &Value) -> Vec<String> {
    reply["sessions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["key"].as_str().unwrap().to_string())
        .collect()
}

fn unscored(reply: &Value) -> Value {
    let mut rows = reply["sessions"].clone();
    for row in rows.as_array_mut().unwrap() {
        row.as_object_mut().unwrap().remove("score");
    }
    rows
}

pub(super) fn call(rig: &Rig, args: Value) -> Value {
    rig.ops.call(ops::SEARCH_QUERY, &args).unwrap()
}

#[test]
fn no_mode_answers_as_before() {
    let rig = two();
    let plain = call(&rig, json!({"text": "token"}));
    let text = call(&rig, json!({"text": "token", "mode": "text"}));
    assert_eq!(unscored(&plain), unscored(&text));
    assert_eq!(plain["pages"], text["pages"]);
    assert!(plain.get("semantic").is_none());
    assert!(plain["sessions"][0].get("match").is_none());
    assert_eq!(keys(&plain), ["claude:c1"]);
}

#[test]
fn hybrid_tags_matches() {
    let rig = two();
    let reply = call(
        &rig,
        json!({"text": "invoice payment ledger reconciliation", "mode": "hybrid"}),
    );
    assert_eq!(reply["semantic"]["state"], "ready");
    let found = reply["sessions"].as_array().unwrap();
    let mine = found.iter().find(|s| s["key"] == "claude:c2").unwrap();
    assert_eq!(mine["match"], "semantic");
    assert!(mine["highlights"].as_array().unwrap().is_empty());
    let snippet = mine["snippet"].as_str().unwrap();
    assert!(snippet.starts_with("invoice payment"), "{snippet}");
    assert!(snippet.chars().count() <= 200);

    let both = call(
        &rig,
        json!({"text": "token refresh login", "mode": "hybrid"}),
    );
    let first = &both["sessions"][0];
    assert_eq!(first["key"], "claude:c1");
    assert_eq!(first["match"], "both");
    assert!(!first["highlights"].as_array().unwrap().is_empty());
}

#[test]
fn a_fixture_granite_hit_under_the_calibrated_floor_is_kept() {
    let rig = two();
    let query = "invoice ledger";
    let fake = HashEmbedder::new(GRANITE);
    let v = fake
        .embed(&[
            query,
            "invoice payment ledger reconciliation for the quarter",
        ])
        .unwrap();
    let cosine: f32 = v[0].iter().zip(&v[1]).map(|(a, b)| a * b).sum();
    assert!(cosine > 0.0 && cosine < FLOOR_GRANITE, "{cosine}");
    let (state, lists) = rig
        .embed
        .query_lists(query, &Filter::default(), &["sessions"]);
    assert_eq!(state, Semantic::Ready);
    let granite = lists["sessions"].granite.clone().unwrap_or_default();
    let hit = granite.iter().find(|h| h.key == "claude:c2");
    assert!(hit.is_some_and(|h| h.score < FLOOR_GRANITE), "{granite:?}");
    let fused = fuse(&lists["sessions"], 10, &|_| None);
    assert!(fused.iter().any(|r| r.key == "claude:c2"), "{fused:?}");
    let reply = call(&rig, json!({"text": query, "mode": "hybrid"}));
    assert!(keys(&reply).contains(&"claude:c2".to_string()), "{reply}");
}

#[test]
fn semantic_mode_returns_only_vector_hits() {
    let rig = two();
    let reply = call(
        &rig,
        json!({"text": "token refresh login", "mode": "semantic", "limit": 1}),
    );
    assert_eq!(keys(&reply), ["claude:c1"]);
    assert_eq!(reply["sessions"][0]["match"], "semantic");
}

#[test]
fn off_or_failing_falls_back_to_text_and_says_so() {
    let rig = two();
    rig.on.lock().unwrap().on = false;
    rig.embed.tick();
    let reply = call(&rig, json!({"text": "token", "mode": "hybrid"}));
    assert_eq!(keys(&reply), ["claude:c1"]);
    assert_eq!(reply["semantic"]["state"], "off");
    assert_eq!(reply["sessions"][0]["match"], "text");
    let only = call(&rig, json!({"text": "token", "mode": "semantic"}));
    assert!(keys(&only).is_empty());
    assert_eq!(only["semantic"]["state"], "off");

    let bare = PagesOps::new(PagesSetup {
        pages_dir: rig.dir.path("data/pages"),
        writable: false,
        roots: Roots {
            main_dir: rig.dir.path("claude"),
            lanes_file: rig.dir.path("data/lanes.json"),
        },
        codex_sessions: rig.dir.path("codex/sessions"),
        cache: None,
    });
    let reply = bare
        .call(
            ops::SEARCH_QUERY,
            &json!({"text": "token", "mode": "hybrid"}),
        )
        .unwrap();
    assert_eq!(reply["semantic"]["state"], "off");
    assert_eq!(reply["sessions"][0]["match"], "text");
}

#[test]
fn an_unknown_mode_is_refused() {
    let rig = two();
    let err = rig
        .ops
        .call(ops::SEARCH_QUERY, &json!({"text": "x", "mode": "magic"}))
        .unwrap_err();
    assert!(err.message.contains("mode"), "{}", err.message);
}

#[test]
fn semantic_settings_lists_for_the_app() {
    let rig = two();
    rig.embed
        .call(
            &app(),
            ops::EMBED_SETTINGS,
            &json!({"docs": [
                {"id": "theme", "text": "Theme. Dark mode and colors of the appearance"},
                {"id": "awake", "text": "Keep the Mac awake while a session runs"},
            ]}),
        )
        .unwrap();
    for _ in 0..3 {
        rig.embed.tick();
    }
    let reply = call(
        &rig,
        json!({"text": "dark appearance colors", "mode": "semantic", "corpora": ["settings"]}),
    );
    let list = reply["settings"].as_array().unwrap();
    assert_eq!(list[0]["id"], "theme");
    assert!(list[0]["score"].as_f64().is_some());
    assert!(reply["dropped"].as_array().unwrap().is_empty());
    let text = call(&rig, json!({"text": "dark", "corpora": ["settings"]}));
    assert_eq!(text["dropped"], json!(["settings"]));
    assert!(text.get("settings").is_none());
}

#[test]
fn a_hybrid_reply_stays_under_100_kb() {
    let long = "meaning of life and many words about ledgers ".repeat(40);
    let names: Vec<String> = (0..200).map(|i| format!("s{i}")).collect();
    let texts: Vec<String> = names.iter().map(|n| format!("{n} {long}")).collect();
    let rows: Vec<(&str, &str, &str, f64)> = names
        .iter()
        .zip(&texts)
        .map(|(n, t)| ("work", n.as_str(), t.as_str(), 60.0))
        .collect();
    let vectors = names
        .iter()
        .zip(&texts)
        .map(|(n, t)| (format!("claude:{n}"), t.clone()))
        .collect();
    let rig = build("big", &rows, vectors, true);
    for args in [
        json!({"text": "ledgers meaning of life", "mode": "hybrid"}),
        json!({"text": "ledgers meaning of life", "mode": "hybrid", "limit": 200}),
        json!({"text": "ledgers meaning of life", "mode": "semantic", "limit": 200}),
    ] {
        let reply = call(&rig, args.clone());
        assert!(!reply["sessions"].as_array().unwrap().is_empty());
        let size = reply.to_string().len();
        assert!(size < 100_000, "{args} {size}");
        for s in reply["sessions"].as_array().unwrap() {
            assert!(s["snippet"]
                .as_str()
                .is_none_or(|t| t.chars().count() <= 260));
        }
    }
}

pub(super) struct Folders;

impl Placing for Folders {
    fn org_of_folder(&self, folder: &str) -> String {
        if folder.contains("/other/") {
            "other".into()
        } else {
            "work".into()
        }
    }

    fn claimed_by(&self, _path: &str) -> Option<String> {
        None
    }

    fn only_personal(&self) -> bool {
        false
    }
}

#[test]
fn hybrid_hits_keep_the_organization_and_window() {
    let rig = build(
        "scope",
        &[
            ("work", "w1", "tidy the quarterly spreadsheet", 60.0),
            ("other", "o1", "tidy the quarterly spreadsheet", 60.0),
            (
                "work",
                "old",
                "tidy the quarterly spreadsheet",
                20.0 * 86400.0,
            ),
        ],
        vec![
            (
                "claude:w1".into(),
                "invoice payment ledger reconciliation".into(),
            ),
            (
                "claude:o1".into(),
                "invoice payment ledger reconciliation".into(),
            ),
            (
                "claude:old".into(),
                "invoice payment ledger reconciliation".into(),
            ),
        ],
        true,
    );
    let reply = rig
        .ops
        .call_scoped(
            ops::SEARCH_QUERY,
            &json!({"text": "invoice payment ledger", "mode": "hybrid"}),
            "work",
            &Folders,
        )
        .unwrap();
    assert_eq!(keys(&reply), ["claude:w1"]);
    assert_eq!(reply["masked"], true);
}

#[test]
fn a_secret_in_a_meaning_snippet_is_masked() {
    let secret = "invoice payment ledger sk-ant-api03-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";
    let rig = build(
        "mask",
        &[("work", "w1", "tidy the quarterly spreadsheet", 60.0)],
        vec![("claude:w1".into(), secret.into())],
        true,
    );
    let reply = rig
        .ops
        .call_scoped(
            ops::SEARCH_QUERY,
            &json!({"text": "invoice payment ledger", "mode": "hybrid"}),
            "work",
            &Folders,
        )
        .unwrap();
    let snippet = reply["sessions"][0]["snippet"].as_str().unwrap();
    assert!(!snippet.contains("AAAAAAAA"), "{snippet}");
}
