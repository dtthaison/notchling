//! Ports of `PageLookupTests` from the Swift app.

use super::*;

// MARK: Page lookup (PageLookupTests)

fn pages() -> Vec<PageEntry> {
    vec![
        PageEntry {
            folder: "a".into(),
            file: "index.html".into(),
            title: "Cost Report".into(),
            url: "https://claude.ai/artifact/AbCdEfGhIjKlMnOpQrStUv".into(),
            source: String::new(),
            project: "notchling".into(),
            account: "me@example.com".into(),
            updated: 1.0,
            version: String::new(),
        },
        PageEntry {
            folder: "b".into(),
            file: "index.html".into(),
            title: "Release Plan".into(),
            url: "https://claude.ai/code/artifact/ZZZZZZZZZZZZZZZZZZZZZZ".into(),
            source: String::new(),
            project: "lingeva".into(),
            account: "work@example.com".into(),
            updated: 2.0,
            version: String::new(),
        },
    ]
}

fn folders(found: &[&PageEntry]) -> Vec<String> {
    found.iter().map(|e| e.folder.clone()).collect()
}

#[test]
fn artifact_id_from_links_and_bare_ids() {
    assert_eq!(
        artifact_id_in(" https://claude.ai/artifact/AbCdEfGhIjKlMnOpQrStUv ").as_deref(),
        Some("AbCdEfGhIjKlMnOpQrStUv")
    );
    assert_eq!(
        artifact_id_in("claude.ai/code/artifact/abc_def-123").as_deref(),
        Some("abc_def-123")
    );
    assert_eq!(
        artifact_id_in("KVBHDXYvY8f1b1AZG6PbAR").as_deref(),
        Some("KVBHDXYvY8f1b1AZG6PbAR")
    );
    assert_eq!(artifact_id_in("cost report"), None);
}

#[test]
fn artifact_id_edge_cases() {
    // The id stops at the first character outside [A-Za-z0-9_-].
    assert_eq!(
        artifact_id_in("see claude.ai/artifact/abc123?x=1 ok").as_deref(),
        Some("abc123")
    );
    // A link with no id falls through to the bare-id rule, which a link is not.
    assert_eq!(artifact_id_in("claude.ai/artifact/"), None);
    // A later match is found when the first `claude.ai/` has no artifact path.
    assert_eq!(
        artifact_id_in("claude.ai/chat claude.ai/artifact/xyz").as_deref(),
        Some("xyz")
    );
    // Bare ids need 16 characters.
    assert_eq!(artifact_id_in("short_id"), None);
    assert_eq!(
        artifact_id_in("0123456789abcdef").as_deref(),
        Some("0123456789abcdef")
    );
}

#[test]
fn find_by_link_then_by_words() {
    let all = pages();
    assert_eq!(
        folders(&find_pages(
            "https://claude.ai/artifact/AbCdEfGhIjKlMnOpQrStUv",
            &all
        )),
        ["a"]
    );
    assert_eq!(
        folders(&find_pages("abcdefghijklmnopqrstuv", &all)),
        ["a"],
        "an id matches regardless of case as a fallback"
    );
    assert_eq!(folders(&find_pages("plan lingeva", &all)), ["b"]);
    assert_eq!(folders(&find_pages("work@example", &all)), ["b"]);
    assert_eq!(find_pages("", &all).len(), 2);
    assert!(find_pages("nothing here", &all).is_empty());
}

#[test]
fn find_words_split_on_spaces_only_and_fold_case() {
    let all = pages();
    assert_eq!(folders(&find_pages("  COST   report ", &all)), ["a"]);
    // Only the space separates words, like the Swift code: a tab stays part of the word.
    assert!(find_pages("cost\treport", &all).is_empty());
    // A bare-id-shaped query that matches no link still searches the words.
    assert!(find_pages("zzzzzzzzzzzzzzzzzzzzzzzz", &all).is_empty());
    assert_eq!(
        folders(&find_pages("0123456789abcdef", &all)),
        Vec::<String>::new()
    );
}

#[test]
fn find_matches_vietnamese_titles_case_insensitively() {
    let mut all = pages();
    all[0].title = "Báo cáo CHI PHÍ".into();
    assert_eq!(folders(&find_pages("chi phí", &all)), ["a"]);
    assert!(find_pages("chi phi", &all).is_empty());
}

#[test]
fn page_entry_artifact_id_and_json() {
    let all = pages();
    assert_eq!(all[0].artifact_id(), Some("AbCdEfGhIjKlMnOpQrStUv"));
    let mut none = all[0].clone();
    none.url = String::new();
    assert_eq!(none.artifact_id(), None);

    let json = serde_json::to_string(&all[0]).unwrap_or_default();
    let back: Option<PageEntry> = serde_json::from_str(&json).ok();
    assert_eq!(back.as_ref(), Some(&all[0]));
    // Only folder and file are required; the title falls back to the folder.
    let min: Option<PageEntry> = serde_json::from_str(r#"{"folder":"f","file":"index.html"}"#).ok();
    assert_eq!(min.as_ref().map(|e| e.title.as_str()), Some("f"));
    assert_eq!(min.map(|e| e.updated), Some(0.0));
    assert!(serde_json::from_str::<PageEntry>(r#"{"title":"no folder"}"#).is_err());
}
