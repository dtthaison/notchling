use super::*;
use crate::cli::{parse, Command};
use crate::contract::{EXIT_CODES, REASONS};
use std::collections::BTreeMap;
use std::path::PathBuf;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn golden(path: PathBuf, text: String) {
    if std::env::var_os("NOTCHLING_UPDATE_GOLDEN").is_some() {
        std::fs::write(&path, text).unwrap();
        return;
    }
    let have = std::fs::read_to_string(&path).unwrap_or_default();
    assert_eq!(
        have,
        text,
        "{} is out of date: run the tests with NOTCHLING_UPDATE_GOLDEN=1",
        path.display()
    );
}

#[test]
fn every_op_is_in_exactly_one_place() {
    let mut seen: BTreeMap<&str, usize> = BTreeMap::new();
    for op in all_ops() {
        *seen.entry(op).or_default() += 1;
    }
    for (op, times) in &seen {
        assert_eq!(*times, 1, "{op} appears {times} times");
    }
    let known: Vec<&str> = crate::op_kinds::known_ops().to_vec();
    for op in &known {
        assert!(seen.contains_key(op), "{op} is in no list of commands.rs");
    }
    for op in seen.keys() {
        assert!(
            known.contains(op) || *op == "daemon.capabilities",
            "{op} is not an op"
        );
    }
}

#[test]
fn every_command_row_parses_and_names_known_exit_codes() {
    let codes: Vec<i32> = EXIT_CODES.iter().map(|(c, _, _)| *c).collect();
    for row in rows() {
        let words: Vec<String> = row.command.split(' ').map(String::from).collect();
        let parsed = parse(words);
        assert!(
            !matches!(&parsed, Err(m) if m.starts_with("unknown command")),
            "{}: {parsed:?}",
            row.command
        );
        assert!(
            row.usage.starts_with(&format!("notchling {}", row.command)),
            "{}",
            row.usage
        );
        assert!(!row.summary.is_empty());
        assert!(row.exits.contains(&0));
        for exit in row.exits {
            assert!(
                codes.contains(exit) || [126, 127].contains(exit),
                "{} returns {exit}",
                row.command
            );
        }
        assert_eq!(
            row.json,
            row.usage.contains("--json") || row.json && row.ops.is_empty()
        );
        assert!(matches!(
            row.consent,
            "none" | "request" | "by_key_class" | "by_entry" | "own_free" | "never"
        ));
    }
    assert_eq!(
        parse(["help".to_string()]),
        Ok(Command::Help { json: false })
    );
}

#[test]
fn a_user_only_or_internal_op_never_has_a_command() {
    for op in USER_ONLY
        .iter()
        .map(|u| u.op)
        .chain(INTERNAL.iter().map(|i| i.op))
    {
        assert!(command_of(op).is_none(), "{op}");
    }
    for i in INTERNAL {
        assert!(INTERNAL_WHYS.contains(&i.why), "{}", i.op);
    }
    for p in PLANNED {
        assert!((2..=9).contains(&p.phase), "{}", p.op);
    }
}

#[test]
fn an_op_an_agent_may_not_call_says_why() {
    let (_, data) = why_not("session.permission").unwrap();
    assert_eq!(data["reason"], "never");
    assert_eq!(data["never"], true);
    let (_, data) = why_not("agy.open").unwrap();
    assert_eq!(
        (data["reason"].as_str(), data["phase"].as_u64()),
        (Some("not_yet"), Some(4))
    );
    let (_, data) = why_not("accounts.switch").unwrap();
    assert_eq!(data["reason"], "app_only");
    let (_, data) = why_not("agy.open").unwrap();
    assert_eq!(data["ask"]["command"], "notchling ask -- …");
    assert_eq!(why_not("git.status").unwrap().1["reason"], "app_only");
    assert_eq!(why_not("hook.request").unwrap().1["reason"], "internal_op");
    assert!(why_not("settings.set").is_none());
}

#[test]
fn the_help_json_has_the_shape_agents_read() {
    let help = help_value("9.9.9");
    assert_eq!(help["notchling"], "9.9.9");
    assert_eq!(help["api"], notchling_local_api::API_VERSION);
    assert_eq!(
        help["exitCodes"].as_array().unwrap().len(),
        EXIT_CODES.len()
    );
    assert_eq!(help["reasons"].as_array().unwrap().len(), REASONS.len());
    let set = help["commands"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["command"] == "settings set")
        .unwrap();
    assert_eq!(set["consent"], "by_key_class");
    assert_eq!(set["waits"], true);
    assert_eq!(set["idempotent"], true);
    assert_eq!(set["skill"], "notchling-settings");
    assert_eq!(
        set["ops"][0],
        json!({"op": "settings.set", "tier": "agent", "acts": true})
    );
    assert!(help["planned"][0]["phase"].is_u64());
    assert!(help["userOnly"][0]["why"].is_string());
    assert!(
        help.get("internal").is_none(),
        "help does not list internal ops"
    );
    assert!(
        registry_value().get("internal").is_some(),
        "the golden keeps them"
    );
    let text = help.to_string();
    for entry in INTERNAL {
        assert!(!text.contains(&format!("\"{}\"", entry.op)), "{}", entry.op);
    }
}

#[test]
fn every_row_lists_the_codes_its_failures_can_return() {
    for row in rows() {
        let has = |code: i32| row.exits.contains(&code);
        assert!(has(0), "{} can succeed", row.command);
        let mut sorted = row.exits.to_vec();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(
            sorted, row.exits,
            "{} lists its codes once, in order",
            row.command
        );
        if row.command == "secret" {
            for code in [125, 126, 127] {
                assert!(has(code), "secret can stop with {code}");
            }
            continue;
        }
        assert!(has(2), "{} fails on a wrong command line", row.command);
        if !row.ops.is_empty() {
            for code in [1, 3, 4] {
                assert!(
                    has(code),
                    "{} calls the daemon, so it can exit {code}",
                    row.command
                );
            }
        }
    }
}

#[test]
fn the_commands_that_wait_can_run_out_of_time_and_say_so() {
    for command in ["ask", "asks"] {
        let row = rows().find(|r| r.command == command).unwrap();
        assert!(row.exits.contains(&124), "{command} can exit 124");
    }
}

fn sorted(value: &Value) -> Value {
    match value {
        Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            let mut out = serde_json::Map::new();
            for key in keys {
                out.insert(key.clone(), sorted(&map[key]));
            }
            Value::Object(out)
        }
        Value::Array(items) => Value::Array(items.iter().map(sorted).collect()),
        other => other.clone(),
    }
}

fn one_per_line(value: &Value) -> String {
    let value = sorted(value);
    let parts: Vec<String> = value
        .as_object()
        .unwrap()
        .iter()
        .map(|(key, part)| match part {
            Value::Array(items) if !items.is_empty() => {
                let lines: Vec<String> = items.iter().map(|item| format!("    {item}")).collect();
                format!("  \"{key}\": [\n{}\n  ]", lines.join(",\n"))
            }
            other => format!("  \"{key}\": {other}"),
        })
        .collect();
    format!("{{\n{}\n}}\n", parts.join(",\n"))
}

fn table(rows: Vec<Vec<String>>, head: &[&str]) -> String {
    let mut out = format!(
        "| {} |\n| {} |\n",
        head.join(" | "),
        vec!["---"; head.len()].join(" | ")
    );
    for row in rows {
        out.push_str(&format!("| {} |\n", row.join(" | ")));
    }
    out
}

fn markdown() -> String {
    let mut out = String::from("# Agent paths\n\nWritten by `NOTCHLING_UPDATE_GOLDEN=1 cargo test -p notchling-notchlingd commands`; do not edit. Every op of the local API is a command's op, planned for a phase of the agents-first design, user-only or internal. `notchling help --json` says the same to an agent.\n\n## Commands\n\n");
    out.push_str(&table(
        rows()
            .map(|r| {
                let ops: Vec<String> = r
                    .ops
                    .iter()
                    .map(|o| {
                        format!(
                            "`{}` ({}{})",
                            o.op,
                            o.tier,
                            if o.acts { ", acts" } else { "" }
                        )
                    })
                    .collect();
                vec![
                    format!("`{}`", r.usage),
                    if ops.is_empty() {
                        "none".into()
                    } else {
                        ops.join(", ")
                    },
                    r.consent.to_string(),
                    r.exits
                        .iter()
                        .map(i32::to_string)
                        .collect::<Vec<_>>()
                        .join(" "),
                ]
            })
            .collect(),
        &["Command", "Ops", "Consent", "Exits"],
    ));
    out.push_str("\n## Planned\n\n");
    out.push_str(&table(
        PLANNED
            .iter()
            .map(|p| {
                vec![
                    format!("`{}`", p.op),
                    p.feature.to_string(),
                    p.phase.to_string(),
                ]
            })
            .collect(),
        &["Op", "Feature", "Phase"],
    ));
    out.push_str("\n## User only\n\n");
    out.push_str(&table(
        USER_ONLY
            .iter()
            .map(|u| {
                vec![
                    format!("`{}`", u.op),
                    u.feature.to_string(),
                    u.why.to_string(),
                ]
            })
            .collect(),
        &["Op", "Feature", "Why"],
    ));
    out.push_str("\n## Internal\n\n");
    out.push_str(&table(
        INTERNAL
            .iter()
            .map(|i| vec![format!("`{}`", i.op), i.why.to_string()])
            .collect(),
        &["Op", "Why"],
    ));
    out
}

#[test]
fn the_golden_files_match_the_registry() {
    let json = one_per_line(&registry_value());
    golden(
        root().join("crates/notchlingd/tests/golden/agent-paths.json"),
        json,
    );
    golden(root().join("docs/agent-paths.md"), markdown());
}

#[test]
fn the_consent_commands_and_the_requests_that_need_no_switch() {
    let find = |name: &str| rows().find(|r| r.command == name).unwrap();
    let get = find("consent");
    assert_eq!((get.ops[0].op, get.ops[0].acts), ("consent.get", false));
    assert!(get.exits.contains(&6) && get.exits.contains(&124));
    let mine = find("consents");
    assert_eq!((mine.ops[0].op, mine.ops[0].acts), ("consent.mine", false));
    let cancel = find("consent cancel");
    assert_eq!(
        (cancel.ops[0].op, cancel.ops[0].acts),
        ("consent.cancel", true)
    );
    let secret = find("secret-request");
    assert_eq!(secret.ops[0].op, "vault.request");
    assert_eq!(
        secret.ops[0].switch, None,
        "a secret request never needs the Vault on"
    );
    assert!(secret.waits && secret.idempotent);
    assert_eq!(why_not("consent.answer").unwrap().1["reason"], "never");
    assert_eq!(why_not("consent.opened").unwrap().1["reason"], "app_only");
    assert_eq!(why_not("consent.list").unwrap().1["reason"], "app_only");
}

#[test]
fn no_planned_op_is_left_for_phase_8_and_a_removal_is_never_an_agents() {
    assert!(PLANNED.iter().all(|p| p.phase != 8));
    let (_, data) = why_not("install.remove").unwrap();
    assert_eq!(data["reason"], "never");
}

#[test]
fn every_phase_8_op_has_a_row_or_is_the_apps() {
    let named = |op: &str| {
        rows().any(|r| r.ops.iter().any(|o| o.op == op)) || INTERNAL.iter().any(|i| i.op == op)
    };
    for op in [
        "machines.list",
        "machines.check",
        "machines.plan",
        "machines.peers",
        "machines.request",
        "machines.apply",
        "network.list",
        "install.request",
        "cost.request",
        "versions.request",
        "tailscale.switch",
        "versions.update_local",
        "cost.profile.set",
        "agy.sidecar.setup",
    ] {
        assert!(named(op), "{op}");
    }
}

#[test]
fn every_switch_is_named_in_the_switch_rows_and_the_usage() {
    let names = crate::switch_asks::NAMES;
    assert_eq!(names.len(), 7);
    assert!(names.contains(&"machines"));
    assert_eq!(crate::capabilities_ops::SWITCH_NAMES, names);
    let joined = names.join("|");
    for command in ["switch on", "switch off"] {
        let row = rows().find(|r| r.command == command).unwrap();
        assert!(row.usage.contains(&format!("<{joined}>")), "{}", row.usage);
    }
    let usage = crate::switch_cli::USAGE;
    assert_eq!(usage.matches(&format!("<{joined}>")).count(), 2, "{usage}");
    let list = rows().find(|r| r.command == "switches").unwrap();
    assert!(list.summary.starts_with("the seven daemon switches: "));
    assert!(
        list.summary.ends_with(&names.join(", ")),
        "{}",
        list.summary
    );
}

#[test]
fn check_updates_has_a_row_that_waits_and_forwards_to_the_setting() {
    let row = rows().find(|r| r.command == "check-updates").unwrap();
    assert!(row.usage.starts_with("notchling check-updates [on|off]"));
    assert!(row.usage.contains("--json") && row.json && row.waits && row.idempotent);
    assert_eq!(row.consent, "by_key_class");
    assert_eq!(row.exits, ACT_EXITS);
    assert_eq!(
        parse(["check-updates".to_string(), "on".to_string()]),
        Ok(Command::CheckUpdates {
            args: vec!["on".to_string()]
        })
    );
}

#[test]
fn the_phase_8_requests_that_install_or_update_need_the_install_switch() {
    let switch_of = |op: &str| command_of(op).unwrap().1.switch;
    for op in ["install.request", "cost.request", "versions.request"] {
        assert_eq!(switch_of(op), Some("install"), "{op}");
    }
    assert_eq!(switch_of("machines.request"), Some("machines"));
    assert_eq!(switch_of("cost.profile.get"), None);
    assert_eq!(switch_of("install.plan"), None);
    let plan = command_of("install.plan").unwrap();
    assert_eq!(
        (plan.0.command, plan.1.tier, plan.1.acts),
        ("install plan", "agent", false)
    );
    assert!(INTERNAL.iter().all(|i| i.op != "install.plan"));
}

#[test]
fn the_machines_row_names_only_actions_phase_8_builds_and_key_is_always_the_idempotency_key() {
    let row = rows().find(|r| r.command == "machines add").unwrap();
    assert!(!row.usage.contains("rename"), "{}", row.usage);
    for row in rows() {
        for (at, _) in row.usage.match_indices("--key") {
            assert!(
                row.usage[at..].starts_with("--key <text>"),
                "{}: --key is the idempotency key, never a file",
                row.command
            );
        }
        assert!(
            !row.usage.contains("--identity") && !row.usage.contains("--ssh-key"),
            "{}",
            row.command
        );
    }
}
