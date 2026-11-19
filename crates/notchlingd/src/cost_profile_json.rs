use notchling_engine::catalog::Tool;
use notchling_engine::cost_profile::manifest::Part;
use notchling_engine::cost_profile::model::{tool_key, HABIT_KEYS};
use notchling_engine::cost_profile::presets::data;
use notchling_engine::cost_profile::resolve::resolve;
use notchling_engine::cost_profile::{
    Action, ActionResult, Available, Plan, Profile, Role, Row, UndoResult,
};
use notchling_local_api::Tier;
use serde_json::{json, Map, Value};

pub fn table(profile: &Profile, avail: &Available) -> Value {
    let mut roles = Map::new();
    for role in Role::ALL.iter().copied() {
        let mut row = Map::new();
        for tool in Tool::ALL.iter().copied() {
            let cell = resolve(profile, role, tool, avail).map_or(Value::Null, |r| {
                json!({
                    "model": r.model,
                    "effort": r.effort,
                    "own": r.own,
                    "notes": r.notes.iter().map(|n| json!({"code": n.code, "detail": n.detail})).collect::<Vec<_>>(),
                })
            });
            row.insert(tool_key(tool).to_string(), cell);
        }
        roles.insert(role.name().to_string(), Value::Object(row));
    }
    Value::Object(roles)
}

pub fn profile(profile: &Profile, avail: &Available, tier: Tier) -> Value {
    let habits = profile.habits();
    let mut numbers = Map::new();
    for key in HABIT_KEYS {
        numbers.insert(key.to_string(), json!(habits.get(key)));
    }
    numbers.insert("compactWindow".into(), json!(habits.compact_window));
    let presets: Map<String, Value> = data()
        .preset_names()
        .into_iter()
        .map(|name| (name.to_string(), table(&Profile::preset(name), avail)))
        .collect();
    let mut out = json!({
        "preset": profile.preset,
        "custom": profile.is_custom(),
        "tools": Tool::ALL.iter().map(|t| (tool_key(*t).to_string(), json!(profile.is_on(*t)))).collect::<Map<_, _>>(),
        "picks": profile.picks.iter().map(|((r, t), c)| (format!("{}.{}", r.name(), tool_key(*t)), json!(c.to_string()))).collect::<Map<_, _>>(),
        "habits": numbers,
        "table": table(profile, avail),
        "presets": presets,
        "floors": data().floors.iter().map(|(r, f)| (r.name().to_string(), json!(format!("{} {}", f.model, f.effort)))).collect::<Map<_, _>>(),
    });
    if tier == Tier::App {
        out["guards"] = json!(profile.guards);
        let one = |i: &notchling_engine::cost_profile::model::Imported| {
            json!({
                "source": i.source,
                "kept": i.kept,
                "report": i.report.iter().map(|r| json!({"line": r.number, "text": r.text, "fate": r.fate})).collect::<Vec<_>>(),
            })
        };
        out["imported"] = profile.imports.first().map_or(Value::Null, one);
        out["imports"] = Value::Array(profile.imports.iter().map(one).collect());
    }
    out
}

pub fn action(a: &Action, tier: Tier) -> Value {
    let hide = tier != Tier::App && a.part == Part::Keys;
    json!({
        "id": a.id,
        "tool": tool_key(a.tool),
        "path": a.path.to_string_lossy(),
        "part": a.part.as_str(),
        "kind": a.kind.as_str(),
        "state": a.state.as_str(),
        "needs": a.needs,
        "reason": a.reason,
        "diff": if hide { "" } else { a.diff.as_str() },
        "keys": a.keys.iter().map(|k| json!({"key": k.path.join("."), "before": k.before, "after": k.after})).collect::<Vec<_>>(),
        "notes": a.notes,
    })
}

pub fn plan(plan: &Plan, avail: &Available, tier: Tier) -> Value {
    let d = &plan.detected;
    json!({
        "changes": plan.changes(),
        "actions": plan.actions.iter().map(|a| action(a, tier)).collect::<Vec<_>>(),
        "report": plan.report.iter().map(|r| json!({"line": r.number, "text": r.text, "fate": r.fate})).collect::<Vec<_>>(),
        "detected": {
            "claude": d.claude,
            "codex": d.codex,
            "antigravity": d.antigravity,
            "pstack": d.pstack,
            "orchestrator": d.orchestrator.is_some(),
            "ownRules": d.own_rules.iter().map(|p| p.to_string_lossy()).collect::<Vec<_>>(),
        },
        "profile": profile(&plan.profile, avail, tier),
    })
}

pub fn done(r: &ActionResult) -> Value {
    json!({
        "id": r.id,
        "path": r.path.to_string_lossy(),
        "kind": r.kind.as_str(),
        "done": r.done,
        "backup": r.backup.as_ref().map(|b| b.to_string_lossy()),
        "error": r.error,
    })
}

pub fn undone(r: &UndoResult) -> Value {
    json!({
        "id": r.id,
        "path": r.path.to_string_lossy(),
        "outcome": r.outcome.as_str(),
        "detail": r.detail,
    })
}

pub fn row(r: &Row) -> Value {
    json!({
        "id": r.id,
        "tool": tool_key(r.tool),
        "path": r.path.to_string_lossy(),
        "state": r.state,
        "detail": r.detail,
    })
}
