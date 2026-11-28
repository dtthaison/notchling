use std::collections::BTreeMap;

use serde_json::{json, Value};

use super::agents::Tool;
use super::settings::{every_section, sections, shared_agent_sections, tool_sections, Daemon, Row};
use super::sidecar_rows;

fn keys(sections: Vec<super::settings::Section>) -> Vec<&'static str> {
    sections
        .into_iter()
        .flat_map(|s| s.rows)
        .map(|r| r.key)
        .collect()
}

fn switching(key: &str) -> Option<Value> {
    (key == "codexLimitMode").then(|| json!("notifyAndSwitch"))
}

fn notifying(key: &str) -> Option<Value> {
    matches!(key, "limitMode" | "codexLimitMode" | "antigravityLimitMode").then(|| json!("notify"))
}

#[test]
fn the_tabs_are_the_providers_in_the_order_they_have_elsewhere() {
    assert_eq!(
        Tool::ALL.map(Tool::title),
        ["Claude Code", "Codex", "Antigravity"]
    );
    assert_eq!(
        Tool::ALL.map(Tool::contract),
        notchling_engine::contract::Tool::ALL
    );
    assert_eq!(Tool::ALL[0], Tool::ClaudeCode, "the default tab");
}

#[test]
fn ids_round_trip() {
    for tool in Tool::ALL {
        assert_eq!(Tool::from_id(tool.id()), Some(tool));
    }
    assert_eq!(Tool::from_id("gemini"), None);
}

#[test]
fn each_tab_shows_only_its_providers_rows() {
    let read = |_: &str| None::<Value>;
    assert_eq!(
        keys(tool_sections(Tool::ClaudeCode, &read)),
        ["answerFromNotch", "keepPages"]
    );
    assert_eq!(keys(tool_sections(Tool::Codex, &read)), ["codexNetwork"]);
    assert_eq!(
        keys(tool_sections(Tool::Antigravity, &read)),
        ["antigravityChannel", "antigravityRemoteIn"]
    );
}

#[test]
fn the_account_limit_rows_are_shared_above_the_tabs() {
    assert_eq!(
        keys(shared_agent_sections(&notifying)),
        ["limitMode", "codexLimitMode", "antigravityLimitMode"]
    );
    assert_eq!(
        keys(shared_agent_sections(&switching)),
        [
            "limitMode",
            "codexLimitMode",
            "antigravityLimitMode",
            "limitSwitchAt"
        ]
    );
}

#[test]
fn the_general_page_no_longer_holds_a_providers_rows() {
    let general = keys(sections(&|_| None, &Daemon::detached()));
    for gone in [
        "answerFromNotch",
        "keepPages",
        "codexNetwork",
        "antigravityChannel",
        "antigravityRemoteIn",
        "limitMode",
        "codexLimitMode",
        "antigravityLimitMode",
    ] {
        assert!(!general.contains(&gone), "{gone}");
    }
    assert!(general.contains(&"appearance") && general.contains(&"chatHistory"));
}

#[test]
fn every_row_there_was_is_in_exactly_one_place() {
    let before = [
        "appearance",
        "checkForUpdates",
        "showInDock",
        "searchShortcut",
        "semanticSearch",
        "searchIndexRange",
        "searchCategories",
        "",
        "mode",
        "keepDisplayOn",
        "continueAfterSleep",
        "hideNotch",
        "notchWidth",
        "showPet",
        "showLastReply",
        "replyFromNotch",
        "answerFromNotch",
        "keepPages",
        "limitMode",
        "codexLimitMode",
        "antigravityLimitMode",
        "limitSwitchAt",
        "codexNetwork",
        "antigravityChannel",
        "antigravityRemoteIn",
        "chatHistory",
        "keepFinishedSessions",
        "attentionLevel",
    ];
    let mut places: BTreeMap<&str, usize> = BTreeMap::new();
    let mut count = |rows: Vec<Row>| {
        for row in rows {
            if !row.key.is_empty() {
                *places.entry(row.key).or_default() += 1;
            }
        }
    };
    let read = switching;
    let daemon = Daemon::detached();
    count(
        sections(&read, &daemon)
            .into_iter()
            .flat_map(|s| s.rows)
            .collect(),
    );
    count(
        shared_agent_sections(&read)
            .into_iter()
            .flat_map(|s| s.rows)
            .collect(),
    );
    for tool in Tool::ALL {
        count(
            tool_sections(tool, &read)
                .into_iter()
                .flat_map(|s| s.rows)
                .collect(),
        );
    }
    for key in before.iter().filter(|k| !k.is_empty()) {
        assert_eq!(places.get(key), Some(&1), "{key}");
    }
    let keyed = before.iter().filter(|k| !k.is_empty()).count();
    assert_eq!(places.len(), keyed, "no row is new");
    let all: usize = every_section(&read, &daemon)
        .iter()
        .map(|s| s.rows.len())
        .sum();
    assert_eq!(
        all,
        before.len() - 1 + 6 + 1,
        "the six daemon rows and the sound row have no key"
    );
}

#[test]
fn the_sidecar_row_belongs_to_the_antigravity_tab_when_app_full_is_chosen() {
    let chosen = json!("sidecar");
    assert!(sidecar_rows::is_chosen(Some(&chosen)));
    assert!(sidecar_rows::visible(true, None));
    assert!(!sidecar_rows::visible(false, None));
    let read = |key: &str| (key == "antigravityChannel").then(|| json!("sidecar"));
    let rows: Vec<Row> = tool_sections(Tool::Antigravity, &read)
        .into_iter()
        .flat_map(|s| s.rows)
        .collect();
    let channel = rows
        .iter()
        .find(|r| r.key == sidecar_rows::CHANNEL_KEY)
        .expect("the channel row is on the Antigravity tab");
    assert!(matches!(
        &channel.field,
        super::settings::Field::List { selected, options } if selected == sidecar_rows::CHANNEL
            && options.len() == 4
    ));
    for tool in [Tool::ClaudeCode, Tool::Codex] {
        assert!(tool_sections(tool, &read)
            .iter()
            .flat_map(|s| &s.rows)
            .all(|r| r.key != sidecar_rows::CHANNEL_KEY));
    }
}
