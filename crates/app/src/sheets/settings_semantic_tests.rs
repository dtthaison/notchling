use super::*;

fn ids(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| (*s).to_owned()).collect()
}

fn known(id: &str) -> bool {
    ["a", "b", "c", "d"].contains(&id)
}

#[test]
fn fused_settings_keep_the_selection() {
    let text = ids(&["a", "b", "c"]);
    let none = fuse_settings(&text, &[], known);
    assert_eq!(none, text);
    let both = fuse_settings(&text, &[("b".into(), 0.9), ("a".into(), 0.5)], known);
    assert_eq!(both, ids(&["a", "b", "c"]));
    let selected = "b";
    assert!(both.iter().any(|id| id == selected));
    let flipped = fuse_settings(&text, &[("b".into(), 0.9)], known);
    assert_eq!(flipped.first().map(String::as_str), Some("b"));
}

#[test]
fn a_semantic_only_setting_is_added() {
    let text = ids(&["a"]);
    let fused = fuse_settings(&text, &[("d".into(), 0.7), ("gone".into(), 0.6)], known);
    assert_eq!(fused, ids(&["a", "d"]));
}

#[test]
fn the_docs_are_shown_keys_in_english_and_within_the_limits() {
    let docs = settings_docs();
    assert!(!docs.is_empty() && docs.len() <= 200);
    assert!(docs.iter().all(|(_, text)| text.chars().count() <= 4000));
    let semantic = docs.iter().find(|(id, _)| id == "semanticSearch").unwrap();
    assert!(semantic.1.starts_with("Semantic search"));
    assert!(docs.iter().all(|(id, _)| id != "semanticModel"));
}
