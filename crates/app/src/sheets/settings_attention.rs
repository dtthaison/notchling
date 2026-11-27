use std::sync::atomic::{AtomicBool, Ordering};

use notchling_locale::{self as t, Text};
use serde_json::Value;

use super::settings::{choice, value, Field, Press, Row, Section};

pub const KEY: &str = "attentionLevel";

static DENIED: AtomicBool = AtomicBool::new(false);

pub fn set_denied(denied: bool) {
    DENIED.store(denied, Ordering::Relaxed);
}

pub fn denied() -> bool {
    DENIED.load(Ordering::Relaxed)
}

const LEVELS: [(&str, Text); 3] = [
    ("calm", t::settings::ATTENTION_LEVEL_CALM),
    ("normal", t::settings::ATTENTION_LEVEL_NORMAL),
    ("urgent", t::settings::ATTENTION_LEVEL_URGENT),
];

pub fn section(read: &dyn Fn(&str) -> Option<Value>, notch_shown: bool, denied: bool) -> Section {
    let level = choice(read, KEY, t::settings::attention_level(), &LEVELS);
    let mut rows = vec![
        level,
        Row {
            key: "",
            title: t::settings::attention_sound(),
            field: Field::Press(Press::PlaySound),
        },
    ];
    if denied {
        rows.push(Row {
            key: "",
            title: t::settings::attention_denied(),
            field: Field::Press(Press::OpenNotificationSettings),
        });
    }
    Section {
        title: t::settings::section_attention(),
        rows,
        footer: Some(if notch_shown {
            t::settings::attention_footer()
        } else {
            t::settings::attention_footer_no_notch()
        }),
    }
}

pub fn notch_shown(read: &dyn Fn(&str) -> Option<Value>) -> bool {
    !value(read, "hideNotch")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
}
