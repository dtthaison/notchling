use crate::outbox_courier::JobFiles;
use notchling_engine::outbox::Row;
use notchling_engine::sessions::Agent;
use notchling_engine::transcript::locate::{begins_with_clear, successor};
use notchling_engine::transcript::message::Message;
use notchling_engine::transcript::sent::{count_since, Seen};
use std::collections::HashMap;
use std::path::Path;
use std::sync::{Mutex, OnceLock};

pub(crate) const SLACK_MS: i64 = 2_000;

#[derive(Clone)]
pub(crate) enum Anchor {
    Nowhere,
    File(String),
}

fn anchors() -> &'static Mutex<HashMap<String, Anchor>> {
    static ANCHORS: OnceLock<Mutex<HashMap<String, Anchor>>> = OnceLock::new();
    ANCHORS.get_or_init(|| Mutex::new(HashMap::new()))
}

pub(crate) fn remember_anchor(id: &str, anchor: Anchor) {
    anchors()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(id.to_string(), anchor);
}

pub(crate) fn anchor_of(id: &str) -> Option<Anchor> {
    anchors()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(id)
        .cloned()
}

pub(crate) fn forget_anchor(id: &str) {
    anchors()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .remove(id);
}

pub(crate) fn message_of(row: &Row) -> Message {
    Message::parse(&row.text, Path::is_file)
}

pub(crate) fn is_clear(message: &Message) -> bool {
    message
        .command
        .as_ref()
        .is_some_and(|(name, _)| name == "clear")
}

enum Proof {
    Own,
    Successor,
}

fn clear_proof(files: &dyn JobFiles, agent: &Agent, path: &str, attempt_ms: i64) -> Option<Proof> {
    let born_after = |file: &str| files.born_ms(file).is_some_and(|born| born >= attempt_ms);
    if born_after(path) && begins_with_clear(path) {
        return Some(Proof::Own);
    }
    successor(path, &agent.name)
        .filter(|next| born_after(next))
        .filter(|_| !files.shares_name(agent, path))
        .map(|_| Proof::Successor)
}

pub(crate) fn count_row(
    files: &dyn JobFiles,
    agent: &Agent,
    path: &str,
    row: &Row,
    slack_ms: i64,
) -> Option<Seen> {
    let attempt = row.attempt_ms?;
    let message = message_of(row);
    let mut seen = count_since(path, &message, attempt - slack_ms);
    if is_clear(&message) {
        let now = seen.get_or_insert_with(Seen::default);
        match clear_proof(files, agent, path, attempt) {
            Some(Proof::Own) => now.taken = now.taken.max(1),
            Some(Proof::Successor) => now.taken += 1,
            None => {}
        }
    }
    seen
}

pub(crate) fn count_anchored(
    files: &dyn JobFiles,
    agent: &Agent,
    path: &str,
    row: &Row,
    before: Seen,
    anchor: Option<&Anchor>,
) -> Option<Seen> {
    let attempt = row.attempt_ms?;
    let moved = match anchor {
        Some(Anchor::File(began)) => began != path,
        Some(Anchor::Nowhere) => true,
        None => false,
    };
    if moved {
        if !files.born_ms(path).is_some_and(|born| born >= attempt) {
            return None;
        }
        return count_row(files, agent, path, row, 0).map(|now| Seen {
            taken: now.taken + before.taken,
            enqueued: now.enqueued + before.enqueued,
        });
    }
    let slack = if anchor.is_some() { SLACK_MS } else { 0 };
    count_row(files, agent, path, row, slack)
}
