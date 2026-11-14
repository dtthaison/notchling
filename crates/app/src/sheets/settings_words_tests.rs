use std::collections::HashSet;

use notchling_locale::{self as t, lookup};
use serde_json::{json, Value};

use super::agents::{Page, Tool, HOOKS};
use super::capabilities::RowState;
use super::hook_rows::{HookState, HooksShown};
use super::settings::{choice_key, sections, title_key, Daemon, Field, Row};
use crate::acting::Acting;
use crate::welcome::hooks::DETACHED;

fn every_row() -> Vec<Row> {
    let store = |key: &str| match key {
        "limitMode" => Some(json!("notifyAndSwitch")),
        _ => None::<Value>,
    };
    let daemon = Daemon {
        acting: Acting::Off,
        ..Daemon::detached()
    };
    sections(&store, &daemon)
        .into_iter()
        .flat_map(|s| s.rows)
        .collect()
}

#[test]
fn kebab_keys() {
    assert_eq!(title_key("hideNotch"), "settings-hide-notch");
    assert_eq!(title_key("codexLimitMode"), "settings-codex-limit-mode");
    assert_eq!(title_key("appearance"), "settings-appearance");
    assert_eq!(
        choice_key("mode", "whileWorking"),
        "settings-mode-while-working"
    );
    assert_eq!(
        choice_key("antigravityLimitMode", "notifyAndSwitch"),
        "settings-antigravity-limit-mode-notify-and-switch"
    );
    assert_eq!(
        choice_key("chatHistory", "2mb"),
        "settings-chat-history-v2mb",
        "a word of a key starts with a letter"
    );
}

#[test]
fn every_keyed_row_has_its_message() {
    let rows = every_row();
    let keyed: Vec<&Row> = rows.iter().filter(|r| !r.key.is_empty()).collect();
    assert!(keyed.len() >= 17, "{}", keyed.len());
    for row in keyed {
        let text = lookup(&title_key(row.key))
            .unwrap_or_else(|| panic!("no message {} for {}", title_key(row.key), row.key));
        assert_eq!(text.english(), row.title, "{}", row.key);
    }
}

#[test]
fn rows_with_no_setting_key_are_named_by_meaning() {
    let rows = every_row();
    let titles: Vec<&str> = rows
        .iter()
        .filter(|r| r.key.is_empty())
        .map(|r| r.title)
        .collect();
    assert_eq!(
        titles,
        [
            t::settings::ACT_ON_SESSIONS.english(),
            t::settings::PHONE_BRIDGE.english(),
            t::settings::LOGINS_AND_LANES.english(),
            t::settings::SECRET_VAULT.english(),
            t::settings::INSTALL_HOOKS_AND_SKILLS.english(),
        ]
    );
    assert_eq!(
        t::settings::ACT_ON_SESSIONS.key(),
        "settings-act-on-sessions"
    );
}

#[test]
fn choice_labels_are_per_setting() {
    let rows = every_row();
    let mut seen = HashSet::new();
    let mut choices = 0;
    for row in &rows {
        let Field::Choice { options, .. } = &row.field else {
            continue;
        };
        for (value, label) in options {
            let text = lookup(&choice_key(row.key, value)).unwrap_or_else(|| {
                panic!("no message {} for {}", choice_key(row.key, value), row.key)
            });
            assert_eq!(text.english(), *label, "{} {value}", row.key);
            assert!(seen.insert(text), "{} {value} shares a message", row.key);
            choices += 1;
        }
    }
    assert_eq!(choices, 3 + 3 + 3 * 3 + 3 + 4);
}

#[test]
fn section_titles_and_footers_come_from_messages() {
    let sections = sections(&|_| None, &Daemon::detached());
    let titles: Vec<&str> = sections.iter().map(|s| s.title).collect();
    assert_eq!(
        titles,
        [
            t::settings::SECTION_GENERAL.get(),
            t::settings::SECTION_SEARCH.get(),
            t::settings::SECTION_DAEMON.get(),
            t::settings::SECTION_KEEP_MAC_AWAKE.get(),
            t::settings::SECTION_NOTCH.get(),
            t::settings::SECTION_CLAUDE_CODE.get(),
            t::settings::SECTION_ACCOUNT_LIMIT.get(),
            t::settings::SECTION_CODEX.get(),
            t::settings::SECTION_CHAT.get(),
        ]
    );
    let footers: Vec<Option<&str>> = sections.iter().map(|s| s.footer).collect();
    assert_eq!(
        footers,
        [
            None,
            Some(t::settings::FOOTER_SEARCH.get()),
            Some(t::settings::FOOTER_DAEMON.get()),
            Some(t::settings::FOOTER_KEEP_MAC_AWAKE.get()),
            None,
            None,
            Some(t::settings::FOOTER_ACCOUNT_LIMIT.get()),
            None,
            Some(t::settings::FOOTER_CHAT.get()),
        ]
    );
}

#[test]
fn pages_and_hooks_use_messages() {
    assert_eq!(Page::General.title(), t::settings::page_general());
    assert_eq!(Page::Agents.title(), t::settings::page_agents());
    assert_eq!(Page::Activity.title(), t::settings::page_activity());
    assert_eq!(Tool::ClaudeCode.title(), t::settings::tool_claude_code());
    assert_eq!(Tool::ClaudeCode.title(), "Claude Code");
    assert_eq!(
        Page::ALL.map(Page::id),
        ["general", "agents", "activity"],
        "an element id never changes with the language"
    );
    let installed = HookState::Installed.word();
    assert_eq!(installed, Some(t::settings::hook_installed()));
    assert_eq!(HookState::Missing.word(), Some(t::settings::hook_missing()));
    assert_eq!(HookState::Swift.word(), Some(t::settings::hook_swift()));
    assert_eq!(
        HookState::NotManaged.word(),
        Some(t::settings::hook_not_managed())
    );
    for hook in HOOKS {
        assert!(hook.what.key().starts_with("settings-hook-"), "{hook:?}");
    }
    assert_eq!(
        HooksShown::Checking.status().as_deref(),
        Some(t::settings::hooks_checking())
    );
    assert_eq!(HooksShown::Detached.status().as_deref(), Some(DETACHED));
}

#[test]
fn the_agents_page_words_keep_their_english() {
    assert_eq!(t::settings::HOOKS.english(), "Hooks");
    assert_eq!(t::settings::CLIENT_TITLE.english(), "Client");
    assert_eq!(t::settings::CLIENT_LOOKING.english(), "Looking…");
    assert_eq!(
        t::settings::CAPABILITIES_TITLE.english(),
        "What Notchling Can Do"
    );
    assert_eq!(
        t::settings::CAPABILITIES_FOOTER.english(),
        "Anything not listed works natively for that tool. notchling providers prints every row."
    );
    assert_eq!(RowState::Fallback.word(), "Notchling's own");
    assert_eq!(RowState::NotApplicable.word(), "Not applicable");
    assert_eq!(RowState::Gap.word(), "Not built yet");
    assert_eq!(
        t::settings::client_found("~/.local/bin/claude", "2.1.0"),
        "Found at ~/.local/bin/claude · 2.1.0"
    );
    assert_eq!(
        t::settings::client_found_by("~/.local/bin/claude", "2.1.0", "npm"),
        "Found at ~/.local/bin/claude · 2.1.0 · npm"
    );
}

#[test]
fn the_shortcut_words_keep_their_english() {
    assert_eq!(
        t::settings::SHORTCUT_RECORDING.english(),
        "Press a shortcut…"
    );
    assert_eq!(t::settings::SHORTCUT_RECORD.english(), "Record");
    assert_eq!(t::settings::SHORTCUT_OFF.english(), "Off");
    assert_eq!(
        t::settings::SHORTCUT_NEEDS_KEY.english(),
        "Use a letter, a number or Space with Control, Option or Command."
    );
}
