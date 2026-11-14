use super::id::{fingerprint, qualify, tool_of, NodeId, Qualified, Tool};
use crate::versions;

#[test]
fn node_id_is_first_16_of_fingerprint() {
    let fp = fingerprint(b"a certificate");
    assert_eq!(fp.len(), 64);
    let id = NodeId::from_fingerprint(&fp).expect("id");
    assert_eq!(id.as_str(), &fp[..16]);
    assert_eq!(id.to_string(), fp[..16]);
}

#[test]
fn rejects_uppercase_or_short_fingerprint() {
    let fp = fingerprint(b"x");
    assert!(NodeId::from_fingerprint(&fp.to_uppercase()).is_none());
    assert!(NodeId::from_fingerprint(&fp[..63]).is_none());
    assert!(NodeId::from_fingerprint(&format!("{fp}0")).is_none());
    assert!(NodeId::parse("0123456789abcdeg").is_none());
}

#[test]
fn qualified_splits_at_first_slash() {
    let q = Qualified::parse("0123456789abcdef/codex:a/b").expect("qualified");
    assert_eq!(q.node.as_str(), "0123456789abcdef");
    assert_eq!(q.id, "codex:a/b");
    assert_eq!(qualify(&q.node, &q.id), "0123456789abcdef/codex:a/b");
}

#[test]
fn bare_uuid_is_not_qualified() {
    assert!(Qualified::parse("9b1deb4d-3b7d-4bad-9bdd-2b0d7b3dcb6d").is_none());
    assert!(Qualified::parse("0123456789abcdef/").is_none());
    assert!(Qualified::parse("0123456789ABCDEF/x").is_none());
}

#[test]
fn tool_of_maps_each_tool_and_shell_has_none() {
    assert_eq!(tool_of(Tool::Claude), Some(versions::Tool::Claude));
    assert_eq!(tool_of(Tool::Codex), Some(versions::Tool::Codex));
    assert_eq!(tool_of(Tool::Agy), Some(versions::Tool::Agy));
    assert_eq!(tool_of(Tool::Shell), None);
}

#[test]
fn node_id_rejects_a_bad_string_on_deserialize() {
    assert!(serde_json::from_str::<NodeId>("\"nope\"").is_err());
}
