#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Class {
    Node,
    Cluster,
}

pub fn table_class(name: &str) -> Option<Class> {
    match name {
        "meta" | "settings" | "drafts" | "docs" => Some(Class::Node),
        "inbox" => Some(Class::Cluster),
        _ => None,
    }
}

pub fn doc_class(name: &str) -> Option<Class> {
    match name {
        "daemon/limits"
        | "daemon/cost-cache"
        | "accounts/codex"
        | "accounts/antigravity"
        | "accounts/ids"
        | "app/second-opinions"
        | "daemon/accounts"
        | "accounts/antigravity-usage"
        | "machines/pending-updates"
        | "machines/registry"
        | "daemon/vault" => Some(Class::Node),
        _ => None,
    }
}

#[cfg(test)]
#[path = "store_class_tests.rs"]
mod store_class_tests;
