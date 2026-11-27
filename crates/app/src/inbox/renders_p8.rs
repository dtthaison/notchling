use serde_json::{json, Value};

use super::family_fixtures::{
    cost_view, install_view, machine_add_view, machine_view, network_view, update_view,
};
use super::model::InboxModel;
use super::renders::NOW_MS;
use super::WIDTH;
use crate::views::ViewSpec;

pub const VIEWS: [ViewSpec; 6] = [
    ViewSpec {
        name: "inbox-consent-machine-add",
        size: (WIDTH + 32.0, 560.0),
        about: "the Inbox panel with a request to add a machine (made-up data)",
    },
    ViewSpec {
        name: "inbox-consent-machine-fix",
        size: (WIDTH + 32.0, 700.0),
        about: "the Inbox panel with a request to fix a machine that needs its admin password: Allow is off until one is given (made-up data)",
    },
    ViewSpec {
        name: "inbox-consent-network",
        size: (WIDTH + 32.0, 600.0),
        about: "the Inbox panel with a request to switch the Tailscale network, with what drops (made-up data)",
    },
    ViewSpec {
        name: "inbox-consent-install",
        size: (WIDTH + 32.0, 700.0),
        about: "the Inbox panel with a request to install hooks and skills, one ticked row for each file (made-up data)",
    },
    ViewSpec {
        name: "inbox-consent-cost",
        size: (WIDTH + 32.0, 700.0),
        about: "the Inbox panel with a request to apply the cost profile: file rows and the profile row (made-up data)",
    },
    ViewSpec {
        name: "inbox-consent-update",
        size: (WIDTH + 32.0, 560.0),
        about: "the Inbox panel with a request to update a tool (made-up data)",
    },
];

pub fn has(name: &str) -> bool {
    VIEWS.iter().any(|v| v.name == name)
}

fn raised(title: &str) -> Value {
    json!({
        "id": "consent:c20:1", "kind": "consent", "title": title, "detail": "",
        "source": {}, "created_ms": NOW_MS - 2 * 60_000, "closed_ms": null,
        "expires_ms": null,
        "actions": [
            {"id": "deny", "label": "Deny", "primary": false},
            {"id": "open", "label": "Open on Mac", "primary": true}
        ],
        "state": "open", "answer": null, "answered_by": null, "seen": false,
        "open": {"consent": "c20"}
    })
}

pub fn model(name: &str) -> InboxModel {
    let (title, mut view) = match name {
        "inbox-consent-machine-add" => ("Claude Code asks to add a machine", machine_add_view()),
        "inbox-consent-machine-fix" => ("Claude Code asks to set up a machine", machine_view(true)),
        "inbox-consent-network" => (
            "Claude Code asks to switch the Tailscale network",
            network_view(),
        ),
        "inbox-consent-install" => (
            "Claude Code asks to install hooks and skills",
            install_view(),
        ),
        "inbox-consent-cost" => ("Claude Code asks to apply the cost profile", cost_view()),
        _ => ("Claude Code asks to update Codex", update_view()),
    };
    view["createdMs"] = json!(NOW_MS - 2 * 60_000);
    view["expiresMs"] = json!(NOW_MS + 13 * 60_000);
    view["organization"] = json!("Personal");
    if name != "inbox-consent-machine-fix" {
        view["folder"] = json!("/Users/maker/work/notchling");
        view["reason"] = json!("Part of the setup you asked for.");
    }
    let mut model = InboxModel::default();
    model.load(&json!({"items": [raised(title)]}));
    model.load_consents(&json!({"requests": [view]}));
    model.set_now(NOW_MS);
    model
}
