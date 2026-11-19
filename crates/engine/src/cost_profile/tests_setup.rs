use super::action::{Kind, Options, State, CONSENT_CODEX_AGENTS, CONSENT_CODEX_CONFIG};
use super::action::{
    CONSENT_ORCHESTRATOR, CONSENT_ORCHESTRATOR_NEW, CONSENT_PSTACK, CONSENT_RULES,
};
use super::apply::apply;
use super::import::import_rules;
use super::manifest::Manifest;
use super::model::{Profile, Role};
use super::plan::{plan, ID_OWN_RULES, ID_PSTACK, ID_RULE, ID_SETTINGS};
use super::render::{self, Features};
use super::tests::*;
use crate::catalog::Tool;

fn rig_full() -> Rig {
    Rig::new("full").claude().codex().agy()
}

fn planned(rig: &Rig, opts: &Options) -> super::action::Plan {
    let (have, probe, manifest) = (avail(), Fake::all(), Manifest::default());
    plan(
        &env(rig, &have, &probe, &manifest),
        &Profile::default(),
        opts,
    )
}

fn kind(p: &super::action::Plan, id: &str) -> Kind {
    p.get(id).unwrap_or_else(|| panic!("no action {id}")).kind
}

#[test]
fn a_mac_with_none_of_the_tools_gets_nothing() {
    let rig = Rig::new("none");
    let p = planned(&rig, &everything());
    assert!(!p.changes());
    assert!(p.actions.iter().all(|a| a.kind == Kind::Skip));
    assert!(p
        .actions
        .iter()
        .all(|a| a.reason.as_deref() == Some("tool-missing")));
    assert!(rig.names(&rig.dir).iter().all(|n| n == "data"));
}

#[test]
fn a_bare_claude_install_gets_only_notchlings_own_files_and_keys() {
    let rig = Rig::new("bare").claude();
    let p = planned(&rig, &Options::default());
    assert_eq!(kind(&p, ID_RULE), Kind::Create);
    assert_eq!(kind(&p, ID_SETTINGS), Kind::Update);
    assert!(
        p.get(ID_PSTACK).is_none(),
        "no pstack plugin, no pstack file"
    );
    assert!(p.get(ID_OWN_RULES).is_none());
    assert_eq!(kind(&p, "claude.orchestrator.new"), Kind::Offer);
    let (have, probe, manifest) = (avail(), Fake::all(), Manifest::default());
    let done = apply(&env(&rig, &have, &probe, &manifest), &p, 1000);
    assert!(!done.failed(), "{:?}", done.results);
    assert_eq!(
        rig.names(&rig.roots.claude.join("agents")),
        [
            "notchling-builder.md",
            "notchling-reader.md",
            "notchling-reviewer.md"
        ]
    );
    assert_eq!(
        rig.names(&rig.roots.claude.join("rules")),
        ["notchling-cost.md"]
    );
    assert!(!rig.rule("pstack-models.md").exists());
    let settings = rig.read(&rig.settings());
    assert!(settings.contains("bashOutputMaxChars"));
    assert!(rig
        .names(&rig.roots.claude)
        .iter()
        .any(|n| n.starts_with("settings.json.notchling-backup-")));
}

#[test]
fn applying_twice_changes_nothing_the_second_time() {
    let rig = rig_full();
    rig.write(&rig.roots.codex.join("config.toml"), "model = \"x\"\n");
    let opts = everything();
    let (have, probe) = (avail(), Fake::all());
    let first = {
        let manifest = Manifest::default();
        let e = env(&rig, &have, &probe, &manifest);
        apply(&e, &plan(&e, &Profile::default(), &opts), 1)
    };
    assert!(!first.failed(), "{:?}", first.results);
    let second = plan(
        &env(&rig, &have, &probe, &first.manifest),
        &Profile::default(),
        &opts,
    );
    let writing: Vec<_> = second
        .actions
        .iter()
        .filter(|a| a.kind.writes())
        .map(|a| &a.id)
        .collect();
    assert!(writing.is_empty(), "{writing:?}");
    assert!(
        second
            .actions
            .iter()
            .filter(|a| a.kind == Kind::Unchanged)
            .count()
            >= 8
    );
}

#[test]
fn the_claude_rule_is_generated_from_the_table_and_names_no_model_id() {
    let rig = Rig::new("rule").claude();
    let p = planned(&rig, &Options::default());
    let text = p.get(ID_RULE).and_then(|a| a.after.clone()).expect("text");
    assert!(text.starts_with("<!-- notchling-cost v1 (Balanced)."));
    assert!(text.contains("| read | reading many files, search, logs, running a check | notchling-reader | haiku | medium |"));
    assert!(text.contains("| hardest |"));
    assert!(text.contains("| fable | xhigh |"));
    assert!(!text.contains("claude-"));
    assert!(text.contains("At most 5 builders at once"));
    assert!(text.contains("The hard stop is 140 turns"));
    let builder = p
        .get("claude.agent.builder")
        .and_then(|a| a.after.clone())
        .expect("builder");
    assert!(builder.contains("maxTurns: 140"));
    assert!(builder.contains("model: sonnet\neffort: high"));
    let reader = p
        .get("claude.agent.reader")
        .and_then(|a| a.after.clone())
        .expect("reader");
    assert!(reader.contains("disallowedTools: Edit, Write, NotebookEdit"));
}

#[test]
fn the_wait_line_follows_the_feature_not_the_text() {
    let rig = Rig::new("wait").claude();
    let off = planned(&rig, &Options::default());
    let text = off
        .get(ID_RULE)
        .and_then(|a| a.after.clone())
        .expect("text");
    assert!(!text.contains("notchling wait"));
    let mut on = Options::default();
    on.features = Features { wait: true };
    let text = planned(&rig, &on)
        .get(ID_RULE)
        .and_then(|a| a.after.clone())
        .expect("text");
    assert!(text.contains("`notchling wait`"));
}

#[test]
fn pstack_is_written_only_when_its_plugin_is_there_and_only_with_consent() {
    let rig = Rig::new("pstack").claude();
    rig.write(&rig.rule("pstack-models.md"), PSTACK_MODELS);
    let p = planned(&rig, &everything());
    assert!(
        p.get(ID_PSTACK).is_none(),
        "the file exists but the plugin does not"
    );
    rig.write(
        &rig.roots
            .claude
            .join("plugins/cache/market/pstack/0.1/skills/x"),
        "",
    );
    let p = planned(&rig, &Options::default());
    assert_eq!(kind(&p, ID_PSTACK), Kind::Offer);
    let p = planned(&rig, &Options::consenting(&[CONSENT_PSTACK]));
    assert_eq!(kind(&p, ID_PSTACK), Kind::Update);
    let text = p
        .get(ID_PSTACK)
        .and_then(|a| a.after.clone())
        .expect("text");
    assert!(text.starts_with("# pstack model configuration."));
    assert!(!text.starts_with("---"));
    assert!(text.contains("\nbug-fix: sonnet xhigh\n"), "{text}");
    assert!(text.contains("\nhardest tasks: fable xhigh\n"), "{text}");
    assert!(
        text.contains("\narena runners: opus xhigh, sonnet high\n"),
        "{text}"
    );
    assert!(text.contains("\nhow explorer: haiku medium\n"), "{text}");
    assert!(text
        .lines()
        .filter(|l| !l.starts_with('#'))
        .all(|l| l.contains(": ")));
}

#[test]
fn pstack_gets_its_file_even_when_it_is_missing_but_the_plugin_is_there() {
    let rig = Rig::new("pstack-new").claude();
    rig.write(
        &rig.roots.claude.join("plugins/cache/m/pstack/1/README.md"),
        "",
    );
    let p = planned(&rig, &Options::consenting(&[CONSENT_PSTACK]));
    assert_eq!(kind(&p, ID_PSTACK), Kind::Create);
}

#[test]
fn pstack_is_found_from_the_plugin_list_too() {
    let rig = Rig::new("pstack-list").claude();
    rig.write(
        &rig.roots.claude.join("plugins/installed_plugins.json"),
        "{\"plugins\":{\"pstack@cursor-plugins\":[{}]}}",
    );
    assert!(planned(&rig, &Options::default()).detected.pstack);
}

#[test]
fn the_import_report_accounts_for_every_line_of_the_hand_written_rules() {
    let out = import_rules(MODEL_CHOICE, &Profile::default());
    let lines: Vec<&str> = MODEL_CHOICE
        .lines()
        .filter(|l| !l.trim().is_empty())
        .collect();
    assert_eq!(out.report.len(), lines.len());
    for (row, line) in out.report.iter().zip(&lines) {
        assert_eq!(row.text, *line);
        assert!(
            ["kept", "regenerated"].contains(&row.fate.as_str())
                || row.fate.starts_with("profile:")
        );
    }
    let fates = |f: &str| out.report.iter().filter(|r| r.fate == f).count();
    assert_eq!(fates("profile:read"), 1);
    assert_eq!(fates("profile:code"), 1);
    assert_eq!(fates("profile:judge"), 1);
    assert_eq!(fates("profile:hardest"), 1);
    assert_eq!(fates("regenerated"), 2);
    let kept = out.kept.join("\n");
    assert!(kept.contains("## Effort"));
    assert!(kept.contains("Escalate one step"));
    assert!(kept.contains("## Reviews and second opinions"));
    assert!(!kept.contains("| haiku"));
    assert_eq!(
        out.report.iter().filter(|r| r.fate == "kept").count(),
        out.kept.iter().filter(|l| !l.trim().is_empty()).count()
    );
    assert!(out
        .picks
        .iter()
        .any(|(r, c)| *r == Role::Hardest && c.to_string() == "fable xhigh"));
}

#[test]
fn role_lines_import_with_their_effort() {
    let text = "feature, refactoring: sonnet low\nbug-fix: opus high\nhillclimb: opus high\nnotes: keep me\n";
    let out = import_rules(text, &Profile::default());
    assert_eq!(out.picks.len(), 2);
    let mut p = Profile::default();
    super::import::apply_to(&mut p, &out, "x.md");
    assert!(p.is_custom());
    assert_eq!(
        p.cell(Role::Code, Tool::Claude)
            .map(|c| c.to_string())
            .as_deref(),
        Some("sonnet low")
    );
    assert_eq!(
        p.cell(Role::Fix, Tool::Claude)
            .map(|c| c.to_string())
            .as_deref(),
        Some("opus high")
    );
    assert_eq!(out.kept, ["hillclimb: opus high", "notes: keep me"]);
}

#[test]
fn replacing_the_hand_written_rules_needs_consent_and_keeps_every_other_line() {
    let rig = Rig::new("own").claude();
    rig.write(&rig.rule("model-choice.md"), MODEL_CHOICE);
    rig.write(&rig.rule("model-choice.md.bak-20261115"), "old copy");
    let p = planned(&rig, &Options::default());
    assert_eq!(kind(&p, ID_OWN_RULES), Kind::Offer);
    assert!(!p
        .get(ID_RULE)
        .and_then(|a| a.after.clone())
        .expect("t")
        .contains("Escalate one step"));
    assert!(!p.report.is_empty());
    let p = planned(&rig, &Options::consenting(&[CONSENT_RULES]));
    assert_eq!(kind(&p, ID_OWN_RULES), Kind::Remove);
    let text = p.get(ID_RULE).and_then(|a| a.after.clone()).expect("text");
    assert!(text.contains("## Your own rules, kept from model-choice.md"));
    assert!(text.contains("Escalate one step"));
    assert!(text.contains("## Reviews and second opinions"));
    assert!(text.contains("notchling delegate"));
    assert_eq!(p.profile.preset, "balanced");
    assert!(p
        .profile
        .imported
        .as_ref()
        .is_some_and(|i| i.source == "model-choice.md"));
    let (have, probe, manifest) = (avail(), Fake::all(), Manifest::default());
    let done = apply(&env(&rig, &have, &probe, &manifest), &p, 5);
    assert!(!done.failed(), "{:?}", done.results);
    assert!(!rig.rule("model-choice.md").exists());
    let backups = rig.names(&rig.roots.claude.join("rules"));
    let backup = backups
        .iter()
        .find(|n| n.starts_with("model-choice.md.notchling-backup-"))
        .expect("backup");
    assert_eq!(rig.read(&rig.rule(backup)), MODEL_CHOICE);
    assert!(backups.contains(&"model-choice.md.bak-20261115".to_string()));
}

#[test]
fn a_second_hand_written_model_file_is_left_alone() {
    let rig = Rig::new("own2").claude();
    rig.write(&rig.rule("model-choice.md"), MODEL_CHOICE);
    rig.write(&rig.rule("models-extra.md"), "# my other rule\n");
    let p = planned(&rig, &everything());
    assert_eq!(kind(&p, "claude.rules.left.models-extra.md"), Kind::Skip);
}

#[test]
fn an_existing_orchestrator_is_managed_in_place_with_every_line_kept() {
    let rig = Rig::new("orch").claude();
    rig.write(&rig.agent("orchestrator.md"), ORCHESTRATOR);
    let p = planned(&rig, &Options::default());
    assert_eq!(kind(&p, "claude.orchestrator"), Kind::Offer);
    assert!(p.get("claude.orchestrator.new").is_none());
    let p = planned(&rig, &Options::consenting(&[CONSENT_ORCHESTRATOR]));
    assert_eq!(kind(&p, "claude.orchestrator"), Kind::Update);
    let after = p
        .get("claude.orchestrator")
        .and_then(|a| a.after.clone())
        .expect("text");
    for line in ORCHESTRATOR.lines() {
        if line.contains("model-choice.md") {
            assert!(
                after.contains(&line.replace("rules/model-choice.md", "rules/notchling-cost.md"))
            );
        } else {
            assert!(after.lines().any(|l| l == line), "lost {line:?}");
        }
    }
    assert!(after.contains("rules/notchling-cost.md"));
    assert!(!after.contains("model-choice.md"));
    assert!(after.contains("Reviews go through `notchling delegate`"));
    assert!(!after.contains("notchling wait"));
    assert!(!rig.agent("notchling-orchestrator.md").exists());
    let mut on = Options::consenting(&[CONSENT_ORCHESTRATOR]);
    on.features = Features { wait: true };
    let after = planned(&rig, &on)
        .get("claude.orchestrator")
        .and_then(|a| a.after.clone())
        .expect("text");
    assert!(after.contains("Wait with `notchling wait`"));
    assert_eq!(render::orchestrator_in_place(&after, on.features), after);
}

#[test]
fn without_an_orchestrator_a_new_one_is_an_offer_that_is_off_by_default() {
    let rig = Rig::new("orch-new").claude();
    let p = planned(&rig, &Options::default());
    assert_eq!(kind(&p, "claude.orchestrator.new"), Kind::Offer);
    let p = planned(&rig, &Options::consenting(&[CONSENT_ORCHESTRATOR_NEW]));
    assert_eq!(kind(&p, "claude.orchestrator.new"), Kind::Create);
    let text = p
        .get("claude.orchestrator.new")
        .and_then(|a| a.after.clone())
        .expect("text");
    assert!(text.contains("name: notchling-orchestrator"));
}

#[test]
fn files_that_point_at_the_replaced_rule_get_a_pointer_fix_with_consent() {
    let rig = Rig::new("pointer").claude();
    rig.write(&rig.rule("model-choice.md"), MODEL_CHOICE);
    rig.write(
        &rig.agent("helper.md"),
        "see ~/.claude/rules/model-choice.md for models\n",
    );
    rig.write(
        &rig.roots.claude.join("CLAUDE.md"),
        "# mine\nno pointer here\n",
    );
    let p = planned(&rig, &Options::default());
    assert_eq!(kind(&p, "claude.pointer.helper.md"), Kind::Offer);
    assert!(p.get("claude.pointer.CLAUDE.md").is_none());
    let p = planned(&rig, &Options::consenting(&[CONSENT_RULES]));
    let a = p.get("claude.pointer.helper.md").expect("pointer");
    assert_eq!(
        a.after.as_deref(),
        Some("see ~/.claude/rules/notchling-cost.md for models\n")
    );
    assert!(a.diff.contains("-see ~/.claude/rules/model-choice.md"));
}

#[test]
fn a_file_of_the_same_name_that_is_not_ours_is_never_overwritten() {
    let rig = Rig::new("foreign").claude();
    rig.write(&rig.agent("notchling-builder.md"), "my own builder\n");
    let p = planned(&rig, &everything());
    let a = p.get("claude.agent.builder").expect("builder");
    assert_eq!(a.kind, Kind::Skip);
    assert_eq!(a.state, State::NotOurs);
    let (have, probe, manifest) = (avail(), Fake::all(), Manifest::default());
    apply(&env(&rig, &have, &probe, &manifest), &p, 1);
    assert_eq!(
        rig.read(&rig.agent("notchling-builder.md")),
        "my own builder\n"
    );
}

#[test]
fn a_symlinked_target_is_refused() {
    #[cfg(unix)]
    {
        let rig = Rig::new("link").claude();
        std::fs::create_dir_all(rig.roots.claude.join("agents")).expect("dir");
        let elsewhere = rig.dir.join("elsewhere.md");
        std::fs::write(&elsewhere, "x").expect("write");
        std::os::unix::fs::symlink(&elsewhere, rig.agent("notchling-reader.md")).expect("link");
        let p = planned(&rig, &everything());
        assert_eq!(kind(&p, "claude.agent.reader"), Kind::Skip);
    }
}

#[test]
fn codex_gets_a_block_in_a_new_agents_file_and_asks_before_touching_its_own() {
    let rig = Rig::new("codex").codex();
    let p = planned(&rig, &Options::default());
    assert_eq!(kind(&p, "codex.agents_md"), Kind::Create);
    let text = p
        .get("codex.agents_md")
        .and_then(|a| a.after.clone())
        .expect("text");
    assert!(text.starts_with("<!-- notchling:notchling-cost -->\n"));
    assert!(text
        .trim_end()
        .ends_with("<!-- /notchling:notchling-cost -->"));
    assert!(text.contains("| code | gpt-6-sol | medium |"));
    assert!(text.contains("| image | tool default | low |"));
    rig.write(
        &rig.roots.codex.join("AGENTS.md"),
        "# my codex rules\n- be brief\n",
    );
    let p = planned(&rig, &Options::default());
    assert_eq!(kind(&p, "codex.agents_md"), Kind::Offer);
    let p = planned(&rig, &Options::consenting(&[CONSENT_CODEX_AGENTS]));
    let after = p
        .get("codex.agents_md")
        .and_then(|a| a.after.clone())
        .expect("text");
    assert!(after.starts_with("# my codex rules\n- be brief\n\n<!-- notchling:notchling-cost -->"));
    rig.write(&rig.roots.codex.join("AGENTS.override.md"), "x");
    let p = planned(&rig, &everything());
    assert_eq!(
        p.get("codex.agents_md")
            .and_then(|a| a.reason.clone())
            .as_deref(),
        Some("override-present")
    );
}

#[test]
fn codex_config_keys_are_written_only_after_each_one_passes_the_probe() {
    let rig = Rig::new("codex-cfg").codex();
    let config = rig.roots.codex.join("config.toml");
    rig.write(
        &config,
        "model = \"gpt-6-sol\"\n\n[agents]\nmax_depth = 2\n",
    );
    let (have, manifest) = (avail(), Manifest::default());
    let probe = Fake {
        reject: vec!["default_subagent_model"],
    };
    let p = plan(
        &env(&rig, &have, &probe, &manifest),
        &Profile::default(),
        &Options::consenting(&[CONSENT_CODEX_CONFIG]),
    );
    let a = p.get("codex.config").expect("config");
    assert_eq!(a.kind, Kind::Update);
    let after = a.after.clone().expect("text");
    assert!(after.contains("max_threads = 5"));
    assert!(after.contains("default_subagent_reasoning_effort = \"medium\""));
    assert!(!after.contains("default_subagent_model"));
    assert!(a
        .notes
        .iter()
        .any(|n| n.contains("agents.default_subagent_model: codex-rejects")));
    assert!(after.contains("model = \"gpt-6-sol\""));
    assert!(after.contains("max_depth = 2"));
    let none = plan(
        &env(&rig, &have, &probe, &manifest),
        &Profile::default(),
        &Options::default(),
    );
    assert_eq!(kind(&none, "codex.config"), Kind::Offer);
}

#[test]
fn the_codex_alias_of_max_threads_is_respected() {
    let rig = Rig::new("codex-alias").codex();
    rig.write(
        &rig.roots.codex.join("config.toml"),
        "[agents]\nmax_concurrent_threads_per_session = 3\n",
    );
    let p = planned(&rig, &everything());
    let a = p.get("codex.config").expect("config");
    assert!(a.notes.iter().any(|n| n.contains("max_threads: alias-set")));
    assert!(!a.after.clone().unwrap_or_default().contains("max_threads"));
}

#[test]
fn a_missing_codex_config_is_not_created() {
    let rig = Rig::new("codex-nocfg").codex();
    let p = planned(&rig, &everything());
    assert_eq!(kind(&p, "codex.config"), Kind::Skip);
}

#[test]
fn antigravity_gets_a_rule_in_notchlings_plugin_folder_and_the_scaffold_only_when_absent() {
    let rig = Rig::new("agy").agy();
    let p = planned(&rig, &Options::default());
    assert_eq!(kind(&p, "agy.plugin.json"), Kind::Create);
    assert_eq!(kind(&p, "agy.plugin.mark"), Kind::Create);
    let rule = p.get("agy.rule").expect("rule");
    assert!(rule
        .path
        .ends_with("plugins/notchling/rules/notchling-cost.md"));
    assert!(rule
        .after
        .clone()
        .expect("t")
        .starts_with("---\ntrigger: always_on\n---\n"));
    let (have, probe, manifest) = (avail(), Fake::all(), Manifest::default());
    let done = apply(&env(&rig, &have, &probe, &manifest), &p, 9);
    assert!(!done.failed(), "{:?}", done.results);
    let plugin = rig.roots.antigravity.join("plugins/notchling");
    assert_eq!(
        rig.names(&plugin),
        [".notchling-plugin", "plugin.json", "rules"]
    );
    let again = {
        let e = env(&rig, &have, &probe, &done.manifest);
        plan(&e, &Profile::default(), &Options::default())
    };
    assert!(
        again.get("agy.plugin.json").is_none(),
        "the folder is ours now"
    );
    assert_eq!(kind(&again, "agy.rule"), Kind::Unchanged);
}

#[test]
fn a_foreign_notchling_plugin_folder_is_left_alone() {
    let rig = Rig::new("agy-foreign").agy();
    rig.write(
        &rig.roots.antigravity.join("plugins/notchling/plugin.json"),
        "{}",
    );
    let p = planned(&rig, &everything());
    assert_eq!(
        p.get("agy.rule").and_then(|a| a.reason.clone()).as_deref(),
        Some("plugin-not-ours")
    );
}

#[test]
fn a_tool_that_is_off_in_the_profile_is_skipped() {
    let rig = rig_full();
    let mut profile = Profile::default();
    profile.tools.insert(Tool::Codex, false);
    profile.tools.insert(Tool::Claude, false);
    let (have, probe, manifest) = (avail(), Fake::all(), Manifest::default());
    let p = plan(
        &env(&rig, &have, &probe, &manifest),
        &profile,
        &everything(),
    );
    for a in &p.actions {
        if a.tool != Tool::Antigravity {
            assert_eq!(a.reason.as_deref(), Some("tool-off"), "{}", a.id);
        }
    }
}

#[test]
fn a_hand_edited_file_is_reported_not_overwritten() {
    let rig = Rig::new("edited").claude();
    let (have, probe, empty) = (avail(), Fake::all(), Manifest::default());
    let done = {
        let e = env(&rig, &have, &probe, &empty);
        apply(&e, &plan(&e, &Profile::default(), &Options::default()), 1)
    };
    let path = rig.rule("notchling-cost.md");
    let text = rig.read(&path);
    rig.write(&path, &format!("{text}\n- my own habit\n"));
    let e = env(&rig, &have, &probe, &done.manifest);
    let p = plan(&e, &Profile::default(), &Options::default());
    let a = p.get(ID_RULE).expect("rule");
    assert_eq!((a.kind, a.state), (Kind::Skip, State::Edited));
    assert!(a.diff.contains("-- my own habit"));
    assert!(rig.read(&path).contains("my own habit"));
}
