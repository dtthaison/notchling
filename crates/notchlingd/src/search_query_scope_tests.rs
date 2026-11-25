use super::tests::{app, build, build_with, call, keys, Folders, Rig};
use super::*;
use crate::limits_ops_tests::TempDir;
use crate::op_group::OpGroup;
use notchling_engine::pages::PageLibrary;
use notchling_engine::search::chunk::{ChunkKind, ChunkText};
use notchling_engine::search::corpus::{Corpus, Doc, DocStamp, PagesCorpus, Tier as CorpusTier};
use std::sync::Arc;

const QUERY: &str = "invoice payment ledger reconciliation for the quarter";

type PageRow = (String, String, String, String);

fn page_row(folder: &str, title: &str, text: &str, cwd: &str) -> PageRow {
    (folder.into(), title.into(), text.into(), cwd.into())
}

fn write_pages(dir: &TempDir, pages: &[PageRow]) {
    let mut list = Vec::new();
    for (folder, title, text, cwd) in pages {
        dir.write(
            &format!("data/pages/{folder}/index.html"),
            &format!("<html><head><title>{title}</title></head><body><p>{text}</p></body></html>"),
        );
        list.push(json!({
            "folder": folder,
            "file": "index.html",
            "title": title,
            "url": format!("https://claude.ai/artifact/{folder}"),
            "source": "",
            "cwd": cwd,
            "project": "app",
            "account": "",
            "updated": super::tests::now() - 60.0,
            "version": "v1",
        }));
    }
    dir.write("data/pages/pages.json", &json!({"pages": list}).to_string());
}

fn with_pages(tag: &str, pages: Vec<PageRow>) -> Rig {
    build_with(
        tag,
        &[],
        Box::new(move |dir| {
            write_pages(dir, &pages);
            vec![Arc::new(PagesCorpus::new(PageLibrary::read_only(
                dir.path("data/pages"),
            ))) as Arc<dyn Corpus>]
        }),
        true,
    )
}

fn two_pages() -> Vec<PageRow> {
    vec![
        page_row(
            "p1",
            "Launch plan",
            "We ship the search page on Friday after the review",
            "/x/work/app",
        ),
        page_row("p2", "Budget", QUERY, "/x/work/app"),
    ]
}

fn titles(reply: &Value) -> Vec<String> {
    reply["pages"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["title"].as_str().unwrap().to_string())
        .collect()
}

#[test]
fn semantic_page_hits_are_returned() {
    let rig = with_pages("pages-semantic", two_pages());
    let reply = call(
        &rig,
        json!({"text": QUERY, "mode": "semantic", "corpora": ["pages"]}),
    );
    assert_eq!(reply["semantic"]["state"], "ready");
    assert_eq!(
        titles(&reply).first().map(String::as_str),
        Some("Budget"),
        "{reply}"
    );
    assert_eq!(reply["pages"][0]["match"], "semantic");
}

#[test]
fn a_page_found_by_text_and_meaning_is_one_row_marked_both() {
    let rig = with_pages("pages-both", two_pages());
    let reply = call(
        &rig,
        json!({"text": "launch plan", "mode": "hybrid", "corpora": ["pages"]}),
    );
    let rows: Vec<&Value> = reply["pages"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|p| p["title"] == "Launch plan")
        .collect();
    assert_eq!(rows.len(), 1, "{reply}");
    assert_eq!(rows[0]["match"], "both", "{reply}");
}

#[test]
fn pages_of_another_organization_do_not_take_the_limit() {
    let mut pages: Vec<PageRow> = (0..130)
        .map(|i| {
            page_row(
                &format!("t{i}"),
                &format!("Theirs {i}"),
                QUERY,
                "/x/other/app",
            )
        })
        .collect();
    pages.push(page_row("ours", "Ours", "invoice", "/x/work/app"));
    let rig = with_pages("pages-scope", pages);
    let reply = rig
        .ops
        .call_scoped(
            ops::SEARCH_QUERY,
            &json!({"text": QUERY, "mode": "semantic", "corpora": ["pages"]}),
            "work",
            &Folders,
        )
        .unwrap();
    assert_eq!(titles(&reply), ["Ours"], "{reply}");
}

#[test]
fn sessions_of_another_organization_do_not_take_the_limit() {
    let rig = build(
        "sessions-scope",
        &[
            ("work", "w1", "tidy the quarterly spreadsheet", 60.0),
            ("other", "o1", "tidy the quarterly spreadsheet", 60.0),
        ],
        vec![
            ("claude:o1".into(), QUERY.into()),
            ("claude:w1".into(), "invoice payment".into()),
        ],
        true,
    );
    let reply = rig
        .ops
        .call_scoped(
            ops::SEARCH_QUERY,
            &json!({"text": QUERY, "mode": "semantic", "limit": 1}),
            "work",
            &Folders,
        )
        .unwrap();
    assert_eq!(keys(&reply), ["claude:w1"], "{reply}");
}

#[test]
fn sessions_older_than_since_do_not_take_the_limit() {
    let rig = build(
        "sessions-since",
        &[
            (
                "work",
                "old",
                "tidy the quarterly spreadsheet",
                10.0 * 3600.0,
            ),
            ("work", "new", "tidy the quarterly spreadsheet", 60.0),
        ],
        vec![
            ("claude:old".into(), QUERY.into()),
            ("claude:new".into(), "invoice".into()),
        ],
        true,
    );
    let all = call(&rig, json!({"text": QUERY, "mode": "semantic", "limit": 2}));
    assert_eq!(keys(&all), ["claude:old", "claude:new"], "{all}");
    let reply = call(
        &rig,
        json!({"text": QUERY, "mode": "semantic", "limit": 1, "since": "5h"}),
    );
    assert_eq!(keys(&reply), ["claude:new"], "{reply}");
}

fn docs() -> Value {
    json!({"docs": [
        {"id": "theme", "text": "Theme. Dark mode and colors of the appearance"},
        {"id": "awake", "text": "Keep the Mac awake while a session runs"},
    ]})
}

fn settings_rig() -> Rig {
    let rig = build(
        "settings-scope",
        &[("work", "w1", "tidy the quarterly spreadsheet", 60.0)],
        vec![("claude:w1".into(), QUERY.into())],
        true,
    );
    rig.embed
        .call(&app(), ops::EMBED_SETTINGS, &docs())
        .unwrap();
    for _ in 0..3 {
        rig.embed.tick();
    }
    rig
}

const ASK: &str =
    r#"{"text": "dark appearance colors", "mode": "semantic", "corpora": ["settings"]}"#;

#[test]
fn an_agent_cannot_search_the_settings_corpus() {
    let rig = settings_rig();
    let args: Value = serde_json::from_str(ASK).unwrap();
    let app = call(&rig, args.clone());
    assert_eq!(app["settings"][0]["id"], "theme");
    let agent = rig
        .ops
        .call_scoped(ops::SEARCH_QUERY, &args, "work", &Folders)
        .unwrap();
    assert!(agent.get("settings").is_none(), "{agent}");
    assert_eq!(agent["dropped"], json!(["settings"]));
}

#[test]
fn settings_off_is_disabled_and_leaves_the_index() {
    let rig = settings_rig();
    let args: Value = serde_json::from_str(ASK).unwrap();
    rig.search.lock().unwrap().on = vec![Category::Sessions, Category::Pages];
    let off = call(&rig, args.clone());
    assert!(off.get("settings").is_none(), "{off}");
    assert_eq!(off["disabled"], json!(["settings"]));
    assert_eq!(off["dropped"], json!([]));

    let held = |rig: &Rig| {
        let (_, lists) =
            rig.embed
                .query_lists("dark appearance colors", &Filter::default(), &["settings"]);
        let l = &lists["settings"];
        l.granite
            .iter()
            .chain(l.statics.iter())
            .any(|v| !v.is_empty())
    };
    assert!(held(&rig));
    rig.on.lock().unwrap().categories = vec![Category::Sessions];
    for _ in 0..3 {
        rig.embed.tick();
    }
    assert!(!held(&rig));
    rig.on.lock().unwrap().categories = vec![Category::Sessions, Category::Settings];
    for _ in 0..3 {
        rig.embed.tick();
    }
    assert!(held(&rig));
}

fn crowd(prefix: &str, folder: &str, n: usize, ago: f64) -> Vec<(String, String, String, f64)> {
    (0..n)
        .map(|i| {
            (
                folder.to_string(),
                format!("{prefix}{i}"),
                "tidy the quarterly spreadsheet".to_string(),
                ago,
            )
        })
        .collect()
}

fn crowded(
    tag: &str,
    theirs: Vec<(String, String, String, f64)>,
    ours: (&str, &str, &str, f64),
) -> Rig {
    let mut vectors: Vec<(String, String)> = theirs
        .iter()
        .map(|(_, name, _, _)| (format!("claude:{name}"), QUERY.to_string()))
        .collect();
    vectors.push((format!("claude:{}", ours.1), "invoice payment".to_string()));
    let mut all: Vec<(&str, &str, &str, f64)> = theirs
        .iter()
        .map(|(f, n, t, a)| (f.as_str(), n.as_str(), t.as_str(), *a))
        .collect();
    all.push(ours);
    build(tag, &all, vectors, true)
}

#[test]
fn many_sessions_of_another_organization_do_not_fill_the_semantic_cut() {
    let rig = crowded(
        "sessions-scope-many",
        crowd("o", "other", 130, 60.0),
        ("work", "w1", "tidy the quarterly spreadsheet", 60.0),
    );
    let reply = rig
        .ops
        .call_scoped(
            ops::SEARCH_QUERY,
            &json!({"text": QUERY, "mode": "semantic", "corpora": ["sessions"]}),
            "work",
            &Folders,
        )
        .unwrap();
    assert_eq!(keys(&reply), ["claude:w1"], "{reply}");
}

#[test]
fn many_sessions_older_than_since_do_not_fill_the_semantic_cut() {
    let rig = crowded(
        "sessions-since-many",
        crowd("old", "work", 130, 10.0 * 3600.0),
        ("work", "new", "tidy the quarterly spreadsheet", 60.0),
    );
    let reply = call(
        &rig,
        json!({"text": QUERY, "mode": "semantic", "since": "5h", "corpora": ["sessions"]}),
    );
    assert_eq!(keys(&reply), ["claude:new"], "{reply}");
}

fn gone_sessions(n: usize) -> Vec<(String, String)> {
    (0..n)
        .map(|i| (format!("claude:gone{i}"), QUERY.to_string()))
        .collect()
}

#[test]
fn sessions_the_index_dropped_do_not_fill_the_semantic_cut() {
    let mut vectors = gone_sessions(130);
    vectors.push(("claude:w1".to_string(), "invoice payment".to_string()));
    let rig = build(
        "sessions-gone",
        &[("work", "w1", "tidy the quarterly spreadsheet", 60.0)],
        vectors,
        true,
    );
    for mode in ["semantic", "hybrid"] {
        let reply = call(
            &rig,
            json!({"text": QUERY, "mode": mode, "corpora": ["sessions"]}),
        );
        assert_eq!(keys(&reply), ["claude:w1"], "{mode}: {reply}");
    }
}

struct Lagging {
    key: String,
}

impl Corpus for Lagging {
    fn id(&self) -> &'static str {
        "sessions"
    }

    fn tier(&self) -> CorpusTier {
        CorpusTier::Agent
    }

    fn scan(&self, _window: Option<f64>, _now: f64) -> Vec<DocStamp> {
        vec![DocStamp {
            key: self.key.clone(),
            stamp: "1".to_string(),
            at: super::tests::now(),
        }]
    }

    fn read(&self, doc: &DocStamp, _now: f64) -> Option<Doc> {
        Some(Doc {
            key: doc.key.clone(),
            title: doc.key.clone(),
            at: doc.at,
            facets: vec![("tool".to_string(), "codex".to_string())],
            chunks: vec![ChunkText {
                kind: ChunkKind::Head,
                text: QUERY.to_string(),
            }],
        })
    }
}

#[test]
fn a_semantic_session_row_still_matches_the_filter_on_its_live_facets() {
    let rig = build_with(
        "sessions-lagging",
        &[("work", "w1", "tidy the quarterly spreadsheet", 60.0)],
        Box::new(|_| {
            vec![Arc::new(Lagging {
                key: "claude:w1".into(),
            }) as Arc<dyn Corpus>]
        }),
        true,
    );
    let ask = |tool: &str| {
        call(
            &rig,
            json!({"text": QUERY, "mode": "semantic", "corpora": ["sessions"],
                "filter": {"tool": [tool]}}),
        )
    };
    assert_eq!(keys(&ask("claude")), Vec::<String>::new());
    assert_eq!(keys(&ask("codex")), Vec::<String>::new());
}
