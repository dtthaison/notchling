use notchling_engine::settings::spec;
use serde_json::{json, Value};

use crate::acting::Acting;
use crate::appearance;
use crate::chat::history;
use crate::search::shortcut;
use crate::switches::restart::Area;

#[derive(Clone, Debug, PartialEq)]
pub enum Field {
    Toggle {
        on: bool,
        inverted: bool,
    },
    Choice {
        options: Vec<(&'static str, &'static str)>,
        selected: String,
    },
    Stepper {
        value: i64,
        min: i64,
        max: i64,
    },
    ReadOnly {
        value: String,
        note: Option<String>,
    },
    Daemon {
        area: Area,
        on: Option<bool>,
    },
    Shortcut(shortcut::Setting),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Daemon {
    pub acting: Acting,
    pub bridge: Option<bool>,
    pub accounts: Option<bool>,
    pub vault: Option<bool>,
    pub install: Option<bool>,
}

impl Daemon {
    pub fn detached() -> Daemon {
        Daemon {
            acting: Acting::Detached,
            bridge: None,
            accounts: None,
            vault: None,
            install: None,
        }
    }

    fn on(&self, area: Area) -> Option<bool> {
        match area {
            Area::Acting => match self.acting {
                Acting::On => Some(true),
                Acting::Off => Some(false),
                Acting::Detached => None,
            },
            Area::Bridge => self.bridge,
            Area::Accounts => self.accounts,
            Area::Vault => self.vault,
            Area::Install => self.install,
        }
    }
}

const DAEMON_ROWS: [(Area, &str); 5] = [
    (Area::Acting, "Act on Sessions"),
    (Area::Bridge, "Phone Bridge"),
    (Area::Accounts, "Logins and Lanes"),
    (Area::Vault, "Secret Vault"),
    (Area::Install, "Install Hooks and Skills"),
];

#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub key: &'static str,
    pub title: &'static str,
    pub field: Field,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Section {
    pub title: &'static str,
    pub rows: Vec<Row>,
    pub footer: Option<&'static str>,
}

const AWAKE_MODES: [(&str, &str); 3] = [
    ("off", "Off"),
    ("on", "On"),
    ("whileWorking", "While Agents Work"),
];
const WIDTHS: [(&str, &str); 3] = [("small", "Small"), ("medium", "Medium"), ("large", "Large")];
const LIMIT_MODES: [(&str, &str); 3] = [
    ("off", "Off"),
    ("notify", "Notify"),
    ("notifyAndSwitch", "Notify and Switch"),
];

fn value(read: &dyn Fn(&str) -> Option<Value>, key: &str) -> Option<Value> {
    read(key).or_else(|| spec(key).and_then(|s| s.default_value()))
}

fn toggle(read: &dyn Fn(&str) -> Option<Value>, key: &'static str, title: &'static str) -> Row {
    Row {
        key,
        title,
        field: Field::Toggle {
            on: value(read, key).and_then(|v| v.as_bool()).unwrap_or(false),
            inverted: false,
        },
    }
}

fn choice(
    read: &dyn Fn(&str) -> Option<Value>,
    key: &'static str,
    title: &'static str,
    options: &[(&'static str, &'static str)],
) -> Row {
    let selected = value(read, key)
        .and_then(|v| v.as_str().map(str::to_owned))
        .unwrap_or_default();
    Row {
        key,
        title,
        field: Field::Choice {
            options: options.to_vec(),
            selected,
        },
    }
}

pub fn sections(read: &dyn Fn(&str) -> Option<Value>, daemon: &Daemon) -> Vec<Section> {
    let mut show_notch = toggle(read, "hideNotch", "Show agents in the notch");
    if let Field::Toggle { on, inverted } = &mut show_notch.field {
        *on = !*on;
        *inverted = true;
    }
    let switching = ["limitMode", "codexLimitMode", "antigravityLimitMode"]
        .iter()
        .any(|k| {
            value(read, k).and_then(|v| v.as_str().map(str::to_owned))
                == Some("notifyAndSwitch".into())
        });
    let mut limits = vec![
        choice(read, "limitMode", "Claude Code", &LIMIT_MODES),
        choice(read, "codexLimitMode", "Codex", &LIMIT_MODES),
        choice(read, "antigravityLimitMode", "Antigravity", &LIMIT_MODES),
    ];
    if switching {
        let (min, max) = spec("limitSwitchAt")
            .and_then(|s| s.range)
            .unwrap_or((80, 99));
        limits.push(Row {
            key: "limitSwitchAt",
            title: "Switch at",
            field: Field::Stepper {
                value: value(read, "limitSwitchAt")
                    .and_then(|v| v.as_i64())
                    .unwrap_or(95)
                    .clamp(min, max),
                min,
                max,
            },
        });
    }
    vec![
        Section {
            title: "General",
            rows: vec![choice(
                read,
                appearance::KEY,
                "Appearance",
                &appearance::CHOICES,
            )],
            footer: None,
        },
        Section {
            title: "Search",
            rows: vec![Row {
                key: shortcut::KEY,
                title: "Search Shortcut",
                field: Field::Shortcut(
                    value(read, shortcut::KEY)
                        .map(|v| shortcut::Setting::from_value(&v))
                        .unwrap_or(shortcut::Setting::DEFAULT),
                ),
            }],
            footer: Some(
                "Opens Search from any app, so no other app can use this key. Inside \
                 Notchling, ⌘⇧P always opens Search.",
            ),
        },
        Section {
            title: "Daemon",
            rows: DAEMON_ROWS
                .iter()
                .map(|(area, title)| Row {
                    key: "",
                    title,
                    field: Field::Daemon {
                        area: *area,
                        on: daemon.on(*area),
                    },
                })
                .collect(),
            footer: Some(
                "Each is off whenever the daemon starts. After a restart a notice offers to turn \
                 back on the ones you had on.",
            ),
        },
        Section {
            title: "Keep Mac Awake",
            rows: vec![
                choice(read, "mode", "Keep awake", &AWAKE_MODES),
                toggle(read, "keepDisplayOn", "Keep display on"),
                toggle(read, "continueAfterSleep", "Continue agents after sleep"),
            ],
            footer: Some(
                "Closing the lid still puts the Mac to sleep. When it wakes, background agents \
                 whose reply was cut off by sleep are told to continue.",
            ),
        },
        Section {
            title: "Notch",
            rows: vec![
                show_notch,
                choice(read, "notchWidth", "Width", &WIDTHS),
                toggle(read, "showPet", "Animate pet"),
                toggle(read, "showLastReply", "Show the last reply when peeking"),
                toggle(
                    read,
                    "replyFromNotch",
                    "Reply to background agents from the notch",
                ),
            ],
            footer: None,
        },
        Section {
            title: "Claude Code",
            rows: vec![
                toggle(read, "answerFromNotch", "Answer prompts from the notch"),
                toggle(read, "keepPages", "Keep local copies of artifacts"),
            ],
            footer: None,
        },
        Section {
            title: "When the Account in Use Nears Its Limit",
            rows: limits,
            footer: Some(
                "Notify tells you at 90% and 100% of a limit. Notify and Switch also moves to the \
                 saved account with the most room.",
            ),
        },
        Section {
            title: "Codex",
            rows: vec![toggle(read, "codexNetwork", "Allow network in Codex turns")],
            footer: None,
        },
        Section {
            title: "Chat",
            rows: vec![choice(
                read,
                history::KEY,
                "Conversation history to load",
                &history::CHOICES,
            )],
            footer: Some(
                "Chat shows the newest part of a long session. More shows earlier turns and uses \
                 more memory. It applies the next time a session loads.",
            ),
        },
    ]
}

pub fn change(row: &Row, option: &str) -> Option<(&'static str, Value)> {
    match &row.field {
        Field::Toggle { on, inverted } => {
            let shown = !*on;
            Some((row.key, json!(if *inverted { !shown } else { shown })))
        }
        Field::Choice { options, .. } => options
            .iter()
            .find(|(v, _)| *v == option)
            .map(|(v, _)| (row.key, json!(v))),
        Field::Stepper { value, min, max } => {
            let step = match option {
                "+1" => 1,
                "-1" => -1,
                _ => return None,
            };
            Some((row.key, json!((value + step).clamp(*min, *max))))
        }
        Field::ReadOnly { .. } | Field::Daemon { .. } | Field::Shortcut(_) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn with(acting: Acting) -> Daemon {
        Daemon {
            acting,
            ..Daemon::detached()
        }
    }

    fn rows_of(read: &dyn Fn(&str) -> Option<Value>, acting: Acting) -> Vec<Row> {
        sections(read, &with(acting))
            .into_iter()
            .flat_map(|s| s.rows)
            .collect()
    }

    fn row(rows: &[Row], key: &str) -> Row {
        rows.iter()
            .find(|r| r.key == key)
            .cloned()
            .unwrap_or_else(|| panic!("{key}"))
    }

    #[test]
    fn an_empty_store_shows_the_defaults() {
        let rows = rows_of(&|_| None, Acting::Off);
        assert_eq!(
            row(&rows, "showPet").field,
            Field::Toggle {
                on: true,
                inverted: false
            }
        );
        assert_eq!(
            row(&rows, "hideNotch").field,
            Field::Toggle {
                on: true,
                inverted: true
            },
            "hideNotch false shows the notch"
        );
        let Field::Choice { selected, .. } = row(&rows, "notchWidth").field else {
            panic!()
        };
        assert_eq!(selected, "medium");
        assert_eq!(
            row(&rows, "limitSwitchAt").field,
            Field::Stepper {
                value: 95,
                min: 80,
                max: 99
            }
        );
        for r in rows.iter().filter(|r| !r.key.is_empty()) {
            assert!(spec(r.key).is_some(), "{}", r.key);
        }
    }

    #[test]
    fn stored_values_win_and_the_stepper_hides_without_switching() {
        let store: HashMap<&str, Value> = [
            ("limitMode", json!("notify")),
            ("codexLimitMode", json!("off")),
            ("showPet", json!(false)),
            ("mode", json!("whileWorking")),
        ]
        .into_iter()
        .collect();
        let read = |k: &str| store.get(k).cloned();
        let rows = rows_of(&read, Acting::On);
        assert!(rows.iter().all(|r| r.key != "limitSwitchAt"));
        assert_eq!(
            row(&rows, "showPet").field,
            Field::Toggle {
                on: false,
                inverted: false
            }
        );
        let Field::Choice { selected, options } = row(&rows, "antigravityLimitMode").field else {
            panic!()
        };
        assert_eq!(selected, "notify");
        assert_eq!(options.len(), 3, "Antigravity switches too");
    }

    #[test]
    fn the_daemons_switches_follow_the_app_wide_sections_with_what_it_said() {
        let daemon = Daemon {
            acting: Acting::On,
            bridge: Some(false),
            accounts: Some(true),
            vault: None,
            install: Some(false),
        };
        let first = sections(&|_| None, &daemon).remove(2);
        assert_eq!(first.title, "Daemon");
        let fields: Vec<Field> = first.rows.iter().map(|r| r.field.clone()).collect();
        assert_eq!(
            fields,
            [
                Field::Daemon {
                    area: Area::Acting,
                    on: Some(true)
                },
                Field::Daemon {
                    area: Area::Bridge,
                    on: Some(false)
                },
                Field::Daemon {
                    area: Area::Accounts,
                    on: Some(true)
                },
                Field::Daemon {
                    area: Area::Vault,
                    on: None
                },
                Field::Daemon {
                    area: Area::Install,
                    on: Some(false)
                },
            ]
        );
        assert_eq!(change(&first.rows[0], ""), None, "not a settings key");
        let off = sections(&|_| None, &with(Acting::Off)).remove(2);
        assert_eq!(
            off.rows[0].field,
            Field::Daemon {
                area: Area::Acting,
                on: Some(false)
            }
        );
        let none = sections(&|_| None, &Daemon::detached()).remove(2);
        assert!(none
            .rows
            .iter()
            .all(|r| matches!(r.field, Field::Daemon { on: None, .. })));
    }

    #[test]
    fn the_general_section_offers_appearance_with_system_the_default() {
        let general = sections(&|_| None, &with(Acting::Off)).remove(0);
        assert_eq!(general.title, "General");
        assert_eq!(general.rows.len(), 1);
        let row = &general.rows[0];
        assert_eq!(row.title, "Appearance");
        let Field::Choice { selected, options } = row.field.clone() else {
            panic!("a choice");
        };
        assert_eq!(selected, "system");
        let names: Vec<&str> = options.iter().map(|(_, label)| *label).collect();
        assert_eq!(names, ["System", "Light", "Dark"]);
        assert_eq!(change(row, "dark"), Some(("appearance", json!("dark"))));
        assert_eq!(change(row, "sepia"), None);
    }

    #[test]
    fn the_app_wide_sections_come_first_and_chat_is_last() {
        let titles: Vec<&str> = sections(&|_| None, &with(Acting::Off))
            .iter()
            .map(|s| s.title)
            .collect();
        assert_eq!(
            titles,
            [
                "General",
                "Search",
                "Daemon",
                "Keep Mac Awake",
                "Notch",
                "Claude Code",
                "When the Account in Use Nears Its Limit",
                "Codex",
                "Chat"
            ]
        );
        let rows = rows_of(&|_| None, Acting::Off);
        let history = row(&rows, "chatHistory");
        assert_eq!(history.title, "Conversation history to load");
        let Field::Choice { selected, options } = history.field.clone() else {
            panic!()
        };
        assert_eq!(selected, "4mb", "four megabytes until chosen");
        let values: Vec<&str> = options.iter().map(|(v, _)| *v).collect();
        assert_eq!(values, ["2mb", "4mb", "8mb", "16mb"]);
        assert_eq!(
            change(&history, "16mb"),
            Some(("chatHistory", json!("16mb")))
        );
        assert_eq!(change(&history, "1gb"), None);
    }

    #[test]
    fn the_search_shortcut_shows_control_option_p_until_chosen() {
        let rows = rows_of(&|_| None, Acting::Off);
        let shown = row(&rows, "searchShortcut");
        assert_eq!(shown.title, "Search Shortcut");
        assert_eq!(shown.field, Field::Shortcut(shortcut::Setting::DEFAULT));
        assert_eq!(change(&shown, "alt+space"), None, "the recorder writes it");
        let store: HashMap<&str, Value> = [("searchShortcut", json!("off"))].into_iter().collect();
        let read = |k: &str| store.get(k).cloned();
        let off = row(&rows_of(&read, Acting::Off), "searchShortcut");
        assert_eq!(off.field, Field::Shortcut(shortcut::Setting::Off));
        let store: HashMap<&str, Value> = [("searchShortcut", json!("alt+space"))]
            .into_iter()
            .collect();
        let read = |k: &str| store.get(k).cloned();
        let Field::Shortcut(set) = row(&rows_of(&read, Acting::Off), "searchShortcut").field else {
            panic!()
        };
        assert_eq!(set.label(), "⌥Space");
    }

    #[test]
    fn changes_write_the_stored_value() {
        let rows = rows_of(&|_| None, Acting::Off);
        assert_eq!(
            change(&row(&rows, "showPet"), ""),
            Some(("showPet", json!(false)))
        );
        assert_eq!(
            change(&row(&rows, "hideNotch"), ""),
            Some(("hideNotch", json!(true)))
        );
        assert_eq!(
            change(&row(&rows, "notchWidth"), "large"),
            Some(("notchWidth", json!("large")))
        );
        assert_eq!(change(&row(&rows, "notchWidth"), "huge"), None);
        let step = row(&rows, "limitSwitchAt");
        assert_eq!(change(&step, "+1"), Some(("limitSwitchAt", json!(96))));
        assert_eq!(change(&step, "-1"), Some(("limitSwitchAt", json!(94))));
        assert_eq!(change(&step, "x"), None);
        let top = Row {
            field: Field::Stepper {
                value: 99,
                min: 80,
                max: 99,
            },
            ..step
        };
        assert_eq!(change(&top, "+1"), Some(("limitSwitchAt", json!(99))));
    }
}
