//! The Settings sheet, pure (part U8, the Swift `SettingsPage`): the Milestone 1 settings keys in
//! the Swift app's sections, each row reading the store (or the key's default), after the daemon's
//! acting switch shown read-only with its state. Launch at login is the system's login item, not a
//! key, so it waits for its part.

use notchling_engine::settings::spec;
use serde_json::{json, Value};

use crate::acting::Acting;

/// How a row is changed.
#[derive(Clone, Debug, PartialEq)]
pub enum Field {
    /// A switch. `inverted` shows the opposite of the stored value ("Show agents in the notch" is
    /// `hideNotch` off).
    Toggle { on: bool, inverted: bool },
    /// One of a few values, each with its title.
    Choice {
        options: Vec<(&'static str, &'static str)>,
        selected: String,
    },
    /// A whole number stepped within a range.
    Stepper { value: i64, min: i64, max: i64 },
    /// Shown, never changed here.
    ReadOnly { value: String, note: Option<String> },
}

#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    /// The settings key; empty for a read-only row that isn't one.
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

/// The key's value from `read`, else its default.
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

/// The sheet's sections over `read` (the store's raw values) and acting as the app knows it.
pub fn sections(read: &dyn Fn(&str) -> Option<Value>, acting: Acting) -> Vec<Section> {
    let mut show_notch = toggle(read, "hideNotch", "Show agents in the notch");
    if let Field::Toggle { on, inverted } = &mut show_notch.field {
        *on = !*on;
        *inverted = true;
    }
    let switching = ["limitMode", "codexLimitMode"].iter().any(|k| {
        value(read, k).and_then(|v| v.as_str().map(str::to_owned)) == Some("notifyAndSwitch".into())
    });
    let mut limits = vec![
        choice(read, "limitMode", "Claude Code", &LIMIT_MODES),
        choice(read, "codexLimitMode", "Codex", &LIMIT_MODES),
        choice(
            read,
            "antigravityLimitMode",
            "Antigravity",
            &LIMIT_MODES[..2],
        ),
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
    // The daemon's switch first: it decides whether anything here can act at all.
    vec![
        Section {
            title: "Daemon",
            rows: vec![Row {
                key: "",
                title: "Acting on sessions",
                field: Field::ReadOnly {
                    value: acting.word().to_owned(),
                    note: acting.message().map(str::to_owned),
                },
            }],
            footer: Some(
                "Only the maintainer turns acting on, with the daemon's settings.set_acting. \
                 Until then the Swift app answers, replies and starts sessions.",
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
    ]
}

/// What a click on a row writes: a toggle flips (stored inverted where it shows inverted), a
/// choice takes `option`, a stepper moves by `option` read as +1 or -1. `None` for a read-only
/// row or an option that isn't one.
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
        Field::ReadOnly { .. } => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::acting::OFF_MESSAGE;
    use std::collections::HashMap;

    fn rows_of(read: &dyn Fn(&str) -> Option<Value>, acting: Acting) -> Vec<Row> {
        sections(read, acting)
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
        // limitMode defaults to notifyAndSwitch, so the stepper shows at 95.
        assert_eq!(
            row(&rows, "limitSwitchAt").field,
            Field::Stepper {
                value: 95,
                min: 80,
                max: 99
            }
        );
        // Every key shown is a real one.
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
        assert_eq!(options.len(), 2, "Antigravity only notifies");
    }

    #[test]
    fn acting_shows_read_only_with_its_state() {
        let daemon = sections(&|_| None, Acting::Off).remove(0);
        assert_eq!(daemon.title, "Daemon");
        let acting = &daemon.rows[0];
        assert_eq!(
            acting.field,
            Field::ReadOnly {
                value: "Off".into(),
                note: Some(OFF_MESSAGE.into())
            }
        );
        assert_eq!(change(acting, "+1"), None, "never changed here");
        let on = sections(&|_| None, Acting::On).remove(0);
        assert_eq!(
            on.rows[0].field,
            Field::ReadOnly {
                value: "On".into(),
                note: None
            }
        );
    }

    #[test]
    fn changes_write_the_stored_value() {
        let rows = rows_of(&|_| None, Acting::Off);
        assert_eq!(
            change(&row(&rows, "showPet"), ""),
            Some(("showPet", json!(false)))
        );
        // Turning "Show agents in the notch" off stores hideNotch true.
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
