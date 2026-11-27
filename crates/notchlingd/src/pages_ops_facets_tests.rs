use crate::pages_ops::*;
use crate::search_scope::Placing;
use notchling_engine::pages::{PageLibrary, RecordContext};
use notchling_engine::sessions::Roots;
use notchling_local_api::{ops, ErrorCode, OpSpec};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

struct Dir(PathBuf);

impl Dir {
    fn new(tag: &str) -> Dir {
        let dir = std::env::temp_dir().join(format!("nl-facets-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Dir(dir)
    }

    fn path(&self, rel: &str) -> PathBuf {
        self.0.join(rel)
    }

    fn write(&self, rel: &str, text: &str) -> PathBuf {
        let path = self.path(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, text).unwrap();
        path
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn now() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

fn age(path: &Path, ago: f64) {
    let file = std::fs::OpenOptions::new().write(true).open(path).unwrap();
    file.set_modified(UNIX_EPOCH + Duration::from_secs_f64(now() - ago))
        .unwrap();
}

fn session(dir: &Dir, name: &str, cwd: &str, word: &str, ago: f64) {
    let line = format!(
        r#"{{"type":"user","cwd":"{cwd}","timestamp":"2026-11-10T10:00:00.000Z","message":{{"role":"user","content":"talk about {word}"}}}}"#
    );
    let slug = cwd.replace('/', "-");
    let path = dir.write(
        &format!("claude/projects/{slug}/{name}.jsonl"),
        &format!("{line}\n"),
    );
    age(&path, ago);
}

fn page(dir: &Dir, file: &str, url: &str, account: &str) {
    page_titled(dir, file, url, account, "Plan");
}

fn page_titled(dir: &Dir, file: &str, url: &str, account: &str, title: &str) {
    dir.write(
        &format!("work/{file}"),
        &format!("<html><title>{title}</title><body> zebrafish plan </body></html>"),
    );
    let event = json!({
        "tool_name": "Artifact",
        "cwd": dir.path("work"),
        "tool_input": {"file_path": file},
        "tool_response": format!("Published {url}"),
    });
    PageLibrary::writable(dir.path("data/pages"))
        .record(
            &event,
            &RecordContext {
                account,
                now: now() - 60.0,
                home: &dir.0,
            },
        )
        .unwrap()
        .unwrap();
}

fn rig(dir: &Dir) -> PagesOps {
    PagesOps::new(PagesSetup {
        pages_dir: dir.path("data/pages"),
        writable: false,
        roots: Roots {
            main_dir: dir.path("claude"),
            lanes_file: dir.path("data/lanes.json"),
        },
        codex_sessions: dir.path("codex/sessions"),
        cache: Some(dir.path("data/daemon/search.db")),
    })
    .with_accounts(Arc::new(|_, _| "work".to_string()))
}

const A: &str = "https://claude.ai/artifact/AbCdEfGhIjKlMnOpQrStUv";
const B: &str = "https://claude.ai/code/artifact/ZZZZZZZZZZZZZZZZZZZZZZ";

fn fixture(tag: &str) -> (Dir, PagesOps) {
    let dir = Dir::new(tag);
    session(&dir, "s1", "/w/app", "zebrafish", 120.0);
    session(&dir, "s2", "/w/site", "zebrafish", 130.0);
    page(&dir, "a.html", A, "alice@example.com");
    page(&dir, "b.html", B, "bob@example.com");
    let ops = rig(&dir);
    ops.poke_and_wait();
    (dir, ops)
}

fn ids(reply: &Value) -> Vec<String> {
    reply["sessions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["session_id"].as_str().unwrap().to_string())
        .collect()
}

fn values(reply: &Value) -> Vec<(String, String, u64)> {
    reply["values"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| {
            (
                v["value"].as_str().unwrap().to_string(),
                v["label"].as_str().unwrap().to_string(),
                v["count"].as_u64().unwrap(),
            )
        })
        .collect()
}

struct Split;

impl Placing for Split {
    fn org_of_folder(&self, folder: &str) -> String {
        if folder == "/w/site" {
            "client"
        } else {
            "personal"
        }
        .into()
    }

    fn claimed_by(&self, _: &str) -> Option<String> {
        None
    }

    fn only_personal(&self) -> bool {
        false
    }
}

#[test]
fn a_filtered_query_returns_only_matching_docs() {
    let (_dir, ops) = fixture("filtered");
    let all = ops
        .call(ops::SEARCH_QUERY, &json!({"text": "zebrafish"}))
        .unwrap();
    assert_eq!(ids(&all).len(), 2);
    let app = ops
        .call(
            ops::SEARCH_QUERY,
            &json!({"text": "zebrafish", "filter": {"project": ["/w/app"]}}),
        )
        .unwrap();
    assert_eq!(ids(&app), ["s1"]);
    let codex = ops
        .call(
            ops::SEARCH_QUERY,
            &json!({"text": "zebrafish", "filter": {"tool": ["codex"]}}),
        )
        .unwrap();
    assert!(ids(&codex).is_empty());
    let alice = ops
        .call(
            ops::SEARCH_QUERY,
            &json!({"text": "zebrafish", "filter": {"owner": ["alice@example.com"]}}),
        )
        .unwrap();
    assert_eq!(alice["pages"].as_array().unwrap().len(), 1);
    assert!(ids(&alice).is_empty());
    let future = ops
        .call(
            ops::SEARCH_QUERY,
            &json!({"text": "zebrafish", "filter": {"after": now() + 10.0}}),
        )
        .unwrap();
    assert!(ids(&future).is_empty());
    assert!(future["pages"].as_array().unwrap().is_empty());
}

#[test]
fn a_filter_with_an_empty_query_lists_the_matching_latest() {
    let (_dir, ops) = fixture("latest");
    let reply = ops
        .call(
            ops::SEARCH_QUERY,
            &json!({"filter": {"project": ["/w/site"]}}),
        )
        .unwrap();
    assert_eq!(ids(&reply), ["s2"]);
}

#[test]
fn hits_carry_their_facets() {
    let (_dir, ops) = fixture("hits");
    let reply = ops
        .call(
            ops::SEARCH_QUERY,
            &json!({"text": "zebrafish", "filter": {"project": ["/w/app"]}}),
        )
        .unwrap();
    assert_eq!(
        reply["sessions"][0]["facets"],
        json!({"tool": "claude", "project": "/w/app", "account": "work"})
    );
    let pages = ops
        .call(ops::SEARCH_QUERY, &json!({"text": "zebrafish"}))
        .unwrap();
    let first = &pages["pages"][0]["facets"];
    assert!(first["owner"].as_str().unwrap().ends_with("@example.com"));
    assert!(first.get("project").is_some());
}

#[test]
fn a_bad_filter_is_a_bad_request_naming_the_field() {
    let (_dir, ops) = fixture("bad");
    let err = ops
        .call(
            ops::SEARCH_QUERY,
            &json!({"text": "zebrafish", "filter": {"tool": "codex"}}),
        )
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::BadRequest);
    assert!(err.message.contains("filter.tool"), "{}", err.message);
}

#[test]
fn facets_follow_the_corpus_tier() {
    let (_dir, ops) = fixture("tier");
    assert_eq!(ops.spec(ops::SEARCH_FACETS), Some(OpSpec::AGENT_READ));
    let vault = ops
        .call(
            ops::SEARCH_FACETS,
            &json!({"corpus": "vault", "name": "project"}),
        )
        .unwrap();
    assert_eq!(vault["values"], json!([]));
    assert_eq!(vault["dropped"], json!(["vault"]));
    let sessions = ops
        .call(
            ops::SEARCH_FACETS,
            &json!({"corpus": "sessions", "name": "tool"}),
        )
        .unwrap();
    assert_eq!(values(&sessions), [("claude".into(), "claude".into(), 2)]);
    let lacking = ops
        .call(
            ops::SEARCH_FACETS,
            &json!({"corpus": "sessions", "name": "owner"}),
        )
        .unwrap();
    assert!(values(&lacking).is_empty());
    let err = ops
        .call(
            ops::SEARCH_FACETS,
            &json!({"corpus": "sessions", "name": "color"}),
        )
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::BadRequest);
}

#[test]
fn facets_labels_are_last_path_parts() {
    let (_dir, ops) = fixture("labels");
    let reply = ops
        .call(
            ops::SEARCH_FACETS,
            &json!({"corpus": "sessions", "name": "project", "prefix": "S", "limit": 5}),
        )
        .unwrap();
    assert_eq!(values(&reply), [("/w/site".into(), "site".into(), 1)]);
    let limited = ops
        .call(
            ops::SEARCH_FACETS,
            &json!({"corpus": "sessions", "name": "project", "limit": 1}),
        )
        .unwrap();
    assert_eq!(values(&limited).len(), 1);
    let pages = ops
        .call(
            ops::SEARCH_FACETS,
            &json!({"corpus": "pages", "name": "owner"}),
        )
        .unwrap();
    assert_eq!(values(&pages).len(), 2);
}

#[test]
fn an_agent_sees_only_its_organizations_values_and_masked_accounts() {
    let (_dir, ops) = fixture("scoped");
    let reply = ops
        .call_scoped(
            ops::SEARCH_FACETS,
            &json!({"corpus": "sessions", "name": "project"}),
            "personal",
            &Split,
        )
        .unwrap();
    assert_eq!(values(&reply), [("/w/app".into(), "app".into(), 1)]);
    let owners = ops
        .call_scoped(
            ops::SEARCH_FACETS,
            &json!({"corpus": "pages", "name": "owner"}),
            "personal",
            &Split,
        )
        .unwrap();
    let shown = values(&owners);
    assert!(!shown.is_empty());
    for (value, label, _) in &shown {
        assert!(!value.contains("alice@example.com"), "{value}");
        assert!(!label.contains("alice@example.com"), "{label}");
    }
    let masked = &shown[0].0;
    let found = ops
        .call_scoped(
            ops::SEARCH_QUERY,
            &json!({"text": "zebrafish", "filter": {"owner": [masked]}}),
            "personal",
            &Split,
        )
        .unwrap();
    assert_eq!(found["pages"].as_array().unwrap().len(), 1);
}

fn found_pages(ops: &PagesOps, filter: Value) -> usize {
    ops.call_scoped(
        ops::SEARCH_QUERY,
        &json!({"text": "zebrafish", "filter": filter}),
        "personal",
        &Split,
    )
    .unwrap()["pages"]
        .as_array()
        .unwrap()
        .len()
}

fn found_sessions(ops: &PagesOps, filter: Value) -> usize {
    ops.call_scoped(
        ops::SEARCH_QUERY,
        &json!({"text": "zebrafish", "filter": filter}),
        "personal",
        &Split,
    )
    .unwrap()["sessions"]
        .as_array()
        .unwrap()
        .len()
}

#[test]
fn an_agent_cannot_confirm_a_raw_email_through_a_filter() {
    let (_dir, ops) = fixture("oracle");
    assert_eq!(
        found_pages(&ops, json!({"owner": ["alice@example.com"]})),
        0
    );
    assert_eq!(
        found_pages(&ops, json!({"owner": ["nobody@example.com"]})),
        0
    );
    let owners = ops
        .call_scoped(
            ops::SEARCH_FACETS,
            &json!({"corpus": "pages", "name": "owner"}),
            "personal",
            &Split,
        )
        .unwrap();
    let masked = values(&owners)[0].0.clone();
    assert_eq!(found_pages(&ops, json!({"owner": [masked]})), 1);
}

#[test]
fn an_agent_cannot_confirm_a_raw_account_through_a_filter() {
    let dir = Dir::new("oracle-account");
    session(&dir, "s1", "/w/app", "zebrafish", 120.0);
    let ops = rig(&dir).with_accounts(Arc::new(|_, _| "carol@example.com".to_string()));
    ops.poke_and_wait();
    assert_eq!(
        found_sessions(&ops, json!({"account": ["carol@example.com"]})),
        0
    );
    let accounts = ops
        .call_scoped(
            ops::SEARCH_FACETS,
            &json!({"corpus": "sessions", "name": "account"}),
            "personal",
            &Split,
        )
        .unwrap();
    let masked = values(&accounts)[0].0.clone();
    assert!(!masked.contains("carol@example.com"), "{masked}");
    assert_eq!(found_sessions(&ops, json!({"account": [masked]})), 1);
}

#[test]
fn an_agent_still_filters_by_an_account_that_masking_leaves_alone() {
    let (_dir, ops) = fixture("plain-account");
    assert_eq!(found_sessions(&ops, json!({"account": ["work"]})), 1);
    assert_eq!(found_sessions(&ops, json!({"account": ["other"]})), 0);
}

fn page_titles(reply: &Value) -> Vec<String> {
    reply["pages"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["title"].as_str().unwrap().to_string())
        .collect()
}

fn asked(ops: &PagesOps, text: &str, mode: &str) -> Value {
    ops.call_scoped(
        ops::SEARCH_QUERY,
        &json!({"text": text, "mode": mode, "corpora": ["pages"]}),
        "personal",
        &Split,
    )
    .unwrap()
}

fn found_by_find(ops: &PagesOps, query: &str) -> usize {
    ops.call_scoped(
        ops::PAGES_FIND,
        &json!({"query": query}),
        "personal",
        &Split,
    )
    .unwrap()["pages"]
        .as_array()
        .unwrap()
        .len()
}

#[test]
fn an_agent_cannot_confirm_a_raw_email_through_page_text() {
    let (_dir, ops) = fixture("oracle-text");
    for text in [
        "alice@example.com",
        "alice@",
        "alice@example",
        "@example.com",
        "ALICE@EXAMPLE.COM",
    ] {
        for mode in ["text", "hybrid"] {
            let reply = asked(&ops, text, mode);
            assert!(
                reply["pages"].as_array().unwrap().is_empty(),
                "{mode} {text}: {reply}"
            );
        }
        assert_eq!(found_by_find(&ops, text), 0, "find {text}");
    }
    let owners = ops
        .call_scoped(
            ops::SEARCH_FACETS,
            &json!({"corpus": "pages", "name": "owner"}),
            "personal",
            &Split,
        )
        .unwrap();
    let masked = values(&owners)[0].0.clone();
    assert!(!masked.contains("alice"), "{masked}");
    assert_eq!(
        asked(&ops, &masked, "text")["pages"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(found_by_find(&ops, &masked), 1);
}

#[test]
fn the_app_still_finds_a_page_by_its_raw_account() {
    let (_dir, ops) = fixture("oracle-app");
    let reply = ops
        .call(ops::SEARCH_QUERY, &json!({"text": "alice@example.com"}))
        .unwrap();
    assert_eq!(reply["pages"].as_array().unwrap().len(), 1, "{reply}");
    let found = ops
        .call(ops::PAGES_FIND, &json!({"query": "alice@example.com"}))
        .unwrap();
    assert_eq!(found["pages"].as_array().unwrap().len(), 1);
}

#[test]
fn an_agent_cannot_confirm_a_raw_email_in_a_page_title() {
    let dir = Dir::new("oracle-title");
    page_titled(
        &dir,
        "t.html",
        A,
        "alice@example.com",
        "Invoice for dave@example.com",
    );
    let ops = rig(&dir);
    ops.poke_and_wait();
    for text in ["dave@example.com", "dave@", "Invoice for dave@example.com"] {
        assert_eq!(found_by_find(&ops, text), 0, "find {text}");
    }
    assert_eq!(found_by_find(&ops, "invoice for d***@e***.com"), 1);
    let reply = asked(&ops, "invoice for d***@e***.com", "text");
    assert_eq!(
        page_titles(&reply),
        ["Invoice for d***@e***.com"],
        "{reply}"
    );
}

#[test]
fn an_agent_account_filter_works_on_a_cold_index() {
    let dir = Dir::new("cold-account");
    session(&dir, "s1", "/w/app", "zebrafish", 120.0);
    let warm = rig(&dir).with_accounts(Arc::new(|_, _| "carol@example.com".to_string()));
    warm.poke_and_wait();
    let accounts = warm
        .call_scoped(
            ops::SEARCH_FACETS,
            &json!({"corpus": "sessions", "name": "account"}),
            "personal",
            &Split,
        )
        .unwrap();
    let masked = values(&accounts)[0].0.clone();
    assert_eq!(
        found_sessions(&warm, json!({"account": [masked.clone()]})),
        1
    );
    drop(warm);
    let cold = rig(&dir).with_accounts(Arc::new(|_, _| "carol@example.com".to_string()));
    assert!(cold.index().is_empty());
    assert_eq!(found_sessions(&cold, json!({"account": [masked]})), 1);
}
