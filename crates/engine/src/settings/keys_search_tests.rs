use super::*;
use serde_json::json;

const EIGHT: [&str; 8] = [
    "sessions", "settings", "pages", "inbox", "actions", "activity", "vault", "machines",
];

fn key(name: &str) -> &'static KeySpec {
    spec(name).expect("the search key")
}

#[test]
fn the_search_keys_have_their_classes() {
    assert_eq!(key("searchIndexRange").access, Access::Ask);
    assert_eq!(key("searchCategories").access, Access::Ask);
    assert_eq!(key("searchCategoryOrder").access, Access::Free);
    for name in [
        "searchIndexRange",
        "searchCategories",
        "searchCategoryOrder",
    ] {
        let k = key(name);
        assert!(!k.title.is_empty() && !k.about.is_empty() && !k.why.is_empty());
        assert!(!k.from_awake);
    }
    assert_eq!(
        key("searchIndexRange").about,
        "Changes how far back sessions are searched. A shorter range removes older sessions from search; a longer one indexes them in the background."
    );
    assert_eq!(
        key("searchCategories").about,
        "Turns a search category on or off. Off removes it from search and from the index, for agents too."
    );
}

#[test]
fn defaults_are_the_specs() {
    let range = key("searchIndexRange");
    assert_eq!(range.default_value(), Some(json!("all")));
    assert_eq!(range.allowed, ["14d", "30d", "all"]);
    assert_eq!(
        key("searchCategories").default_value(),
        Some(json!(["sessions", "settings", "pages", "inbox", "actions"]))
    );
    assert_eq!(
        key("searchCategoryOrder").default_value(),
        Some(json!(EIGHT))
    );
}

#[test]
fn categories_refuse_unknown_and_duplicate_ids() {
    for name in ["searchCategories", "searchCategoryOrder"] {
        let k = key(name);
        assert_eq!(k.allowed, EIGHT);
        assert!(k.accepts(&json!(["pages", "sessions"])));
        assert!(k.accepts(&json!([])));
        assert!(!k.accepts(&json!(["pages", "bogus"])));
        assert!(!k.accepts(&json!(["pages", "pages"])));
        assert!(!k.accepts(&json!([1])));
    }
    assert!(key("searchIndexRange").accepts(&json!("30d")));
    assert!(!key("searchIndexRange").accepts(&json!("7d")));
}
