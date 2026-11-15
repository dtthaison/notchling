use crate::cluster_prompts_clean::{clean_pending, Finder};
use crate::prompts::row_bytes;
use notchling_engine::cluster::NodeId;
use notchling_local_api::topics;
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet, VecDeque};

pub const MAX_PER_NODE: usize = 50;
pub const MAX_TOTAL: usize = 200;
pub const GONE_MAX: usize = MAX_PER_NODE * 4;
pub const REMOTE_BYTES: usize = 384 * 1024;

#[derive(Default)]
pub(crate) struct Followed {
    pub(crate) next_at: u64,
    pub(crate) ok_at: Option<u64>,
    pub(crate) listing: bool,
    pub(crate) gone: BTreeSet<String>,
}

impl Followed {
    pub(crate) fn following(&self) -> bool {
        self.ok_at.is_some()
    }

    fn remember(&mut self, id: &str) {
        if self.listing && self.gone.len() < GONE_MAX {
            self.gone.insert(id.to_string());
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct Ticket {
    gen: u64,
    stop: u64,
}

#[derive(Default)]
pub(crate) struct Book {
    pub(crate) followed: BTreeMap<NodeId, Followed>,
    pub(crate) prompts: BTreeMap<String, (NodeId, Value)>,
    pub(crate) busy: BTreeSet<NodeId>,
    gens: BTreeMap<NodeId, u64>,
    stops: u64,
    bytes: usize,
    outbox: VecDeque<(&'static str, Value)>,
    draining: bool,
}

fn resolved_for(shaped: &Value, by: &str) -> Value {
    let mut data = json!({
        "id": shaped["id"], "sessionId": shaped["sessionId"],
        "outcome": "pass", "by": by, "node": shaped["node"],
    });
    if let Some(row) = shaped.get("rowId") {
        data["rowId"] = row.clone();
    }
    data
}

impl Book {
    fn count(&self, node: &NodeId) -> usize {
        self.prompts.values().filter(|(n, _)| n == node).count()
    }

    pub(crate) fn ids_of(&self, node: &NodeId) -> BTreeSet<String> {
        self.prompts
            .iter()
            .filter(|(_, (n, _))| n == node)
            .map(|(id, _)| id.clone())
            .collect()
    }

    pub(crate) fn tracked(&self) -> BTreeSet<NodeId> {
        self.followed
            .keys()
            .cloned()
            .chain(self.prompts.values().map(|(n, _)| n.clone()))
            .chain(self.busy.iter().cloned())
            .collect()
    }

    pub(crate) fn ticket(&self, node: &NodeId) -> Ticket {
        Ticket {
            gen: self.gens.get(node).copied().unwrap_or(0),
            stop: self.stops,
        }
    }

    pub(crate) fn current(&self, node: &NodeId, ticket: Ticket) -> bool {
        self.ticket(node) == ticket
    }

    fn say(&mut self, event: &'static str, data: Value) {
        self.outbox.push_back((event, data));
    }

    pub(crate) fn claim(&mut self) -> bool {
        if self.draining || self.outbox.is_empty() {
            return false;
        }
        self.draining = true;
        true
    }

    pub(crate) fn next_event(&mut self) -> Option<(&'static str, Value)> {
        let next = self.outbox.front().cloned();
        if next.is_none() {
            self.draining = false;
        }
        next
    }

    pub(crate) fn published(&mut self) {
        self.outbox.pop_front();
    }

    pub(crate) fn held_bytes(&self) -> usize {
        self.bytes
    }

    fn take(&mut self, id: &str) -> Option<(NodeId, Value)> {
        let held = self.prompts.remove(id)?;
        self.bytes = self.bytes.saturating_sub(row_bytes(&held.1));
        Some(held)
    }

    pub(crate) fn release(&mut self) {
        self.draining = false;
    }

    pub(crate) fn put(&mut self, node: &NodeId, shaped: Value) {
        let Some(id) = shaped.get("id").and_then(Value::as_str).map(String::from) else {
            return;
        };
        let size = row_bytes(&shaped);
        let freed = match self.prompts.get(&id) {
            Some((_, old)) if *old == shaped => return,
            Some((_, old)) => row_bytes(old),
            None if self.count(node) >= MAX_PER_NODE || self.prompts.len() >= MAX_TOTAL => return,
            None => 0,
        };
        if self.bytes - freed + size > REMOTE_BYTES {
            return;
        }
        self.bytes = self.bytes - freed + size;
        self.prompts.insert(id, (node.clone(), shaped.clone()));
        self.say(topics::PROMPT_PENDING, shaped);
    }

    pub(crate) fn pending(&mut self, from: &NodeId, shaped: Value) {
        let Some(f) = self.followed.get_mut(from).filter(|f| f.following()) else {
            return;
        };
        if let Some(id) = shaped.get("id").and_then(Value::as_str) {
            f.gone.remove(id);
        }
        self.put(from, shaped);
    }

    pub(crate) fn resolved(&mut self, from: &NodeId, id: &str, mut shaped: Value) {
        let Some(f) = self.followed.get_mut(from).filter(|f| f.following()) else {
            return;
        };
        f.remember(id);
        let held = matches!(self.prompts.get(id), Some((n, _)) if n == from);
        if !held {
            return;
        }
        if let Some((_, prompt)) = self.take(id) {
            if let Some(row) = prompt.get("rowId") {
                shaped["rowId"] = row.clone();
            }
            self.say(topics::PROMPT_RESOLVED, shaped);
        }
    }

    pub(crate) fn drop_node(&mut self, node: &NodeId, why: &str) {
        *self.gens.entry(node.clone()).or_insert(0) += 1;
        self.followed.remove(node);
        for id in self.ids_of(node) {
            if let Some((_, prompt)) = self.take(&id) {
                self.say(topics::PROMPT_RESOLVED, resolved_for(&prompt, why));
            }
        }
    }

    pub(crate) fn stop(&mut self, why: &str) {
        self.stops += 1;
        for node in self.tracked() {
            self.drop_node(&node, why);
        }
    }

    pub(crate) fn fill(
        &mut self,
        node: &NodeId,
        listed: &Value,
        before: &BTreeSet<String>,
        find: Finder,
    ) {
        let Value::Array(rows) = listed else {
            return;
        };
        let mut seen = BTreeSet::new();
        for row in rows.iter().filter(|r| r.get("tool").is_none()) {
            let Some(shaped) = clean_pending(node, row, find) else {
                continue;
            };
            let id = shaped
                .get("id")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            let skip = self
                .followed
                .get(node)
                .is_none_or(|f| f.gone.contains(&id) || !f.following());
            seen.insert(id);
            if !skip {
                self.put(node, shaped);
            }
        }
        if crate::prompts::may_be_cut(rows) {
            return;
        }
        for id in before.difference(&seen) {
            if let Some((_, prompt)) = self.take(id) {
                self.say(topics::PROMPT_RESOLVED, resolved_for(&prompt, "unlisted"));
            }
        }
    }
}
