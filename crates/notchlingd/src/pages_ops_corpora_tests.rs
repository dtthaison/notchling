use crate::pages_ops::*;
use crate::search_scope::Placing;
use notchling_engine::search::query::category::Category;
use notchling_engine::search::range::IndexRange;
use notchling_engine::sessions::Roots;
use notchling_local_api::{ops, OpSpec};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::mpsc::channel;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const DAY: f64 = 86_400.0;

struct Dir(PathBuf);

impl Dir {
    fn new(tag: &str) -> Dir {
        let dir = std::env::temp_dir().join(format!("nl-corpora-{tag}-{}", std::process::id()));
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

fn session(dir: &Dir, name: &str, word: &str, ago: f64) {
    let line = format!(
        r#"{{"type":"user","cwd":"/w/app","message":{{"role":"user","content":"talk about {word}"}}}}"#
    );
    let path = dir.write(
        &format!("claude/projects/-w-app/{name}.jsonl"),
        &format!("{line}\n"),
    );
    age(&path, ago);
}

type Shared = Arc<Mutex<SearchSettings>>;

fn ops_for(dir: &Dir, settings: SearchSettings) -> (PagesOps, Shared) {
    let shared: Shared = Arc::new(Mutex::new(settings));
    let read = shared.clone();
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
    .with_search_settings(Arc::new(move || read.lock().unwrap().clone()));
    (ops, shared)
}

fn all_on() -> SearchSettings {
    SearchSettings::default()
}

fn query(ops: &PagesOps, args: Value) -> Value {
    ops.call(ops::SEARCH_QUERY, &args).unwrap()
}

fn session_ids(reply: &Value) -> Vec<String> {
    reply["sessions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["session_id"].as_str().unwrap().to_string())
        .collect()
}

struct One;

impl Placing for One {
    fn org_of_folder(&self, _: &str) -> String {
        "personal".into()
    }

    fn claimed_by(&self, _: &str) -> Option<String> {
        None
    }

    fn only_personal(&self) -> bool {
        true
    }
}

#[test]
fn the_defaults_are_everything_and_the_default_categories() {
    let settings = SearchSettings::default();
    assert_eq!(settings.range, IndexRange::All);
    assert_eq!(
        settings.on,
        [
            Category::Sessions,
            Category::Settings,
            Category::Pages,
            Category::Inbox,
            Category::Actions
        ]
    );
}

#[test]
fn no_corpora_answers_as_before() {
    let dir = Dir::new("plain");
    session(&dir, "a", "zebrafish", 60.0);
    let (ops, _) = ops_for(&dir, all_on());
    let reply = query(&ops, json!({"text": "zebrafish"}));
    let mut keys: Vec<&str> = reply
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    keys.sort();
    assert_eq!(
        keys,
        [
            "disabled",
            "dropped",
            "indexedSince",
            "indexing",
            "pages",
            "refreshing",
            "sessions"
        ]
    );
    assert_eq!(reply["disabled"], json!([]));
    assert_eq!(reply["dropped"], json!([]));
    assert_eq!(session_ids(&reply), ["a"]);
}

#[test]
fn sessions_off_answers_disabled_and_empties_the_index() {
    let dir = Dir::new("off");
    session(&dir, "a", "zebrafish", 60.0);
    let (ops, shared) = ops_for(&dir, all_on());
    assert_eq!(
        session_ids(&query(&ops, json!({"text": "zebrafish"}))),
        ["a"]
    );
    assert!(!ops.index().is_empty());
    shared.lock().unwrap().on = vec![Category::Pages];
    let reply = query(&ops, json!({"text": "zebrafish"}));
    assert_eq!(reply["sessions"], json!([]));
    assert_eq!(reply["disabled"], json!(["sessions"]));
    assert!(ops.index().is_empty());
    let cache =
        notchling_engine::search::fts::SearchDb::open(&dir.path("data/daemon/search.db")).unwrap();
    assert!(cache.session_metas().is_empty());
    shared.lock().unwrap().on = vec![Category::Pages, Category::Sessions];
    ops.poke_and_wait();
    assert_eq!(
        session_ids(&query(&ops, json!({"text": "zebrafish"}))),
        ["a"]
    );
}

#[test]
fn sessions_off_at_start_empties_the_cache_on_disk() {
    let dir = Dir::new("off-cold");
    session(&dir, "a", "zebrafish", 60.0);
    let (warm, _) = ops_for(&dir, all_on());
    warm.poke_and_wait();
    let cache = || {
        notchling_engine::search::fts::SearchDb::open(&dir.path("data/daemon/search.db"))
            .unwrap()
            .session_metas()
            .len()
    };
    assert_eq!(cache(), 1);
    drop(warm);
    let (cold, _) = ops_for(
        &dir,
        SearchSettings {
            range: IndexRange::All,
            on: vec![Category::Pages],
        },
    );
    assert!(cold.index().is_empty());
    let reply = query(&cold, json!({"text": "zebrafish"}));
    assert_eq!(reply["disabled"], json!(["sessions"]));
    assert_eq!(cache(), 0);
}

#[test]
fn corpora_that_are_asked_and_not_answered_come_back_dropped() {
    let dir = Dir::new("dropped");
    session(&dir, "a", "zebrafish", 60.0);
    let (ops, shared) = ops_for(&dir, all_on());
    let asked = json!({"text": "zebrafish", "corpora": ["sessions", "actions", "inbox", "settings", "vault", "bogus"]});
    let reply = query(&ops, asked.clone());
    assert_eq!(session_ids(&reply), ["a"]);
    assert_eq!(
        reply["dropped"],
        json!(["actions", "inbox", "settings", "bogus"])
    );
    assert_eq!(reply["disabled"], json!(["vault"]));
    let agent = ops
        .call_scoped(ops::SEARCH_QUERY, &asked, "personal", &One)
        .unwrap();
    assert_eq!(agent["dropped"], reply["dropped"]);
    assert_eq!(agent["disabled"], reply["disabled"]);
    shared.lock().unwrap().on = vec![Category::Sessions];
    let narrower = query(&ops, asked);
    assert_eq!(
        narrower["disabled"],
        json!(["actions", "inbox", "settings", "vault"])
    );
    assert_eq!(narrower["dropped"], json!(["bogus"]));
}

#[test]
fn corpora_choose_what_is_searched() {
    let dir = Dir::new("choose");
    session(&dir, "a", "zebrafish", 60.0);
    let (ops, _) = ops_for(&dir, all_on());
    let reply = query(&ops, json!({"text": "zebrafish", "corpora": ["pages"]}));
    assert_eq!(reply["sessions"], json!([]));
    assert_eq!(reply["disabled"], json!([]));
    let bad = ops.call(
        ops::SEARCH_QUERY,
        &json!({"text": "zebrafish", "corpora": "x"}),
    );
    assert!(bad.is_err());
}

#[test]
fn the_range_follows_the_key_without_a_restart() {
    let dir = Dir::new("range");
    session(&dir, "recent", "zebrafish", 60.0);
    session(&dir, "ancient", "zebrafish", 400.0 * DAY);
    let (ops, shared) = ops_for(&dir, all_on());
    let mut both = session_ids(&query(&ops, json!({"text": "zebrafish"})));
    both.sort();
    assert_eq!(both, ["ancient", "recent"]);
    assert_eq!(
        query(&ops, json!({"text": "zebrafish"}))["indexedSince"],
        Value::Null
    );
    shared.lock().unwrap().range = IndexRange::Days14;
    ops.poke_and_wait();
    let narrowed = query(&ops, json!({"text": "zebrafish"}));
    assert_eq!(session_ids(&narrowed), ["recent"]);
    assert!(narrowed["indexedSince"].as_f64().unwrap() > now() - 15.0 * DAY);
    shared.lock().unwrap().range = IndexRange::All;
    ops.poke_and_wait();
    assert_eq!(
        session_ids(&query(&ops, json!({"text": "zebrafish"}))).len(),
        2
    );
}

#[test]
fn an_agent_is_cut_to_14_days_whatever_the_key_says() {
    let dir = Dir::new("agent");
    session(&dir, "recent", "zebrafish", 60.0);
    session(&dir, "ancient", "zebrafish", 400.0 * DAY);
    let (ops, _) = ops_for(&dir, all_on());
    let app = query(&ops, json!({"text": "zebrafish"}));
    assert_eq!(session_ids(&app).len(), 2);
    let agent = ops
        .call_scoped(
            ops::SEARCH_QUERY,
            &json!({"text": "zebrafish"}),
            "personal",
            &One,
        )
        .unwrap();
    assert_eq!(session_ids(&agent), ["recent"]);
    assert_eq!(agent["window_days"], json!(14));
}

#[test]
fn embed_refresh_is_app_tier() {
    let dir = Dir::new("refresh");
    let (ops, _) = ops_for(&dir, all_on());
    assert_eq!(ops.spec(ops::EMBED_REFRESH), Some(OpSpec::APP));
    assert_eq!(ops.call(ops::EMBED_REFRESH, &json!({})).unwrap(), json!({}));
}

#[test]
fn the_sessions_corpus_carries_accounts_and_antigravity_rows() {
    use notchling_engine::search::index::TitledSession;
    let dir = Dir::new("embed-accounts");
    session(&dir, "s1", "zebrafish", 120.0);
    let (ops, _) = ops_for(&dir, all_on());
    let ops = ops
        .with_accounts(Arc::new(|_, _| "carol".to_string()))
        .with_titles(Arc::new(|| {
            vec![TitledSession {
                id: "a1".into(),
                title: "tidy the zebrafish notes".into(),
                folder: "/w/agy".into(),
                modified: now() - 60.0,
            }]
        }));
    let corpora = ops.embed_corpora(&[Category::Sessions]);
    assert_eq!(corpora.len(), 1);
    let corpus = &corpora[0];
    let stamps = corpus.scan(None, now());
    let mut keys: Vec<&str> = stamps.iter().map(|s| s.key.as_str()).collect();
    keys.sort();
    assert_eq!(keys, ["agy:a1", "claude:s1"]);
    for stamp in &stamps {
        let doc = corpus.read(stamp, now()).unwrap();
        assert!(
            doc.facets
                .contains(&("account".to_string(), "carol".to_string())),
            "{}: {:?}",
            stamp.key,
            doc.facets
        );
    }
}

#[test]
fn a_query_that_read_the_settings_before_a_disable_cannot_undo_it() {
    let dir = Dir::new("stale-read");
    session(&dir, "a", "zebrafish", 60.0);
    let shared: Shared = Arc::new(Mutex::new(all_on()));
    let (read_tx, reading) = channel::<()>();
    let (go_tx, proceed) = channel::<()>();
    let pause = Mutex::new(Some((read_tx, proceed)));
    let current = shared.clone();
    let ops = Arc::new(
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
        .with_search_settings(Arc::new(move || {
            let seen = current.lock().unwrap().clone();
            let first = pause.lock().unwrap().take();
            if let Some((read_tx, proceed)) = first {
                read_tx.send(()).unwrap();
                proceed.recv().unwrap();
            }
            seen
        })),
    );
    let stale = {
        let ops = ops.clone();
        thread::spawn(move || query(&ops, json!({"text": "zebrafish"})))
    };
    reading.recv_timeout(Duration::from_secs(10)).unwrap();
    shared.lock().unwrap().on = vec![Category::Pages];
    let (done, finished) = channel::<()>();
    let poke = {
        let ops = ops.clone();
        thread::spawn(move || {
            ops.poke();
            let _ = done.send(());
        })
    };
    let _ = finished.recv_timeout(Duration::from_millis(500));
    go_tx.send(()).unwrap();
    stale.join().unwrap();
    poke.join().unwrap();
    assert!(ops.index().is_empty());
    let cache =
        notchling_engine::search::fts::SearchDb::open(&dir.path("data/daemon/search.db")).unwrap();
    assert!(cache.session_metas().is_empty());
    let reply = query(&ops, json!({"text": "zebrafish"}));
    assert_eq!(reply["disabled"], json!(["sessions"]));
    assert!(ops.index().is_empty());
}
