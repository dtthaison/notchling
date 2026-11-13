use serde_json::{json, Value};

use super::model::InboxModel;
use super::WIDTH;
use crate::views::ViewSpec;

pub const NOW_MS: i64 = 1_794_096_000_000;

pub const VIEWS: [ViewSpec; 1] = [ViewSpec {
    name: "inbox-panel",
    size: (WIDTH + 32.0, 680.0),
    about: "the Inbox panel: three things that need you, two done (made-up data)",
}];

pub fn shown(name: &str) -> bool {
    name == "inbox-panel"
}

struct Fixture {
    id: &'static str,
    kind: &'static str,
    title: &'static str,
    detail: &'static str,
    ago_ms: i64,
    actions: Value,
    state: &'static str,
    open: Value,
}

fn item(f: Fixture) -> Value {
    let closed = if f.state == "open" {
        Value::Null
    } else {
        json!(NOW_MS - f.ago_ms / 2)
    };
    let answer = if f.state == "answered" {
        json!("allow_once")
    } else {
        Value::Null
    };
    json!({
        "id": f.id,
        "kind": f.kind,
        "title": f.title,
        "detail": f.detail,
        "source": {},
        "created_ms": NOW_MS - f.ago_ms,
        "closed_ms": closed,
        "expires_ms": null,
        "actions": f.actions,
        "state": f.state,
        "answer": answer,
        "answered_by": null,
        "seen": f.state != "open",
        "open": f.open
    })
}

fn approval_actions() -> Value {
    json!([
        {"id": "allow_once", "label": "Allow Once", "primary": true},
        {"id": "allow_always", "label": "Always Allow", "primary": false},
        {"id": "deny", "label": "Deny", "primary": false}
    ])
}

pub fn model() -> InboxModel {
    let items = json!([
        item(Fixture {
            id: "a1",
            kind: "secret_approval",
            title: "Allow github, npm for gh api /user",
            detail: "/Users/maker/work/notchling",
            ago_ms: 2 * 60_000,
            actions: approval_actions(),
            state: "open",
            open: json!({"vault_approval": "a1"}),
        }),
        item(Fixture {
            id: "r1",
            kind: "secret_request",
            title: "Add secret stripe-key",
            detail: "Needed to run the billing tests",
            ago_ms: 9 * 60_000,
            actions: json!([
                {"id": "open", "label": "Open Form", "primary": true},
                {"id": "cancel", "label": "Cancel", "primary": false}
            ]),
            state: "open",
            open: json!({"vault_request": "r1"}),
        }),
        item(Fixture {
            id: "l1",
            kind: "limit_notice",
            title: "Claude Code is at 90% of its 5-hour limit",
            detail: "",
            ago_ms: 47 * 60_000,
            actions: json!([{"id": "done", "label": "Done", "primary": false}]),
            state: "open",
            open: Value::Null,
        }),
        item(Fixture {
            id: "a0",
            kind: "secret_approval",
            title: "Allow aws for terraform plan",
            detail: "/Users/maker/work/infra",
            ago_ms: 3 * 3_600_000,
            actions: approval_actions(),
            state: "answered",
            open: json!({"vault_approval": "a0"}),
        }),
        item(Fixture {
            id: "a2",
            kind: "secret_approval",
            title: "Allow npm for npm publish",
            detail: "/Users/maker/work/kit",
            ago_ms: 26 * 3_600_000,
            actions: approval_actions(),
            state: "expired",
            open: json!({"vault_approval": "a2"}),
        }),
    ]);
    let mut model = InboxModel::default();
    model.load(&json!({"items": items}));
    model.set_now(NOW_MS);
    model
}
