use serde_json::{json, Map, Value};

use super::cache::Cache;
use super::check::CheckModel;
use super::picks::Options;
use super::profile::{Profile, TOOLS};
use super::setup::{SetupModel, Stage};
use crate::views::ViewSpec;

pub const HOME: &str = "/Users/me";

const DAMAGED: &str = "the setup record /Users/me/Library/Application Support/Notchling/daemon/cost-setup.json is damaged; nothing was changed. Move it aside to start over.";

pub const VIEWS: [ViewSpec; 9] = [
    ViewSpec {
        name: "settings-costs",
        size: (620.0, 900.0),
        about: "the Settings sheet's Models & Costs page on Balanced: the preset, the role table with floors, the habits and Set Up (made-up data)",
    },
    ViewSpec {
        name: "settings-costs-custom",
        size: (620.0, 1560.0),
        about: "the Models & Costs page after an edit (Custom) with the judge's Claude Code picker open: models and efforts under the floor are disabled (made-up data)",
    },
    ViewSpec {
        name: "cost-setup",
        size: (620.0, 1500.0),
        about: "the Set Up sheet: the tools found with versions, each planned file as New, Edit, Replace or Remove with one diff shown, unticked offers, the imported lines and Apply (made-up data)",
    },
    ViewSpec {
        name: "cost-setup-applied",
        size: (620.0, 760.0),
        about: "the Set Up sheet after Apply: what was written with its backups, Undo per tool and the conflicts of an Undo that found edited files (made-up data)",
    },
    ViewSpec {
        name: "settings-costs-check",
        size: (620.0, 760.0),
        about: "the Models & Costs page's Check tab: a dot and a state per file, with Keep Mine and Restore (made-up data)",
    },
    ViewSpec {
        name: "settings-costs-cache",
        size: (620.0, 620.0),
        about: "the Models & Costs page's Cache tab: Claude Code's cache times from settings.json, the other tools' notes and Keep Warm as Coming Later (made-up data)",
    },
    ViewSpec {
        name: "cost-setup-off",
        size: (620.0, 760.0),
        about: "the Set Up sheet while the install switch is off: the plan is shown and Apply is disabled with its note (made-up data)",
    },
    ViewSpec {
        name: "cost-setup-damaged",
        size: (620.0, 420.0),
        about: "the Set Up sheet when the setup record is damaged: the error and no Apply (made-up data)",
    },
    ViewSpec {
        name: "settings-costs-check-damaged",
        size: (620.0, 560.0),
        about: "the Models & Costs page's Check tab when the setup record is damaged: the error and no rows (made-up data)",
    },
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shown {
    Roles,
    Custom,
    Setup,
    Applied,
    Check,
    Cache,
    SetupOff,
    Damaged,
    CheckDamaged,
}

pub fn shown(name: &str) -> Option<Shown> {
    Some(match name {
        "settings-costs" => Shown::Roles,
        "settings-costs-custom" => Shown::Custom,
        "cost-setup" => Shown::Setup,
        "cost-setup-applied" => Shown::Applied,
        "settings-costs-check" => Shown::Check,
        "settings-costs-cache" => Shown::Cache,
        "cost-setup-off" => Shown::SetupOff,
        "cost-setup-damaged" => Shown::Damaged,
        "settings-costs-check-damaged" => Shown::CheckDamaged,
        _ => return None,
    })
}

const BALANCED: [(&str, [&str; 3]); 7] = [
    (
        "read",
        ["haiku medium", "gpt-6-luna medium", "gemini-4-hydrogen low"],
    ),
    (
        "code",
        [
            "sonnet high",
            "gpt-6-sol medium",
            "gemini-4-hydrogen medium",
        ],
    ),
    (
        "fix",
        ["sonnet xhigh", "gpt-6-sol high", "gemini-4-hydrogen high"],
    ),
    (
        "judge",
        ["opus xhigh", "gpt-6-astra xhigh", "gemini-4-argon high"],
    ),
    (
        "hardest",
        ["fable xhigh", "gpt-6-astra xhigh", "gemini-4-argon xhigh"],
    ),
    ("review", ["sonnet high", "-", "gemini-4-hydrogen high"]),
    ("image", ["-", "- low", "- low"]),
];

fn cell(text: &str, own: bool) -> Value {
    if text == "-" {
        return Value::Null;
    }
    let (model, effort) = text.split_once(' ').unwrap_or((text, "medium"));
    json!({
        "model": if model == "-" { Value::Null } else { json!(model) },
        "effort": effort,
        "own": own,
        "notes": [],
    })
}

fn imports() -> Value {
    json!([
        {"source": "/Users/me/.claude/rules/model-choice.md", "kept": [], "report": [
            {"line": 3, "text": "| haiku | Bulk reading, search, listing |", "fate": "profile:read"},
            {"line": 4, "text": "| sonnet | Routine code: specified edits |", "fate": "profile:code"},
            {"line": 9, "text": "- Try a stronger model at lower effort first.", "fate": "kept"},
            {"line": 12, "text": "| Model | Use for |", "fate": "regenerated"},
        ]},
        {"source": "/Users/me/.claude/rules/costs-old.md", "kept": [], "report": [
            {"line": 2, "text": "| opus | Judgment: design, review |", "fate": "profile:judge"},
            {"line": 6, "text": "- Keep helpers on five minutes.", "fate": "kept"},
        ]},
    ])
}

pub fn profile_reply(edited: bool) -> Value {
    let mut table = Map::new();
    for (role, picks) in BALANCED {
        let mut row = Map::new();
        for (i, tool) in TOOLS.iter().enumerate() {
            let mut text = picks[i].to_owned();
            let mut own = false;
            if edited && role == "code" && *tool == "claude" {
                text = "sonnet xhigh".to_owned();
                own = true;
            }
            if edited && role == "read" && *tool == "codex" {
                text = "gpt-6-sol medium".to_owned();
                own = true;
            }
            row.insert((*tool).to_owned(), cell(&text, own));
        }
        table.insert(role.to_owned(), Value::Object(row));
    }
    json!({"profile": {
        "preset": "balanced",
        "custom": edited,
        "tools": {"claude": true, "codex": true, "agy": true},
        "picks": {},
        "habits": {"handBackTurns": 80, "maxTurns": 140, "builderContext": 150000,
            "parallelCap": 5, "readLines": 150, "catLines": 300, "waitSecs": 270,
            "bashOutputMaxChars": 12000},
        "table": table,
        "presets": {},
        "floors": {"fix": "sonnet high", "judge": "opus high", "hardest": "opus xhigh"},
        "guards": {},
        "imported": imports()[0].clone(),
        "imports": imports(),
    }})
}

pub fn profile(shown: Shown) -> Profile {
    let edited = shown == Shown::Custom;
    Profile::from_reply(&profile_reply(edited)).unwrap_or_default()
}

fn catalog(models: &[(&str, &[&str])]) -> Value {
    json!({"models": models.iter().map(|(id, e)| json!({"id": id, "efforts": e})).collect::<Vec<_>>()})
}

pub fn options() -> Options {
    let claude = catalog(&[
        ("claude-haiku-5-5", &["low", "medium", "high"]),
        ("claude-sonnet-5-5", &["low", "medium", "high", "xhigh"]),
        (
            "claude-opus-5-5",
            &["low", "medium", "high", "xhigh", "max"],
        ),
        ("claude-fable-5-1", &["medium", "high", "xhigh", "max"]),
    ]);
    let codex = catalog(&[
        ("gpt-6-luna", &["low", "medium", "high"]),
        ("gpt-6-sol", &["low", "medium", "high", "xhigh"]),
        ("gpt-6-astra", &["medium", "high", "xhigh"]),
    ]);
    let agy = catalog(&[
        ("gemini-4-hydrogen", &["low", "medium", "high", "xhigh"]),
        ("gemini-4-argon", &["medium", "high", "xhigh"]),
    ]);
    Options::from_replies(Some(&claude), Some(&codex), Some(&agy))
}

fn action(id: &str, tool: &str, path: &str, part: &str, kind: &str) -> Value {
    json!({"id": id, "tool": tool, "path": path, "part": part, "kind": kind,
        "state": "fresh", "needs": [], "reason": null, "diff": "", "keys": [], "notes": []})
}

fn with(mut base: Value, extra: Value) -> Value {
    if let (Some(b), Some(e)) = (base.as_object_mut(), extra.as_object()) {
        for (k, v) in e {
            b.insert(k.clone(), v.clone());
        }
    }
    base
}

const RULE_DIFF: &str = "--- /Users/me/.claude/rules/notchling-cost.md\n+++ /Users/me/.claude/rules/notchling-cost.md\n@@ -0,0 +1,6 @@\n+<!-- notchling-cost v1 (Balanced). Written by Notchling. -->\n+# Models and work habits\n+| Role | Agent type | Model | Effort |\n+| read | notchling-reader | haiku | medium |\n+| code | notchling-builder | sonnet | high |\n+| fix | notchling-builder | sonnet | xhigh |\n";
const OLD_RULES_DIFF: &str = "--- /Users/me/.claude/rules/model-choice.md\n+++ /Users/me/.claude/rules/model-choice.md\n@@ -1,5 +1,3 @@\n # Choosing a subagent model\n-| haiku | Bulk reading |\n-| sonnet | Routine code |\n+Models per role: see notchling-cost.md (managed by Notchling)\n";

pub fn plan_reply(on: bool, ticked: bool) -> Value {
    let rule = with(
        action(
            "claude.rule",
            "claude",
            "/Users/me/.claude/rules/notchling-cost.md",
            "file",
            "create",
        ),
        json!({"diff": RULE_DIFF}),
    );
    let builder = action(
        "claude.agent.builder",
        "claude",
        "/Users/me/.claude/agents/notchling-builder.md",
        "file",
        "create",
    );
    let settings = with(
        action(
            "claude.settings",
            "claude",
            "/Users/me/.claude/settings.json",
            "keys",
            "update",
        ),
        json!({
            "diff": "--- /Users/me/.claude/settings.json\n+++ /Users/me/.claude/settings.json\n@@ -12,4 +12,5 @@\n-    \"autoCompactWindow\": 400000\n+    \"autoCompactWindow\": 250000\n+  \"bashOutputMaxChars\": 12000,\n",
            "keys": [
                {"key": "modelSettings.claude-sonnet-5-5.autoCompactWindow", "before": 400000, "after": 250000},
                {"key": "bashOutputMaxChars", "before": null, "after": 12000},
            ],
        }),
    );
    let old_rules = with(
        action(
            "claude.rules.replaced.model-choice.md",
            "claude",
            "/Users/me/.claude/rules/model-choice.md",
            "file",
            if ticked { "update" } else { "offer" },
        ),
        json!({"needs": ["rules"], "diff": OLD_RULES_DIFF}),
    );
    let pointer = with(
        action(
            "claude.pointer.agents/CLAUDE.md",
            "claude",
            "/Users/me/.claude/agents/CLAUDE.md",
            "file",
            if ticked { "update" } else { "offer" },
        ),
        json!({"needs": ["rules"],
            "diff": "--- /Users/me/.claude/agents/CLAUDE.md\n+++ /Users/me/.claude/agents/CLAUDE.md\n@@ -3,1 +3,1 @@\n-Models: see rules/model-choice.md\n+Models: see rules/notchling-cost.md\n"}),
    );
    let orchestrator = with(
        action(
            "claude.orchestrator.new",
            "claude",
            "/Users/me/.claude/agents/notchling-orchestrator.md",
            "file",
            "offer",
        ),
        json!({"needs": ["orchestrator_new"]}),
    );
    let agents = with(
        action(
            "codex.agents_md",
            "codex",
            "/Users/me/.codex/AGENTS.md",
            "block",
            "create",
        ),
        json!({"diff": "--- /Users/me/.codex/AGENTS.md\n+++ /Users/me/.codex/AGENTS.md\n@@ -0,0 +1,3 @@\n+<!-- notchling:notchling-cost -->\n+## Models and work habits (Notchling)\n+<!-- /notchling:notchling-cost -->\n"}),
    );
    let config = with(
        action(
            "codex.config",
            "codex",
            "/Users/me/.codex/config.toml",
            "keys",
            "offer",
        ),
        json!({"needs": ["codex_config"],
            "keys": [{"key": "agents.max_threads", "before": null, "after": 5}],
            "notes": ["agents.job_max_runtime_seconds: kept"]}),
    );
    let agy = with(
        action(
            "agy.rule",
            "agy",
            "/Users/me/.gemini/config/plugins/notchling/rules/notchling-cost.md",
            "file",
            "skip",
        ),
        json!({"reason": "Antigravity is not installed here."}),
    );
    let unchanged = action(
        "claude.agent.reader",
        "claude",
        "/Users/me/.claude/agents/notchling-reader.md",
        "file",
        "unchanged",
    );
    json!({"on": on, "plan": {
        "changes": 5,
        "actions": [rule, builder, settings, old_rules, pointer, orchestrator, unchanged, agents, config, agy],
        "report": [],
        "detected": {"claude": true, "codex": true, "antigravity": false, "pstack": false,
            "orchestrator": false, "ownRules": ["/Users/me/.claude/rules/model-choice.md"]},
        "profile": profile_reply(false)["profile"].clone(),
    }})
}

pub fn versions_reply() -> Value {
    json!({"tools": [
        {"tool": "claude", "installed": {"version": "2.1.328", "sha256": null}},
        {"tool": "codex", "installed": {"version": "0.166.0", "sha256": null}},
        {"tool": "antigravity", "installed": null},
        {"tool": "notchling", "installed": {"version": "2.0.0", "sha256": null}},
    ]})
}

fn applied_reply() -> Value {
    json!({"on": true, "applied": true, "failed": false, "results": [
        {"id": "claude.rule", "path": "/Users/me/.claude/rules/notchling-cost.md", "kind": "create",
            "done": true, "backup": null, "error": null},
        {"id": "claude.agent.builder", "path": "/Users/me/.claude/agents/notchling-builder.md",
            "kind": "create", "done": true, "backup": null, "error": null},
        {"id": "claude.settings", "path": "/Users/me/.claude/settings.json", "kind": "update",
            "done": true, "backup": "/Users/me/.claude/settings.json.notchling-setup-1793000000000", "error": null},
        {"id": "codex.agents_md", "path": "/Users/me/.codex/AGENTS.md", "kind": "create",
            "done": true, "backup": null, "error": null},
    ]})
}

fn undo_reply() -> Value {
    json!({"on": true, "tool": "claude", "conflicts": 1, "results": [
        {"id": "claude.rule", "path": "/Users/me/.claude/rules/notchling-cost.md",
            "outcome": "removed", "detail": null},
        {"id": "claude.agent.builder", "path": "/Users/me/.claude/agents/notchling-builder.md",
            "outcome": "conflict", "detail": "edited since Notchling wrote it; left as it is"},
        {"id": "claude.settings", "path": "/Users/me/.claude/settings.json",
            "outcome": "restored", "detail": null},
    ]})
}

pub fn setup(shown: Shown) -> SetupModel {
    let on = shown != Shown::SetupOff;
    let mut model = SetupModel::new(HOME);
    if shown == Shown::Damaged {
        model.show_plan(Err(DAMAGED.to_owned()), None);
        return model;
    }
    model.show_plan(Ok(plan_reply(on, false)), Some(versions_reply()));
    match shown {
        Shown::Setup | Shown::SetupOff => {
            model.toggle_diff("claude.rule");
            model.toggle_diff("claude.settings");
        }
        Shown::Applied => {
            model.stage = Stage::Review;
            model.finish_apply(Ok(applied_reply()));
            model.finish_undo(0, Ok(undo_reply()));
        }
        _ => {}
    }
    model
}

pub fn check(shown: Shown) -> CheckModel {
    let mut model = CheckModel::default();
    if shown == Shown::CheckDamaged {
        model.show(Err(DAMAGED.to_owned()), HOME);
        return model;
    }
    if shown != Shown::Check {
        return model;
    }
    model.show(
        Ok(json!({"on": true, "drift": true, "rows": [
            {"id": "claude.rule", "tool": "claude", "path": "/Users/me/.claude/rules/notchling-cost.md",
                "state": "in_step", "detail": null},
            {"id": "claude.agent.builder", "tool": "claude",
                "path": "/Users/me/.claude/agents/notchling-builder.md", "state": "edited", "detail": null},
            {"id": "claude.settings", "tool": "claude", "path": "/Users/me/.claude/settings.json",
                "state": "changed_by_tool",
                "detail": "modelSettings.claude-opus-5-5.autoCompactWindow: /model wrote 400000"},
            {"id": "claude.agent.reader", "tool": "claude",
                "path": "/Users/me/.claude/agents/notchling-reader.md", "state": "missing", "detail": null},
            {"id": "codex.agents_md", "tool": "codex", "path": "/Users/me/.codex/AGENTS.md",
                "state": "outdated", "detail": null},
            {"id": "codex.config", "tool": "codex", "path": "/Users/me/.codex/config.toml",
                "state": "changed_by_tool", "detail": "agents.max_threads: kept"},
            {"id": "claude.pstack", "tool": "claude", "path": "/Users/me/.claude/rules/pstack-models.md",
                "state": "skipped", "detail": "kept"},
        ]})),
        HOME,
    );
    model
}

pub fn cache() -> Cache {
    Cache::parse("{\"promptCacheTtl\": \"1h\", \"cache_control\": {\"ttl\": \"2h\"}}")
}

pub fn selection(shown: Shown) -> Option<(&'static str, usize)> {
    (shown == Shown::Custom).then_some(("judge", 0))
}
