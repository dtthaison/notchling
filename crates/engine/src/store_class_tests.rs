use super::{doc_class, table_class, Class};
use crate::accounts::antigravity::store::USAGE_DOC;
use crate::accounts::book::{ANTIGRAVITY_DOC, CODEX_DOC, IDS_DOC};
use crate::accounts::claude::LIST_DOC;
use crate::cost::cache::CACHE_DOC;
use crate::delegate::second_opinion_store::DOC as SECOND_OPINIONS_DOC;
use crate::limits::mode::LIMITS_DOC;
use crate::machines::offers::OFFERS_DOC;
use crate::machines::registry::REGISTRY_DOC;
use crate::sessions::testutil::TempDir;
use crate::store;
use crate::vault::list::INDEX_DOC;

#[test]
fn every_table_in_notchling_db_has_a_class() {
    let tmp = TempDir::new("store-class");
    let db = store::open(&tmp.path("data")).expect("open");
    let mut query = db
        .prepare("SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%'")
        .expect("query");
    let names: Vec<String> = query
        .query_map([], |r| r.get(0))
        .expect("rows")
        .collect::<Result<_, _>>()
        .expect("names");
    assert!(names.len() >= 5, "{names:?}");
    for name in names {
        assert!(
            table_class(&name).is_some(),
            "table {name} in notchling.db has no class: add it to store_class"
        );
    }
}

#[test]
fn inbox_is_the_only_cluster_table() {
    assert_eq!(table_class("inbox"), Some(Class::Cluster));
    for name in ["meta", "settings", "drafts", "docs"] {
        assert_eq!(table_class(name), Some(Class::Node), "{name}");
    }
    assert_eq!(table_class("nonsense"), None);
}

#[test]
fn every_doc_constant_has_a_class() {
    for name in [
        LIMITS_DOC,
        CACHE_DOC,
        CODEX_DOC,
        ANTIGRAVITY_DOC,
        IDS_DOC,
        SECOND_OPINIONS_DOC,
        LIST_DOC,
        USAGE_DOC,
        OFFERS_DOC,
        REGISTRY_DOC,
        INDEX_DOC,
    ] {
        assert_eq!(doc_class(name), Some(Class::Node), "{name}");
    }
    assert_eq!(doc_class("daemon/search"), None);
}
