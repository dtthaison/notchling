use serde_json::{json, Value};

use super::model::InboxModel;
use super::renders::NOW_MS;
use super::triage::Section;
#[cfg(target_os = "macos")]
use super::view::InboxPanelView;
use super::WIDTH;
use crate::views::ViewSpec;

pub const VIEWS: [ViewSpec; 9] = [
    ViewSpec {
        name: "inbox-triage",
        size: (WIDTH + 32.0, 900.0),
        about: "the Inbox: the Blocking section on top, then the Waiting tab (made-up data)",
    },
    ViewSpec {
        name: "inbox-triage-running",
        size: (WIDTH + 32.0, 576.0),
        about: "the Inbox with the Running tab: an applying row and its pill (made-up data)",
    },
    ViewSpec {
        name: "inbox-triage-didnt-work",
        size: (WIDTH + 32.0, 576.0),
        about: "the Inbox with the Didn't Work tab: failed, withdrawn and expired rows with Clear (made-up data)",
    },
    ViewSpec {
        name: "inbox-triage-info",
        size: (WIDTH + 32.0, 576.0),
        about: "the Inbox with the Info tab: unseen notices with nothing to answer (made-up data)",
    },
    ViewSpec {
        name: "inbox-triage-done",
        size: (WIDTH + 32.0, 576.0),
        about: "the Inbox with the Done tab: answered rows, one denied with a note (made-up data)",
    },
    ViewSpec {
        name: "inbox-triage-calm",
        size: (WIDTH + 32.0, 360.0),
        about: "the Inbox when nothing blocks: the calm block and the last answer (made-up data)",
    },
    ViewSpec {
        name: "inbox-triage-change",
        size: (WIDTH + 32.0, 576.0),
        about: "the Inbox with Change… open on a blocking card: note field, hint, Send and Cancel (made-up data)",
    },
    ViewSpec {
        name: "inbox-triage-asked-again",
        size: (WIDTH + 32.0, 576.0),
        about: "the Inbox with a blocking card the agent asked again after a Change… note (made-up data)",
    },
    ViewSpec {
        name: "inbox-triage-error",
        size: (WIDTH + 32.0, 576.0),
        about: "the Inbox with a row error under the card that failed (made-up data)",
    },
];

pub fn has(name: &str) -> bool {
    VIEWS.iter().any(|v| v.name == name)
}

pub fn shown(name: &str) -> Option<Section> {
    match name {
        "inbox-triage"
        | "inbox-triage-change"
        | "inbox-triage-asked-again"
        | "inbox-triage-error"
        | "inbox-triage-calm" => Some(Section::Waiting),
        "inbox-triage-running" => Some(Section::Running),
        "inbox-triage-didnt-work" => Some(Section::DidntWork),
        "inbox-triage-info" => Some(Section::Info),
        "inbox-triage-done" => Some(Section::Done),
        _ => None,
    }
}

#[cfg(target_os = "macos")]
pub fn dress(name: &str, panel: InboxPanelView) -> InboxPanelView {
    match name {
        "inbox-triage-change" => panel.with_change(
            "a1",
            super::model::ChangeRoute::Approval("a1".to_owned()),
            "Use the read-only token, not the admin one.",
        ),
        "inbox-triage-error" => panel.with_error("a1", "The Vault did not answer. Try again."),
        _ => panel,
    }
}

fn ago(minutes: i64) -> i64 {
    NOW_MS - minutes * 60_000
}

fn blocking_approval() -> Value {
    json!({
        "id": "a1", "kind": "secret_approval",
        "title": "Allow github, npm for gh api /user",
        "detail": "/Users/maker/work/notchling",
        "source": {"session": "0b1c2d3e4f5a"}, "created_ms": ago(2), "closed_ms": null,
        "expires_ms": NOW_MS + 4 * 60_000 + 20_000,
        "actions": [
            {"id": "allow_once", "label": "Allow Once", "primary": true},
            {"id": "allow_always", "label": "Always Allow", "primary": false},
            {"id": "deny", "label": "Deny", "primary": false}
        ],
        "state": "open", "answer": null, "answered_by": null, "seen": false,
        "open": {"vault_approval": "a1"},
        "blocking": true, "waiting_since": ago(2), "waiter": "vault",
        "alert_actions": ["allow", "deny", "change"],
        "place": {"machine_name": "m1", "session_name": "build-fix", "branch": "agent/b2-3"}
    })
}

fn blocking_consent() -> Value {
    json!({
        "id": "consent:c1:1", "kind": "consent",
        "title": "Claude Code asks to change Default Prompt",
        "detail": "",
        "source": {}, "created_ms": ago(3), "closed_ms": null,
        "expires_ms": NOW_MS + 11 * 60_000,
        "actions": [
            {"id": "deny", "label": "Deny", "primary": false},
            {"id": "open", "label": "Open on Mac", "primary": true}
        ],
        "state": "open", "answer": null, "answered_by": null, "seen": false,
        "open": {"consent": "c1"},
        "blocking": true, "waiting_since": ago(3), "waiter": "cli",
        "alert_actions": ["allow", "deny", "change"],
        "place": {"machine_name": "mac-mini", "session_name": "docs", "branch": "main"}
    })
}

fn closed(id: &str, kind: &str, title: &str, minutes: i64, state: &str) -> Value {
    json!({
        "id": id, "kind": kind, "title": title, "detail": "",
        "source": {}, "created_ms": ago(minutes + 2), "closed_ms": ago(minutes),
        "expires_ms": null,
        "actions": [
            {"id": "allow_once", "label": "Allow Once", "primary": true},
            {"id": "deny", "label": "Deny", "primary": false}
        ],
        "state": state, "answer": if state == "answered" { json!("allow_once") } else { Value::Null },
        "answered_by": null, "seen": true, "open": {"vault_approval": id}
    })
}

fn others() -> Vec<Value> {
    let mut running = closed(
        "run1",
        "consent",
        "Switch to the work account",
        1,
        "answered",
    );
    running["outcome"] = json!({"state": "applying", "at_ms": ago(1)});
    running["place"] = json!({"machine_name": "m1", "session_name": "build-fix"});
    let mut failed = closed(
        "bad1",
        "consent",
        "Add a machine to the cluster",
        12,
        "answered",
    );
    failed["outcome"] = json!({"state": "failed", "reason": "m2 did not answer", "at_ms": ago(12)});
    let mut withdrawn = closed(
        "bad2",
        "consent",
        "Install hooks and skills",
        40,
        "answered",
    );
    withdrawn["outcome"] = json!({"state": "withdrawn", "at_ms": ago(40)});
    let expired = closed(
        "bad3",
        "secret_approval",
        "Allow npm for npm publish",
        90,
        "expired",
    );
    let mut denied = closed(
        "done1",
        "secret_approval",
        "Allow aws for terraform plan",
        30,
        "answered",
    );
    denied["answer"] = json!("deny");
    denied["note"] = json!("Use the staging role first.");
    let approved = closed(
        "done2",
        "secret_approval",
        "Allow github for git push",
        55,
        "answered",
    );
    let notice = json!({
        "id": "info1", "kind": "limit_notice",
        "title": "Claude Code is at 90% of its 5-hour limit", "detail": "",
        "source": {}, "created_ms": ago(47), "closed_ms": null, "expires_ms": null,
        "actions": [], "state": "open", "answer": null, "answered_by": null, "seen": false,
        "open": null
    });
    let notice2 = json!({
        "id": "info2", "kind": "version_drift",
        "title": "m2 runs Notchling 2.0.1, this Mac 2.0.2", "detail": "",
        "source": {}, "created_ms": ago(180), "closed_ms": null, "expires_ms": null,
        "actions": [], "state": "open", "answer": null, "answered_by": null, "seen": false,
        "open": null
    });
    let waiting = json!({
        "id": "r1", "kind": "secret_request", "title": "Add secret stripe-key",
        "detail": "Needed to run the billing tests",
        "source": {}, "created_ms": ago(9), "closed_ms": null, "expires_ms": null,
        "actions": [
            {"id": "open", "label": "Open Form", "primary": true},
            {"id": "cancel", "label": "Cancel", "primary": false}
        ],
        "state": "open", "answer": null, "answered_by": null, "seen": false,
        "open": {"vault_request": "r1"}
    });
    let waiting2 = json!({
        "id": "w2", "kind": "session_waits", "title": "build-fix is waiting for you",
        "detail": "",
        "source": {}, "created_ms": ago(25), "closed_ms": null, "expires_ms": null,
        "actions": [{"id": "open", "label": "Open Chat", "primary": true}],
        "state": "open", "answer": null, "answered_by": null, "seen": false,
        "open": {"session": "0b1c2d3e4f5a"},
        "place": {"machine_name": "m1", "session_name": "build-fix"}
    });
    vec![
        running, failed, withdrawn, expired, denied, approved, notice, notice2, waiting, waiting2,
    ]
}

pub fn model(name: &str) -> InboxModel {
    let mut items = others();
    match name {
        "inbox-triage-calm" => items.retain(|item| item["state"] != "open"),
        "inbox-triage" => {
            items.insert(0, blocking_approval());
            items.insert(0, blocking_consent());
        }
        "inbox-triage-asked-again" => items.insert(0, blocking_consent()),
        _ => items.insert(0, blocking_approval()),
    }
    let mut request = json!({
        "id": "c1", "kind": "setting", "target": "defaultPrompt", "state": "waiting",
        "change": {"key": "defaultPrompt", "title": "Default Prompt", "before": "Review the diff.",
                   "after": "Review the diff, then run the tests and push to main.", "text": true},
        "facts": [{"line": "consent-setting-change", "args": {"key": "defaultPrompt", "title": "Default Prompt"}}],
        "tool": "claude", "verified": true, "session": "0b1c2d3e4f5a", "organization": "Personal",
        "folder": "/Users/maker/work/notchling",
        "reason": "The old prompt skips the tests.",
        "appliedBy": "daemon", "answerOn": "mac_or_phone",
        "createdMs": ago(3), "expiresMs": NOW_MS + 11 * 60_000
    });
    if name == "inbox-triage-asked-again" {
        request["change"]["priorNote"] = json!("Keep the first line as it is.");
    }
    let mut model = InboxModel::default();
    model.load(&json!({"items": items}));
    model.load_consents(&json!({"requests": [request]}));
    model.set_now(NOW_MS);
    model
}

pub fn blocked_for(session: &str) -> InboxModel {
    let mut item = blocking_approval();
    item["place"]["session"] = json!(session);
    let mut model = InboxModel::default();
    model.load(&json!({"items": [item]}));
    model.set_now(NOW_MS);
    model
}
