//! What changed between two loads of a conversation, by item (decision 0018, point 2). The loader
//! computes it off the main thread; the main thread only checks that the change starts from the
//! conversation on show.

use std::ops::Range;
use std::sync::Arc;

use notchling_engine::transcript::model::Conversation;

/// Items `old` in the previous conversation became items `new` in this one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Splice {
    pub old: Range<usize>,
    pub new: Range<usize>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Change {
    /// Same `Arc`, or equal: nothing the column shows changed.
    Same,
    /// Splices in old-item order, none overlapping; `other` says a field besides `items` changed
    /// (tasks, facts, queued, activity, turn start or tokens, finished agents, shells, the pending
    /// question).
    Items { splices: Vec<Splice>, other: bool },
    /// The ids don't line up (another session, a fresh cursor renumbered them): start over.
    Reset,
}

/// More changed items than this in one load and the change is not worth describing.
pub const RESET_ABOVE: usize = 64;

/// What `new` changed from `old`. Items are matched by id: the engine numbers them by line, keeps the
/// newest few hundred, and so only ever trims the front, grows the end or edits an item in place (a
/// tools group gaining a step); anything else resets.
pub fn diff(old: &Arc<Conversation>, new: &Arc<Conversation>) -> Change {
    if Arc::ptr_eq(old, new) {
        return Change::Same;
    }
    let other = other_changed(old, new);
    let (a, b) = (&old.items, &new.items);
    let mut splices: Vec<Splice> = Vec::new();
    match (a.is_empty(), b.is_empty()) {
        (true, true) => return finish(splices, other),
        (true, false) | (false, true) => return Change::Reset,
        (false, false) => {}
    }
    let Some(k) = a.iter().position(|item| item.id == b[0].id) else {
        return Change::Reset;
    };
    if k > 0 {
        splices.push(Splice {
            old: 0..k,
            new: 0..0,
        });
    }
    let m = (a.len() - k).min(b.len());
    let mut changed = 0;
    // Where the last changed pair ended, so a neighbour joins it.
    let mut open: Option<usize> = None;
    for i in 0..m {
        let (x, y) = (&a[k + i], &b[i]);
        if x.id != y.id {
            return Change::Reset;
        }
        if x == y {
            continue;
        }
        changed += 1;
        if changed > RESET_ABOVE {
            return Change::Reset;
        }
        match (open, splices.last_mut()) {
            (Some(end), Some(last)) if end == i => {
                last.old.end = k + i + 1;
                last.new.end = i + 1;
            }
            _ => splices.push(Splice {
                old: k + i..k + i + 1,
                new: i..i + 1,
            }),
        }
        open = Some(i + 1);
    }
    if a.len() > k + m {
        splices.push(Splice {
            old: k + m..a.len(),
            new: m..m,
        });
    }
    if b.len() > m {
        splices.push(Splice {
            old: a.len()..a.len(),
            new: m..b.len(),
        });
    }
    finish(splices, other)
}

fn finish(splices: Vec<Splice>, other: bool) -> Change {
    if splices.is_empty() && !other {
        Change::Same
    } else {
        Change::Items { splices, other }
    }
}

/// A field besides the items differs.
fn other_changed(old: &Conversation, new: &Conversation) -> bool {
    old.tasks != new.tasks
        || old.facts != new.facts
        || old.activity != new.activity
        || old.turn_start != new.turn_start
        || old.turn_tokens != new.turn_tokens
        || old.queued != new.queued
        || old.finished_agents != new.finished_agents
        || old.shells != new.shells
        || old.pending_question != new.pending_question
}
