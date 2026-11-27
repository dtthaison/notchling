use super::corpus_pages::page_key;
use super::index::SessionIndex;
use super::PageEntry;
use crate::pages::PageLibrary;
use crate::sessions::testutil::TempDir;
use serde_json::json;

fn entry(folder: &str, title: &str, updated: f64, version: &str) -> PageEntry {
    PageEntry {
        folder: folder.into(),
        file: "index.html".into(),
        title: title.into(),
        url: format!("https://claude.ai/artifact/{folder}"),
        source: String::new(),
        cwd: String::new(),
        project: "notchling".into(),
        account: "me@example.com".into(),
        updated,
        version: version.into(),
    }
}

fn plant(dir: &TempDir, pages: &[(PageEntry, &str)]) -> PageLibrary {
    for (e, html) in pages {
        dir.write(&format!("pages/{}/{}", e.folder, e.file), html);
    }
    let list: Vec<&PageEntry> = pages.iter().map(|(e, _)| e).collect();
    dir.write(
        "pages/pages.json",
        &json!({"pages": serde_json::to_value(&list).unwrap()}).to_string(),
    );
    PageLibrary::read_only(dir.path("pages"))
}

fn keys(index: &SessionIndex, word: &str) -> Vec<String> {
    index
        .search_pages(&[word], 100.0, 30)
        .into_iter()
        .map(|h| h.key)
        .collect()
}

#[test]
fn a_page_is_found_by_its_content() {
    let dir = TempDir::new("corpus-found");
    let a = entry("aaa", "Cost Report", 10.0, "v1");
    let lib = plant(
        &dir,
        &[(
            a.clone(),
            "<h1>Intro</h1><p>nothing</p><h2>Marsupials</h2><p>the quokka smiles</p>",
        )],
    );
    let index = SessionIndex::with_range(None, None);
    index.refresh_pages(&lib);
    let hits = index.search_pages(&["quokka"], 100.0, 30);
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].key, page_key(&a));
    assert!(index
        .page_snippet(&a, &["quokka"])
        .unwrap()
        .contains("quokka"));
    assert!(keys(&index, "zebra").is_empty());
}

#[test]
fn scripts_and_data_urls_are_not_text() {
    let dir = TempDir::new("corpus-scripts");
    let html = concat!(
        "<!DOCTYPE html>",
        "<style>.hiddenclass{color:red}</style>",
        "<script>var scriptvar = 1;</script>",
        "<img src=\"data:image/png;base64,AAAAiVBORw0KGgoAAAANSUhEUg\">",
        "<p>visible words data:image/png;base64,QQQQiVBORw0KGgoZZZ here</p>"
    );
    let lib = plant(&dir, &[(entry("aaa", "Page", 10.0, "v1"), html)]);
    let index = SessionIndex::with_range(None, None);
    index.refresh_pages(&lib);
    for hidden in [
        "hiddenclass",
        "scriptvar",
        "iVBORw0KGgoAAAANSUhEUg",
        "QQQQiVBORw0KGgoZZZ",
    ] {
        assert!(keys(&index, hidden).is_empty(), "{hidden} was indexed");
    }
    assert_eq!(keys(&index, "visible").len(), 1);
    assert!(keys(&index, "doctype").is_empty());
}

#[test]
fn a_removed_page_leaves_the_index() {
    let dir = TempDir::new("corpus-removed");
    let (a, b) = (
        entry("aaa", "One", 10.0, "v1"),
        entry("bbb", "Two", 11.0, "v1"),
    );
    let lib = plant(
        &dir,
        &[(a.clone(), "<p>quokka</p>"), (b.clone(), "<p>quokka</p>")],
    );
    let index = SessionIndex::with_range(None, None);
    index.refresh_pages(&lib);
    assert_eq!(keys(&index, "quokka").len(), 2);
    let lib = plant(&dir, &[(a.clone(), "<p>quokka</p>")]);
    let stats = index.refresh_pages(&lib);
    assert_eq!(stats.dropped, 1);
    assert_eq!(keys(&index, "quokka"), [page_key(&a)]);
}

#[test]
fn an_unchanged_page_is_not_read_again_and_a_new_version_is() {
    let dir = TempDir::new("corpus-stamps");
    let a = entry("aaa", "One", 10.0, "v1");
    let lib = plant(&dir, &[(a.clone(), "<p>quokka</p>")]);
    let index = SessionIndex::with_range(None, None);
    assert_eq!(index.refresh_pages(&lib).read, 1);
    let again = index.refresh_pages(&lib);
    assert_eq!((again.read, again.reused), (0, 1));
    let next = entry("aaa", "One", 20.0, "v2");
    let lib = plant(&dir, &[(next, "<p>wombat</p>")]);
    assert_eq!(index.refresh_pages(&lib).read, 1);
    assert!(keys(&index, "quokka").is_empty());
    assert_eq!(keys(&index, "wombat").len(), 1);
}

#[test]
fn a_disabled_pages_corpus_is_not_refilled_by_a_refresh() {
    let dir = TempDir::new("corpus-off");
    let a = entry("aaa", "Cost Report", 10.0, "v1");
    let lib = plant(&dir, &[(a, "<p>the quokka smiles</p>")]);
    let index = SessionIndex::with_range(None, None);
    index.refresh_pages(&lib);
    assert_eq!(keys(&index, "quokka").len(), 1);
    index.set_pages_on(false).unwrap();
    assert!(keys(&index, "quokka").is_empty());
    assert_eq!(index.refresh_pages(&lib).read, 0);
    assert!(keys(&index, "quokka").is_empty());
    index.set_pages_on(true).unwrap();
    assert_eq!(index.refresh_pages(&lib).read, 1);
    assert_eq!(keys(&index, "quokka").len(), 1);
}
