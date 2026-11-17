use crate::outbox_courier::JobFiles;
use notchling_engine::outbox::Row;
use notchling_engine::sessions::Agent;
use notchling_engine::transcript::locate::{begins_with_clear, successor};
use notchling_engine::transcript::message::Message;
use notchling_engine::transcript::sent::{count_since, Seen};
use std::path::Path;

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
    message: &Message,
    slack_ms: i64,
) -> Option<Seen> {
    let attempt = row.attempt_ms?;
    let mut seen = count_since(path, message, attempt - slack_ms);
    if is_clear(message) {
        let now = seen.get_or_insert_with(Seen::default);
        match clear_proof(files, agent, path, attempt) {
            Some(Proof::Own) => now.taken = now.taken.max(1),
            Some(Proof::Successor) if now.taken == 0 => now.taken = 1,
            Some(Proof::Successor) => {}
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
    message: &Message,
    before: Seen,
) -> Option<Seen> {
    let attempt = row.attempt_ms?;
    let tied = row
        .anchor
        .as_ref()
        .filter(|anchor| anchor.file.as_deref() == Some(path));
    if let Some(anchor) = tied {
        return count_row(files, agent, path, row, message, anchor.slack_ms);
    }
    if !files.born_ms(path).is_some_and(|born| born >= attempt) {
        return None;
    }
    let now = count_row(files, agent, path, row, message, 0)?;
    Some(match row.anchor {
        Some(_) => Seen {
            taken: now.taken + before.taken,
            enqueued: now.enqueued + before.enqueued,
        },
        None => now,
    })
}
