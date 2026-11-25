use serde_json::{json, Value};

#[cfg(test)]
use super::consent_card::Env;

pub(crate) const NOW: i64 = 1_794_096_000_000;

#[cfg(test)]
pub(crate) fn env() -> Env {
    Env {
        now_ms: NOW,
        vault_on: true,
        shown_ms: 5_000,
        ..Env::default()
    }
}

pub(crate) fn family(kind: &str, change: Value, facts: Value, parts: Value) -> Value {
    let mut view = json!({
        "id": "c20", "kind": kind, "target": "t", "state": "waiting",
        "change": change, "facts": facts,
        "tool": "claude", "verified": true, "session": "0b1c2d3e4f5a",
        "expiresMs": NOW + 14 * 60_000, "appliedBy": "app", "answerOn": "mac"
    });
    if !parts.is_null() {
        view["parts"] = parts;
    }
    view
}

pub(crate) fn update_view() -> Value {
    let args = json!({"tool": "codex"});
    family(
        "update",
        json!({"action": "update", "tool": "codex", "args": args,
               "apply": {"op": "versions.update_local", "args": args}}),
        json!([
            {"line": "consent-update-tool",
             "args": {"tool": "Codex", "method": "Homebrew", "from": "0.9.1", "to": "0.9.4"}},
            {"line": "consent-update-running", "args": {}}
        ]),
        Value::Null,
    )
}

#[cfg(test)]
pub(crate) fn model_view() -> Value {
    let args = json!({"model": "granite-97m-r2@835ad14/p1"});
    family(
        "model",
        json!({"action": "download", "model": "granite-97m-r2@835ad14/p1", "args": args,
               "apply": {"op": "embed.fetch", "args": args}}),
        json!([
            {"line": "consent-model-download",
             "args": {"model": "Standard", "download": "195", "kept": "195"}}
        ]),
        Value::Null,
    )
}

pub(crate) fn install_view() -> Value {
    family(
        "install",
        json!({"action": "apply", "apply": {"op": "install.apply", "args": {"what": ["hooks"]}}}),
        json!([{"line": "consent-install-backup", "args": {}}]),
        json!([
            {"id": "hooks", "path": "/home/u/.claude/settings.json", "tool": "claude",
             "change": "update", "added": 4, "removed": 1, "before": "aaaa1111bbbb", "after": "cccc2222dddd"},
            {"id": "skill:claude:notchling-vault", "path": "/home/u/.claude/skills/notchling-vault/SKILL.md",
             "tool": "claude", "change": "create", "added": 30, "removed": 0, "before": null, "after": "eeee3333ffff"}
        ]),
    )
}

pub(crate) fn cost_view() -> Value {
    family(
        "cost_profile",
        json!({"action": "apply", "apply": {"op": "cost.setup.apply", "args": {"consent": ["a"]}}}),
        json!([{"line": "consent-cost-backup", "args": {}}]),
        json!([
            {"id": "claude-settings", "path": "/home/u/.claude/settings.json", "tool": "claude",
             "change": "update", "added": 2, "removed": 2, "before": "1111", "after": "2222"},
            {"id": "profile", "path": "/home/u/.notchling/settings.json", "tool": "notchling",
             "change": "update", "before": "3333", "after": "4444",
             "fields": [{"field": "preset", "before": "Balanced", "after": "Lean"}]}
        ]),
    )
}

pub(crate) fn machine_view(admin: bool) -> Value {
    let args = json!({"machine": "m1"});
    let need = if admin { "password" } else { "none" };
    family(
        "machine",
        json!({"action": "fix", "admin": need, "organization": "Personal",
               "apply": {"op": "machines.apply", "args": args}}),
        json!([
            {"line": "consent-machine",
             "args": {"machine": "m1", "user": "maker", "host": "m1.ts.net", "organization": "Personal",
                      "addresses": "m1.ts.net, 10.0.0.4", "key": "id_ed25519"}},
            {"line": "consent-machine-step", "args": {"title": "Install the tool", "display": "brew install codex"}},
            {"line": "consent-machine-admin", "args": {"need": need}}
        ]),
        Value::Null,
    )
}

pub(crate) fn machine_add_view() -> Value {
    let args = json!({"machine": "m1"});
    family(
        "machine",
        json!({"action": "add", "args": args, "apply": {"op": "machines.apply", "args": args}}),
        json!([
            {"line": "consent-machine",
             "args": {"machine": "m1", "user": "maker", "host": "m1.ts.net", "organization": "Personal",
                      "addresses": "m1.ts.net", "key": ""}},
            {"line": "consent-machine-add", "args": {"machine": "m1"}}
        ]),
        Value::Null,
    )
}

pub(crate) fn network_view() -> Value {
    let args = json!({"tailnet": "work.ts.net"});
    family(
        "machine",
        json!({"action": "network", "args": args, "apply": {"op": "tailscale.switch", "args": args}}),
        json!([
            {"line": "consent-network-switch", "args": {"from": "h***@gmail.com", "to": "w***@gmail.com"}},
            {"line": "consent-network-drops", "args": {"count": 2, "groups": [{"organization": "Personal", "count": 2}]}},
            {"line": "consent-network-cluster", "args": {"members": 1, "failover": "off"}},
            {"line": "consent-network-phone", "args": {}}
        ]),
        Value::Null,
    )
}
