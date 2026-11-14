use crate::log_queue::Stamped;
use notchling_events::field::FieldValue;
use std::collections::HashMap;

pub const WINDOW_MS: i64 = 1_000;

const FOLDED: [&str; 2] = ["api.refused", "api.unknown_peer"];
const KEY_FIELDS: [&str; 4] = ["op", "exe", "why", "switch"];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FoldAction {
    Write,
    Hold,
}

struct Group {
    first_ts: i64,
    held: u32,
    last: Option<Stamped>,
}

#[derive(Default)]
pub struct Fold {
    groups: HashMap<String, Group>,
}

fn text(value: &FieldValue) -> String {
    match value {
        FieldValue::Name(name) => (*name).to_owned(),
        FieldValue::Token(token) => token.to_string(),
        _ => String::new(),
    }
}

fn key_of(stamped: &Stamped) -> Option<String> {
    let event = &stamped.event;
    if stamped.commit.is_some() || !FOLDED.contains(&event.spec.name) {
        return None;
    }
    let mut key = event.spec.name.to_owned();
    for name in KEY_FIELDS {
        key.push('\u{1}');
        if let Some(value) = event.fields.get(name) {
            key.push_str(&text(value));
        }
    }
    Some(key)
}

impl Fold {
    pub fn offer(&mut self, stamped: &Stamped, now_ms: i64) -> FoldAction {
        let Some(key) = key_of(stamped) else {
            return FoldAction::Write;
        };
        match self.groups.get_mut(&key) {
            Some(group) if now_ms - group.first_ts < WINDOW_MS => {
                group.held += 1;
                group.last = Some(stamped.clone());
                FoldAction::Hold
            }
            _ => {
                self.groups.insert(
                    key,
                    Group {
                        first_ts: now_ms,
                        held: 0,
                        last: None,
                    },
                );
                FoldAction::Write
            }
        }
    }

    pub fn due(&mut self, now_ms: i64) -> Vec<Stamped> {
        let mut out = Vec::new();
        self.groups.retain(|_, group| {
            if now_ms - group.first_ts < WINDOW_MS {
                return true;
            }
            if let Some(mut last) = group.last.take() {
                last.env.repeat = Some(group.held);
                out.push(last);
            }
            false
        });
        out.sort_by_key(|s| s.env.seq);
        out
    }
}
