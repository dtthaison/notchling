use super::*;
use crate::catalog::roles::Roles;

const AGY: &str = "Fetching available models...\ngemini-4-neon-high\tGemini 4 Neon (High)\ngemini-4-neon-xhigh\tGemini 4 Neon (XHigh)\ngemini-4-neon-medium\tGemini 4 Neon (Medium)\ngemini-4-neon-low\tGemini 4 Neon (Low)\ngemini-4-argon-medium\tGemini 4 Argon (Medium)\ngemini-4-argon-high\tGemini 4 Argon (High)\ngemini-4-hydrogen-high\tGemini 4 Hydrogen (High)\ngemini-4-hydrogen-xhigh\tGemini 4 Hydrogen (XHigh)\ngemini-pro-orchestrator-neon\tOrchestrator (Neon)\ngpt-oss-120b-medium\tGPT-OSS 120B (Medium)\n";

const CODEX: &str = r#"{"models":[
  {"slug":"gpt-6.2-sol","display_name":"GPT-6.2-Sol","description":"fast","priority":1,"visibility":"list",
   "default_reasoning_level":"medium","context_window":500000,"base_instructions":"long text",
   "supported_reasoning_levels":[{"effort":"low"},{"effort":"medium"},{"effort":"xhigh"}]},
  {"slug":"gpt-6.2-astra","display_name":"GPT-6.2-Astra","priority":2,"visibility":"list",
   "supported_reasoning_levels":[{"effort":"medium"},{"effort":"xhigh"}]},
  {"slug":"gpt-6-astra","priority":3,"visibility":"list","upgrade":{"model":"gpt-6.2-astra"}},
  {"slug":"gpt-reserve","priority":9,"visibility":"hide"},
  {"display_name":"no slug"}]}"#;

fn model(id: &str, efforts: &[&str]) -> LiveModel {
    LiveModel {
        id: id.into(),
        efforts: efforts.iter().map(|e| e.to_string()).collect(),
        ..LiveModel::default()
    }
}

#[test]
fn the_agy_listing_gives_each_model_its_own_efforts_and_xhigh_only_where_listed() {
    let before = vec![LiveModel {
        id: "gemini-4-argon".into(),
        efforts: vec!["low".into(), "medium".into(), "high".into()],
        default_effort: Some("medium".into()),
        ..LiveModel::default()
    }];
    let live = agy_live(AGY, &before);
    let ids: Vec<&str> = live.iter().map(|m| m.id.as_str()).collect();
    assert_eq!(
        ids,
        [
            "gemini-4-neon",
            "gemini-4-argon",
            "gemini-4-hydrogen",
            "gemini-pro-orchestrator-neon",
            "gpt-oss-120b-medium"
        ]
    );
    assert_eq!(live[0].efforts, ["low", "medium", "high", "xhigh"]);
    assert_eq!(live[0].name.as_deref(), Some("Gemini 4 Neon"));
    assert_eq!(live[1].efforts, ["medium", "high"]);
    assert_eq!(live[1].default_effort.as_deref(), Some("medium"));
    assert_eq!(live[2].efforts, ["high", "xhigh"]);
    assert!(live[3].efforts.is_empty());
    assert!(
        live[4].efforts.is_empty(),
        "one fixed level: the id carries it"
    );
    let listed = agy_pick::models(AGY);
    assert!(agy_pick::offers("gemini-4-neon", "xhigh", &listed));
    assert!(agy_pick::offers("gemini-4-hydrogen", "xhigh", &listed));
    assert!(!agy_pick::offers("gemini-4-argon", "xhigh", &listed));
}

#[test]
fn the_codex_catalog_becomes_live_models_and_a_trimmed_snapshot_the_loader_reads() {
    let live = codex_live(CODEX.as_bytes());
    let ids: Vec<&str> = live.iter().map(|m| m.id.as_str()).collect();
    assert_eq!(ids, ["gpt-6.2-sol", "gpt-6.2-astra", "gpt-6-astra"]);
    assert_eq!(live[0].efforts, ["low", "medium", "xhigh"]);
    assert_eq!(live[0].default_effort.as_deref(), Some("medium"));
    assert_eq!(live[0].context_window, Some(500000));

    let snapshot = codex_snapshot(CODEX.as_bytes()).unwrap();
    assert!(!snapshot.contains("base_instructions"));
    assert!(!snapshot.contains("no slug"));
    assert_eq!(codex_live(snapshot.as_bytes()), live);
    assert_eq!(
        hints_of(snapshot.as_bytes()),
        [("gpt-6-astra".to_string(), "gpt-6.2-astra".to_string())]
    );
    assert!(codex_snapshot(b"not json").is_none());
    assert!(codex_snapshot(br#"{"models":[]}"#).is_none());
}

#[test]
fn the_diff_names_new_gone_and_changed_efforts() {
    let before = vec![
        model("gemini-4-argon", &["low", "medium", "high"]),
        model("gemini-4-hydrogen", &["low", "medium", "high"]),
        model("old-model", &[]),
    ];
    let after = vec![
        model("gemini-4-argon", &["low", "medium", "high"]),
        model("gemini-4-hydrogen", &["low", "medium", "high", "xhigh"]),
        model("gemini-4-neon", &["high", "xhigh"]),
    ];
    let d = diff(&before, &after);
    assert_eq!(d.added, ["gemini-4-neon"]);
    assert_eq!(d.removed, ["old-model"]);
    assert_eq!(
        d.efforts,
        [EffortChange {
            model: "gemini-4-hydrogen".into(),
            added: vec!["xhigh".into()],
            removed: Vec::new(),
        }]
    );
    assert!(!d.is_empty());
    assert!(diff(&after, &after).is_empty());
    assert!(diff(&[], &[]).is_empty());
}

#[test]
fn a_model_without_a_price_is_reported_and_a_priced_one_is_not() {
    let catalog = Catalog::built_in();
    let live = vec![
        model("gpt-5", &[]),
        model("gpt-6.2-sol", &[]),
        model("gpt-5.6-luna", &[]),
        model("gpt-9-nova", &[]),
    ];
    assert_eq!(
        unpriced(&catalog, Tool::Codex, &live),
        ["gpt-5.6-luna", "gpt-9-nova"]
    );
    let agy = vec![model("gemini-4-neon", &[]), model("gemini-9-xenon", &[])];
    assert_eq!(
        unpriced(&catalog, Tool::Antigravity, &agy),
        ["gemini-9-xenon"]
    );
    assert!(PRICE_HELP.contains("never invents a price"));
    assert!(PRICE_HELP.contains("\"prices\""));
}

fn live_of(models: &[&str], hints: &[(&str, &str)], efforts: &[(&str, &[&str])]) -> Live {
    Live {
        models: models.iter().map(|m| m.to_string()).collect(),
        hints: hints
            .iter()
            .map(|(a, b)| (a.to_string(), b.to_string()))
            .collect(),
        efforts: efforts
            .iter()
            .map(|(m, e)| (m.to_string(), e.iter().map(|x| x.to_string()).collect()))
            .collect(),
    }
}

#[test]
fn role_suggestions_name_a_gone_or_newer_model_and_change_nothing() {
    let roles = Roles::built_in();
    let before = roles.clone();
    let codex = live_of(&["gpt-6-astra", "gpt-6.2-astra", "gpt-6-sol"], &[], &[]);
    let agy = live_of(
        &["gemini-4-argon", "gemini-4-hydrogen", "claude-opus-5-5"],
        &[],
        &[
            ("gemini-4-argon", &["medium", "high"]),
            ("gemini-4-hydrogen", &["high", "xhigh"]),
            ("claude-opus-5-5", &["high"]),
        ],
    );
    let lines = suggestions(&roles, &codex, &agy);
    assert!(
        lines
            .iter()
            .any(|l| l.starts_with("review-final: codex gpt-6-astra")
                && l.contains("newer in family: gpt-6.2-astra")),
        "{lines:?}"
    );
    assert_eq!(roles, before, "the roles are never rewritten");

    let gone = live_of(&["gemini-4-argon"], &[], &[("gemini-4-argon", &["high"])]);
    let lines = suggestions(&roles, &codex, &gone);
    assert!(
        lines
            .iter()
            .any(|l| l.contains("gemini-4-hydrogen") && l.contains("gone")),
        "{lines:?}"
    );

    let lacking = live_of(
        &["gemini-4-argon", "gemini-4-hydrogen"],
        &[],
        &[
            ("gemini-4-argon", &["high"]),
            ("gemini-4-hydrogen", &["low", "medium"]),
        ],
    );
    let lines = suggestions(&roles, &codex, &lacking);
    assert!(
        lines
            .iter()
            .any(|l| l.contains("gemini-4-hydrogen no longer offers effort high")),
        "{lines:?}"
    );
}

#[test]
fn the_report_lists_changes_prices_suggestions_and_says_when_nothing_was_written() {
    let report = Report {
        applied: false,
        tools: vec![
            ToolReport {
                tool: "antigravity".into(),
                source: "agy models".into(),
                read: true,
                note: None,
                total: 5,
                diff: Diff {
                    added: vec!["gemini-4-neon".into()],
                    removed: vec!["gone".into()],
                    efforts: vec![EffortChange {
                        model: "gemini-4-hydrogen".into(),
                        added: vec!["xhigh".into()],
                        removed: Vec::new(),
                    }],
                },
                unpriced: vec!["gemini-4-neon".into()],
                written: false,
                first: false,
            },
            ToolReport {
                tool: "codex".into(),
                source: "codex debug models".into(),
                read: false,
                note: Some(
                    "couldn't run codex debug models; the lists Notchling has were kept".into(),
                ),
                total: 3,
                ..ToolReport::default()
            },
        ],
        suggestions: vec!["review: codex gpt-6-astra: newer in family: gpt-6.2-astra".into()],
        skills: vec!["notchling-delegate (claude): would update".into()],
    };
    let text = report.render();
    for line in [
        "Antigravity (agy models): 5 models",
        "  new: gemini-4-neon",
        "  gone: gone",
        "  efforts gemini-4-hydrogen: +xhigh",
        "  no price: gemini-4-neon",
        "Codex (codex debug models): not read",
        "  note: couldn't run codex debug models",
        "Role picks to review (nothing was changed):",
        "  review: codex gpt-6-astra: newer in family: gpt-6.2-astra",
        "  notchling-delegate (claude): would update",
        "This was a check: nothing was written.",
    ] {
        assert!(text.contains(line), "{line}\n{text}");
    }
    assert!(text.contains("Prices: Notchling never invents a price"));
    let v = report.to_json();
    assert_eq!(v["applied"], false);
    assert_eq!(v["changed"], true);
    assert_eq!(v["tools"][0]["efforts"][0]["added"][0], "xhigh");
    assert_eq!(v["tools"][1]["read"], false);
    assert_eq!(v["text"], text);
    assert_eq!(report.unpriced_count(), 1);

    let quiet = Report {
        applied: true,
        tools: vec![ToolReport {
            tool: "claude".into(),
            source: "Anthropic's model list".into(),
            read: true,
            total: 4,
            ..ToolReport::default()
        }],
        ..Report::default()
    };
    let text = quiet.render();
    assert!(text.contains("Claude Code (Anthropic's model list): 4 models\n  no change"));
    assert!(text.contains("Role picks: all still current."));
    assert!(!text.contains("This was a check"));
    assert!(!text.contains("Prices:"));
}

#[test]
fn a_list_that_lost_most_of_the_old_one_is_shrunk_and_a_small_change_is_not() {
    let live = |ids: &[&str]| -> Vec<LiveModel> {
        ids.iter()
            .map(|id| LiveModel {
                id: (*id).into(),
                ..LiveModel::default()
            })
            .collect()
    };
    let before = live(&["a", "b", "c", "d"]);
    assert!(shrunk(&before, &live(&["a"])));
    assert!(shrunk(&before, &live(&["x", "y", "z", "a"])));
    assert!(!shrunk(&before, &live(&["a", "b", "x"])));
    assert!(
        !shrunk(&live(&["a", "b"]), &live(&["z"])),
        "a tiny list may change wholly"
    );
    assert!(!shrunk(&[], &live(&["a"])));
}

#[test]
fn a_first_read_prints_no_diff_and_says_so() {
    let report = Report {
        applied: true,
        tools: vec![ToolReport {
            tool: "claude".into(),
            source: "Anthropic's model list".into(),
            read: true,
            total: 4,
            first: true,
            ..ToolReport::default()
        }],
        ..Report::default()
    };
    let text = report.render();
    assert!(
        text.contains("first read: nothing to compare with"),
        "{text}"
    );
    assert!(!text.contains("no change"));
    assert_eq!(report.to_json()["tools"][0]["first"], true);
}
