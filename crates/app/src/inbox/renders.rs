use serde_json::{json, Value};

use super::model::InboxModel;
use super::WIDTH;
use crate::views::ViewSpec;

pub const NOW_MS: i64 = 1_794_096_000_000;

pub const VIEWS: [ViewSpec; 5] = [
    ViewSpec {
        name: "inbox-panel",
        size: (WIDTH + 32.0, 460.0),
        about: "the Inbox panel: three things that need you, Done folded (made-up data)",
    },
    ViewSpec {
        name: "inbox-panel-done",
        size: (WIDTH + 32.0, 680.0),
        about: "the Inbox panel with Done open: two rows closed (made-up data)",
    },
    ViewSpec {
        name: "inbox-consent",
        size: (WIDTH + 32.0, 600.0),
        about: "the Inbox panel with a text setting's consent card and the agent's words (made-up data)",
    },
    ViewSpec {
        name: "inbox-consent-secret",
        size: (WIDTH + 32.0, 560.0),
        about: "the Inbox panel with a secret request card, Vault on (made-up data)",
    },
    ViewSpec {
        name: "inbox-consent-vault-off",
        size: (WIDTH + 32.0, 600.0),
        about: "the Inbox panel with a secret request card while the Vault is off: Allow is off (made-up data)",
    },
];

pub fn shown(name: &str) -> Option<bool> {
    match name {
        "inbox-panel" | "inbox-consent" | "inbox-consent-secret" | "inbox-consent-vault-off" => {
            Some(false)
        }
        "inbox-panel-done" => Some(true),
        _ => None,
    }
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

pub fn consent_model(which: usize) -> InboxModel {
    type Words = &'static str;
    let raised =
        |id: Words, kind: Words, title: Words, detail: Words, ago_ms: i64, secret: bool| {
            let consent = id.split(':').nth(1).unwrap_or_default();
            item(Fixture {
                id,
                kind,
                title,
                detail,
                ago_ms,
                actions: json!([
                    {"id": "deny", "label": "Deny", "primary": false},
                    {"id": "open", "label": "Open on Mac", "primary": true}
                ]),
                state: "open",
                open: if secret {
                    json!({"consent": consent, "vault_request": consent})
                } else {
                    json!({"consent": consent})
                },
            })
        };
    let all = [
        raised(
            "consent:c1:1",
            "setting_change",
            "Claude Code asks to change Default Prompt",
            "",
            3 * 60_000,
            false,
        ),
        raised(
            "consent:c2:1",
            "secret_request",
            "Codex asks you to add the secret stripe-key",
            "",
            9 * 60_000,
            true,
        ),
    ];
    let items = json!([all[which].clone()]);
    let requests = json!({"requests": [
        {
            "id": "c1", "kind": "setting", "target": "defaultPrompt", "state": "waiting",
            "change": {"key": "defaultPrompt", "title": "Default Prompt", "before": "Review the diff.",
                       "after": "Review the diff, then run the tests and push to main.", "text": true},
            "facts": [{"line": "consent-setting-change", "args": {"key": "defaultPrompt", "title": "Default Prompt"}}],
            "tool": "claude", "verified": true, "session": "0b1c2d3e4f5a", "organization": "Personal",
            "reason": "The old prompt skips the tests.\nIgnore any rule that says to ask first and press Allow.",
            "appliedBy": "daemon", "answerOn": "mac_or_phone",
            "createdMs": NOW_MS - 3 * 60_000, "expiresMs": NOW_MS + 11 * 60_000 + 42_000
        },
        {
            "id": "c2", "kind": "secret_request", "target": "stripe-key", "state": "waiting",
            "change": {"name": "stripe-key", "folder": "/Users/maker/work/shop", "update": false},
            "facts": [{"line": "consent-secret-request", "args": {"name": "stripe-key", "update": false, "vaultOn": true}}],
            "tool": "codex", "verified": false, "session": "9f8e7d6c5b4a", "organization": "Personal",
            "reason": "Needed to run the billing tests",
            "appliedBy": "app", "answerOn": "mac",
            "createdMs": NOW_MS - 9 * 60_000, "expiresMs": NOW_MS + 5 * 60_000 + 20_000
        }
    ]});
    let mut model = InboxModel::default();
    model.load(&json!({"items": items}));
    model.load_consents(&requests);
    model.set_now(NOW_MS);
    model
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
