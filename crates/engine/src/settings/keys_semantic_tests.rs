use super::*;

fn key(name: &str) -> &'static KeySpec {
    spec(name).expect("the semantic key")
}

#[test]
fn semantic_keys_have_their_classes() {
    assert_eq!(key("semanticSearch").access, Access::Ask);
    assert_eq!(key("semanticPaused").access, Access::Ask);
    assert_eq!(key("semanticModel").access, Access::Never);
    assert_eq!(key("semanticSearch").kind, Kind::Bool);
    assert_eq!(key("semanticPaused").kind, Kind::Bool);
    assert_eq!(key("semanticModel").kind, Kind::StringList);
    for name in ["semanticSearch", "semanticPaused", "semanticModel"] {
        let k = key(name);
        assert!(!k.title.is_empty() && !k.about.is_empty() && !k.why.is_empty());
        assert!(!k.from_awake);
    }
    assert_eq!(
        key("semanticSearch").about,
        "Turns on search by meaning: indexes your sessions in the background (about 20 minutes the first time) and keeps the accent helper in memory (about 50 MB)."
    );
    assert_eq!(
        key("semanticPaused").about,
        "Pauses or resumes indexing by meaning. While paused, new sessions are found by words only."
    );
}

#[test]
fn semantic_search_is_off_by_default() {
    assert_eq!(key("semanticSearch").default_value(), Some(json!(false)));
    assert_eq!(key("semanticPaused").default_value(), Some(json!(false)));
    assert_eq!(key("semanticModel").default_value(), Some(json!([])));
    assert!(key("semanticSearch").accepts(&json!(true)));
    assert!(!key("semanticSearch").accepts(&json!("yes")));
}
