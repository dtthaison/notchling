//! The calls' shapes and the readers over the module docs' examples.

use super::*;

#[test]
fn the_calls_have_the_module_docs_shapes() {
    assert_eq!(Call::accounts_list(true).args, json!({"sync": true}));
    assert_eq!(Call::accounts_switch("a1").args, json!({"id": "a1"}));
    assert_eq!(
        Call::accounts_add_finish("l1", true).args,
        json!({"login": "l1", "cancel": true})
    );
    assert_eq!(
        Call::accounts_add_finish("l1", false).args,
        json!({"login": "l1"})
    );
    assert_eq!(
        Call::accounts_move_all(None, true).args,
        json!({"target": null, "dryRun": true})
    );
    assert_eq!(
        Call::providers_switch("codex", "u:a", true).args,
        json!({"provider": "codex", "id": "u:a", "confirm": true})
    );
    assert_eq!(
        Call::providers_add_begin("antigravity", false).args,
        json!({"provider": "antigravity"})
    );
    assert_eq!(
        Call::providers_usage("codex", false).args,
        json!({"provider": "codex", "refresh": false})
    );
    assert_eq!(Call::limits_status().op, "limits.status");
    assert_eq!(Call::cost_summary("all").args, json!({"range": "all"}));
}

#[test]
fn the_claude_list_and_lanes_are_read() {
    let list = parse_claude_list(&json!({"on": true, "active": "a1", "accounts": [
        {"id": "a1", "email": "me@example.com", "plan": "Max 20x", "needsLogin": false, "lane": null},
        {"id": "a2", "email": "ops@example.com", "needsLogin": true, "lane": "night"},
        {"email": "no id"}
    ]}));
    assert!(list.on);
    assert_eq!(list.active.as_deref(), Some("a1"));
    assert_eq!(list.accounts.len(), 2);
    assert_eq!(list.accounts[0].plan.as_deref(), Some("Max 20x"));
    assert!(list.accounts[1].needs_login);
    assert_eq!(list.accounts[1].lane.as_deref(), Some("night"));
    let lanes = parse_lanes(
        &json!({"lanes": [{"name": "night", "path": "/h/.claude-lanes/night",
        "accountID": "a2", "email": "ops@example.com", "folder": "/h/.claude-lanes/night"}]}),
    );
    assert_eq!(lanes[0].email.as_deref(), Some("ops@example.com"));
    assert_eq!(parse_claude_list(&Value::Null), ClaudeList::default());
}

#[test]
fn providers_usage_limits_and_cost_are_read() {
    let p = parse_providers(&json!({"accounts_on": false, "providers": [
        {"id": "codex", "title": "Codex", "capabilities": ["addAccount", "usage"], "available": true,
         "state": {"accounts": [{"id": "u:a", "email": "me@example.com", "plan": "Plus",
                   "needs_login": false, "switchable": true}],
                   "active_id": null, "busy": false, "adding": true, "sign_in_url": "https://x",
                   "unlisted_active_title": null}}]}));
    assert!(!p.accounts_on);
    let codex = &p.providers[0];
    assert!(codex.can("usage") && !codex.can("cost"));
    assert!(codex.adding && codex.active_id.is_none());
    assert_eq!(codex.accounts[0].plan.as_deref(), Some("Plus"));
    let usage = parse_usage(
        &json!({"provider": "codex", "reports": {"u:a": {"fetched": 1.0,
        "groups": [{"name": "Codex", "bars": [{"label": "5h", "kind": "session", "percent": 41.0,
        "resets_at": 2.0}]}]}}}),
    );
    assert_eq!(usage["u:a"].groups[0].bars[0].percent, 41.0);
    let limits = parse_limits(
        &json!({"accounts_on": true, "watching": true, "switch_at": 90,
        "providers": [{"id": "claude", "label": "Claude Code", "mode": "notify",
        "modes": ["off", "notify"], "registered": true, "active": "me@example.com",
        "windows": [{"name": "5-hour", "percent": 41.0, "resets_at": 3.0}], "checked": 4.0}]}),
    );
    assert_eq!(limits.switch_at, 90);
    assert_eq!(limits.providers[0].windows[0].name, "5-hour");
    let cost = parse_cost(&json!({"range": "7", "scanned": null, "scanning": true,
        "summary": {"days": [{"day": "2026-11-08", "cost": 1.5}], "today": 1.5, "week": 9.0,
                    "month": 31.0, "models": [], "model_detail": [], "projects": [],
                    "accounts": [], "channels": [], "channel_tokens": {}},
        "by_account": {}}));
    assert!(cost.scanning && cost.scanned.is_none());
    assert_eq!(cost.summary.week, 9.0);
    assert_eq!(cost.range, "7");
}

#[test]
fn outcomes_sign_ins_and_move_plans_are_read() {
    assert_eq!(
        parse_outcome(
            &json!({"state": "needs_confirmation", "confirmation": {"title": "Restart Antigravity?",
            "message": "Antigravity quits.", "button": "Restart"}})
        ),
        Outcome::NeedsConfirmation {
            title: "Restart Antigravity?".into(),
            message: "Antigravity quits.".into(),
            button: "Restart".into()
        }
    );
    assert_eq!(
        parse_outcome(
            &json!({"state": "waiting", "provider": "codex", "sign_in_url": "https://x"})
        ),
        Outcome::Done {
            state: "waiting".into(),
            sign_in_url: Some("https://x".into())
        }
    );
    let add = parse_add(
        &json!({"login": "l1", "state": "added", "account": {"email": "me@example.com"}}),
    );
    assert_eq!(add.unwrap().email.as_deref(), Some("me@example.com"));
    assert_eq!(parse_add(&json!({"state": "waiting"})), None);
    let plan = parse_move_plan(&json!({"plan": {"jobs": ["j1", "j2"], "closeLane": "night",
        "switchMain": true, "signedOut": ["t1"]}}));
    assert_eq!(plan.jobs, 2);
    assert_eq!(plan.close_lane.as_deref(), Some("night"));
    assert!(plan.switch_main);
    assert_eq!(parse_move_plan(&json!({"plan": {"jobs": 3}})).jobs, 3);
}
