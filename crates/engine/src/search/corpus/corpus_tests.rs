use super::*;
use crate::pages::PageLibrary;
use crate::search::chunk::ChunkKind;
use crate::search::index::{ClaudeRoot, IndexRoots, TitledSession};
use crate::sessions::testutil::TempDir;
use std::fs::OpenOptions;
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const DAY: f64 = 86_400.0;

fn now() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

fn age(path: &Path, ago: f64) {
    let file = OpenOptions::new().write(true).open(path).unwrap();
    file.set_modified(UNIX_EPOCH + Duration::from_secs_f64((now() - ago).max(0.0)))
        .unwrap();
}

fn user(text: &str, cwd: &str) -> String {
    format!(r#"{{"type":"user","cwd":"{cwd}","message":{{"role":"user","content":"{text}"}}}}"#)
}

fn assistant(text: &str) -> String {
    format!(
        r#"{{"type":"assistant","message":{{"role":"assistant","content":[{{"type":"text","text":"{text}"}}]}}}}"#
    )
}

fn transcript(dir: &TempDir, rel: &str, lines: &[String], ago: f64) -> std::path::PathBuf {
    let path = dir.write(rel, &(lines.join("\n") + "\n"));
    age(&path, ago);
    path
}

fn roots(dir: &TempDir) -> IndexRoots {
    IndexRoots {
        claude: vec![ClaudeRoot {
            projects: dir.path("claude/projects"),
            config_dir: None,
        }],
        codex_sessions: Some(dir.path("codex/sessions")),
        titled: Vec::new(),
    }
}

fn session_lines() -> Vec<String> {
    vec![
        r#"{"type":"ai-title","aiTitle":"Token cache work"}"#.to_string(),
        user("fix the token refresh", "/w/app"),
        assistant("Found the stale token cache and cleared it."),
        user("now add a test for the cache", "/w/app"),
        assistant("Added a test that fills the cache twice."),
    ]
}

#[cfg(unix)]
#[test]
fn sessions_scan_is_stat_only() {
    use std::os::unix::fs::PermissionsExt;
    let dir = TempDir::new("corpus-stat");
    let path = transcript(
        &dir,
        "claude/projects/-w-app/c1.jsonl",
        &session_lines(),
        3600.0,
    );
    let size = std::fs::metadata(&path).unwrap().len();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o000)).unwrap();
    let corpus = SessionsCorpus::new(roots(&dir));
    let stamps = corpus.scan(Some(14.0 * DAY), now());
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
    assert_eq!(stamps.len(), 1);
    assert_eq!(stamps[0].key, "claude:c1");
    assert!(
        stamps[0].stamp.ends_with(&format!(":{size}")),
        "{}",
        stamps[0].stamp
    );
    assert!((stamps[0].at - (now() - 3600.0)).abs() < 5.0);
    let again = corpus.scan(Some(14.0 * DAY), now());
    assert_eq!(again, stamps);
}

#[test]
fn a_session_stamp_changes_when_the_file_goes_quiet() {
    let dir = TempDir::new("corpus-quiet");
    transcript(
        &dir,
        "claude/projects/-w-app/c1.jsonl",
        &session_lines(),
        10.0,
    );
    let corpus = SessionsCorpus::new(roots(&dir));
    let open = corpus.scan(None, now());
    let quiet = corpus.scan(None, now() + 120.0);
    assert_eq!(open.len(), 1);
    assert_ne!(open[0].stamp, quiet[0].stamp);
    let later = corpus.scan(None, now() + 3600.0);
    assert_eq!(quiet[0].stamp, later[0].stamp);
}

#[test]
fn sessions_scan_honors_the_window() {
    let dir = TempDir::new("corpus-window");
    transcript(
        &dir,
        "claude/projects/-w-app/new.jsonl",
        &session_lines(),
        3600.0,
    );
    transcript(
        &dir,
        "claude/projects/-w-app/old.jsonl",
        &session_lines(),
        40.0 * DAY,
    );
    let corpus = SessionsCorpus::new(roots(&dir));
    let keys = |window| -> Vec<String> {
        let mut k: Vec<String> = corpus
            .scan(window, now())
            .into_iter()
            .map(|s| s.key)
            .collect();
        k.sort();
        k
    };
    assert_eq!(keys(Some(14.0 * DAY)), vec!["claude:new"]);
    assert_eq!(keys(None), vec!["claude:new", "claude:old"]);
}

#[test]
fn sessions_read_gives_head_and_turns() {
    let dir = TempDir::new("corpus-read");
    transcript(
        &dir,
        "claude/projects/-w-app/c1.jsonl",
        &session_lines(),
        3600.0,
    );
    let corpus = SessionsCorpus::new(roots(&dir));
    assert_eq!(corpus.id(), "sessions");
    assert_eq!(corpus.tier(), Tier::Agent);
    let stamps = corpus.scan(None, now());
    let doc = corpus.read(&stamps[0], now()).expect("doc");
    assert_eq!(doc.key, "claude:c1");
    assert_eq!(doc.title, "Token cache work");
    assert_eq!(doc.at, stamps[0].at);
    assert!(doc
        .facets
        .contains(&("tool".to_string(), "claude".to_string())));
    assert!(doc
        .facets
        .contains(&("project".to_string(), "/w/app".to_string())));
    assert_eq!(doc.chunks[0].kind, ChunkKind::Head);
    assert!(doc.chunks[0].text.contains("Token cache work"));
    let turns: Vec<&str> = doc
        .chunks
        .iter()
        .filter(|c| c.kind == ChunkKind::Turn)
        .map(|c| c.text.as_str())
        .collect();
    assert_eq!(turns.len(), 2);
    assert!(turns.iter().any(|t| t.contains("stale token cache")));
    assert!(turns.iter().any(|t| t.contains("fills the cache twice")));
}

#[test]
fn sessions_read_of_a_gone_file_is_none() {
    let dir = TempDir::new("corpus-gone");
    let path = transcript(
        &dir,
        "claude/projects/-w-app/c1.jsonl",
        &session_lines(),
        3600.0,
    );
    let corpus = SessionsCorpus::new(roots(&dir));
    let stamps = corpus.scan(None, now());
    std::fs::remove_file(path).unwrap();
    assert!(corpus.read(&stamps[0], now()).is_none());
    let unknown = DocStamp {
        key: "claude:nope".to_string(),
        stamp: String::new(),
        at: 0.0,
    };
    assert!(corpus.read(&unknown, now()).is_none());
}

fn library(dir: &TempDir, updated: f64) -> PageLibrary {
    let html = "<html><head><title>Plan</title></head><body><h1>Launch plan</h1>\
                <p>We ship the search page on Friday after the review.</p>\
                <h2>Risks</h2><p>The index may be stale for old pages.</p></body></html>";
    dir.write("pages/plan-1/index.html", html);
    let list = format!(
        r#"{{"pages":[{{"folder":"plan-1","file":"index.html","title":"Plan","url":"https://claude.ai/artifact/abc","source":"","project":"app","account":"me@example.com","updated":{updated},"version":"v3"}},{{"folder":"ghost","file":"index.html","title":"Ghost","updated":{updated},"version":"v1"}}]}}"#
    );
    dir.write("pages/pages.json", &list);
    PageLibrary::read_only(dir.path("pages"))
}

#[test]
fn pages_read_local_html() {
    let dir = TempDir::new("corpus-pages");
    let t = now() - 3600.0;
    let corpus = PagesCorpus::new(library(&dir, t));
    assert_eq!(corpus.id(), "pages");
    assert_eq!(corpus.tier(), Tier::Agent);
    let stamps = corpus.scan(None, now());
    assert_eq!(stamps.len(), 1, "a page with no local file is not listed");
    assert_eq!(stamps[0].key, "page:plan-1/index.html");
    assert!(stamps[0].stamp.starts_with("v3:"), "{}", stamps[0].stamp);
    assert_eq!(stamps[0].at, t);
    let doc = corpus.read(&stamps[0], now()).expect("doc");
    assert_eq!(doc.title, "Plan");
    assert!(doc
        .facets
        .contains(&("owner".to_string(), "me@example.com".to_string())));
    assert!(doc
        .facets
        .contains(&("project".to_string(), "app".to_string())));
    assert!(!doc.chunks.is_empty());
    assert!(doc.chunks.iter().all(|c| c.kind == ChunkKind::Page));
    assert!(doc
        .chunks
        .iter()
        .any(|c| c.text.contains("search page on Friday")));
    assert!(corpus.scan(Some(60.0), now()).is_empty());
}

#[test]
fn settings_stamp_changes_with_text() {
    let corpus = SettingsCorpus::new();
    assert_eq!(corpus.id(), "settings");
    assert_eq!(corpus.tier(), Tier::App);
    assert!(corpus.scan(None, 0.0).is_empty());
    corpus.set(vec![
        (
            "search.semantic".to_string(),
            "Search by meaning".to_string(),
        ),
        ("notch.width".to_string(), "Width of the notch".to_string()),
    ]);
    let first = corpus.scan(None, 0.0);
    assert_eq!(first.len(), 2);
    corpus.set(vec![
        (
            "search.semantic".to_string(),
            "Search by meaning, on this Mac".to_string(),
        ),
        ("notch.width".to_string(), "Width of the notch".to_string()),
    ]);
    let second = corpus.scan(None, 0.0);
    let stamp =
        |list: &[DocStamp], key: &str| list.iter().find(|s| s.key == key).unwrap().stamp.clone();
    assert_ne!(
        stamp(&first, "search.semantic"),
        stamp(&second, "search.semantic")
    );
    assert_eq!(stamp(&first, "notch.width"), stamp(&second, "notch.width"));
    let doc = corpus
        .read(
            second.iter().find(|s| s.key == "search.semantic").unwrap(),
            0.0,
        )
        .expect("doc");
    assert_eq!(doc.chunks.len(), 1);
    assert_eq!(doc.chunks[0].kind, ChunkKind::Entry);
    assert_eq!(doc.chunks[0].text, "Search by meaning, on this Mac");
    assert!(doc
        .facets
        .contains(&("section".to_string(), "search".to_string())));
}

fn page_list(dir: &TempDir, updated: f64) {
    let list = format!(
        r#"{{"pages":[{{"folder":"plan-1","file":"index.html","title":"Plan","url":"https://claude.ai/artifact/abc","source":"","project":"app","account":"me@example.com","updated":{updated},"version":"v3"}}]}}"#
    );
    dir.write("pages/pages.json", &list);
}

#[test]
fn pages_keys_are_the_text_index_keys() {
    let dir = TempDir::new("corpus-page-key");
    let t = now() - 60.0;
    let library = library(&dir, t);
    let entry = library
        .entries()
        .into_iter()
        .find(|e| e.folder == "plan-1")
        .unwrap();
    let corpus = PagesCorpus::new(library);
    let stamps = corpus.scan(None, now());
    assert_eq!(stamps[0].key, crate::search::corpus_pages::page_key(&entry));
    let doc = corpus.read(&stamps[0], now()).expect("doc");
    assert_eq!(doc.key, stamps[0].key);
}

#[test]
fn a_page_with_bad_bytes_is_read_without_markup_noise() {
    let dir = TempDir::new("corpus-page-noise");
    let mut html = b"<!DOCTYPE html><html><head><title>Plan</title></head><body><p>Launch plan for the search page ".to_vec();
    html.push(0xFF);
    html.extend_from_slice(
        b" and a logo data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAAB shipped Friday.</p></body></html>",
    );
    std::fs::create_dir_all(dir.path("pages/plan-1")).unwrap();
    std::fs::write(dir.path("pages/plan-1/index.html"), html).unwrap();
    page_list(&dir, now() - 60.0);
    let corpus = PagesCorpus::new(PageLibrary::read_only(dir.path("pages")));
    let stamps = corpus.scan(None, now());
    let doc = corpus
        .read(&stamps[0], now())
        .expect("a bad byte does not drop the page");
    let text: String = doc
        .chunks
        .iter()
        .map(|c| c.text.as_str())
        .collect::<Vec<_>>()
        .join(" ");
    assert!(text.contains("Launch plan for the search page"), "{text}");
    assert!(!text.to_lowercase().contains("doctype"), "{text}");
    assert!(!text.contains("iVBORw0KGgo"), "{text}");
}

#[test]
fn a_page_cut_inside_a_character_is_still_read() {
    let dir = TempDir::new("corpus-page-cut");
    let mut html = String::from("<html><body><p>Launch plan for the search page.</p><p>");
    while html.len() < 1_999_999 {
        html.push('a');
    }
    html.push('\u{e9}');
    html.push_str("</p></body></html>");
    dir.write("pages/plan-1/index.html", &html);
    page_list(&dir, now() - 60.0);
    let corpus = PagesCorpus::new(PageLibrary::read_only(dir.path("pages")));
    let stamps = corpus.scan(None, now());
    let doc = corpus
        .read(&stamps[0], now())
        .expect("a cut character does not drop the page");
    assert!(doc.chunks.iter().any(|c| c.text.contains("Launch plan")));
}

fn accounted(account: &'static str) -> crate::search::index::AccountOf {
    std::sync::Arc::new(move |_, _| account.to_string())
}

#[test]
fn session_docs_carry_the_account_facet() {
    let dir = TempDir::new("corpus-account");
    transcript(
        &dir,
        "claude/projects/-w-app/c1.jsonl",
        &session_lines(),
        3600.0,
    );
    let plain = SessionsCorpus::new(roots(&dir));
    let stamps = plain.scan(None, now());
    let doc = plain.read(&stamps[0], now()).expect("doc");
    assert!(doc
        .facets
        .contains(&("account".to_string(), "unknown".to_string())));
    let named = SessionsCorpus::new(roots(&dir)).with_accounts(accounted("work"));
    let stamps = named.scan(None, now());
    let doc = named.read(&stamps[0], now()).expect("doc");
    assert!(doc
        .facets
        .contains(&("account".to_string(), "work".to_string())));
    assert!(doc
        .facets
        .contains(&("tool".to_string(), "claude".to_string())));
}

fn titled(dir: &TempDir) -> IndexRoots {
    let mut roots = roots(dir);
    roots.titled = vec![
        TitledSession {
            id: "a1".into(),
            title: "Plan the notch".into(),
            folder: "/w/app".into(),
            modified: now() - 3600.0,
        },
        TitledSession {
            id: "old".into(),
            title: "Ancient chat".into(),
            folder: "/w/app".into(),
            modified: now() - 40.0 * DAY,
        },
    ];
    roots
}

#[test]
fn antigravity_sessions_are_scanned_and_read() {
    let dir = TempDir::new("corpus-agy");
    let corpus = SessionsCorpus::new(titled(&dir)).with_accounts(accounted("work"));
    let keys = |window| -> Vec<String> {
        let mut k: Vec<String> = corpus
            .scan(window, now())
            .into_iter()
            .map(|s| s.key)
            .collect();
        k.sort();
        k
    };
    assert_eq!(keys(Some(14.0 * DAY)), vec!["agy:a1"]);
    assert_eq!(keys(None), vec!["agy:a1", "agy:old"]);
    let stamps = corpus.scan(Some(14.0 * DAY), now());
    let doc = corpus.read(&stamps[0], now()).expect("doc");
    assert_eq!(doc.key, "agy:a1");
    assert_eq!(doc.title, "Plan the notch");
    assert_eq!(doc.at, stamps[0].at);
    for facet in [("tool", "agy"), ("project", "/w/app"), ("account", "work")] {
        assert!(
            doc.facets
                .contains(&(facet.0.to_string(), facet.1.to_string())),
            "{facet:?} in {:?}",
            doc.facets
        );
    }
    assert!(doc.chunks[0].text.contains("Plan the notch"));
}

#[test]
fn an_antigravity_stamp_changes_with_the_title() {
    let dir = TempDir::new("corpus-agy-stamp");
    let first = SessionsCorpus::new(titled(&dir)).scan(Some(14.0 * DAY), now());
    let mut renamed = titled(&dir);
    renamed.titled[0].title = "Plan the dock".into();
    renamed.titled[0].modified = first_modified(&first);
    let second = SessionsCorpus::new(renamed).scan(Some(14.0 * DAY), now());
    assert_eq!(first[0].key, second[0].key);
    assert_ne!(first[0].stamp, second[0].stamp);
}

fn first_modified(stamps: &[DocStamp]) -> f64 {
    stamps[0].at
}
