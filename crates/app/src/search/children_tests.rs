use super::wire::{parse_results, parse_session, SessionRef};
use notchling_engine::search::SessionTool;
use serde_json::json;

fn hit(parent: serde_json::Value) -> serde_json::Value {
    json!({
        "key": "codex:0199c0de-child",
        "tool": "codex",
        "session_id": "0199c0de-child",
        "title": "Count the narwhal tusks",
        "folder": "/work/app",
        "parent": parent,
        "modified": 1_800_000_100,
        "snippet": "Four tusks.",
        "highlights": [[0, 4]]
    })
}

#[test]
fn a_child_hit_opens_its_parent() {
    let row = parse_session(&hit(json!("codex:0199a1b2-parent"))).expect("a row");
    assert_eq!(row.id, "codex:0199c0de-child", "it is still its own hit");
    assert_eq!(row.parent.as_deref(), Some("codex:0199a1b2-parent"));
    assert_eq!(
        row.reference(),
        SessionRef {
            tool: SessionTool::Codex,
            session_id: "0199a1b2-parent".into()
        }
    );
}

#[test]
fn a_hit_without_a_parent_opens_itself() {
    for none in [json!(null), json!("")] {
        let row = parse_session(&hit(none)).expect("a row");
        assert_eq!(row.parent, None);
        assert_eq!(
            row.reference(),
            SessionRef {
                tool: SessionTool::Codex,
                session_id: "0199c0de-child".into()
            }
        );
    }
    let mut old = hit(json!(null));
    old.as_object_mut().expect("an object").remove("parent");
    assert_eq!(parse_session(&old).and_then(|r| r.parent), None);
}

#[test]
fn a_child_and_its_parent_both_stay_in_the_results() {
    let results = parse_results(&json!({
        "pages": [],
        "sessions": [
            hit(json!("codex:0199a1b2-parent")),
            {"key": "codex:0199a1b2-parent", "tool": "codex", "session_id": "0199a1b2-parent", "title": "Plan", "folder": "/work/app", "modified": 1_800_000_000}
        ]
    }));
    let ids: Vec<&str> = results.sessions.iter().map(|s| s.id.as_str()).collect();
    assert_eq!(ids, ["codex:0199c0de-child", "codex:0199a1b2-parent"]);
}
