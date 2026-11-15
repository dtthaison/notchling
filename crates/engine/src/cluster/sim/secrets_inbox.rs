use serde_json::{json, Value};

use super::secrets::Path;
use super::world::World;
use crate::cluster::id::NodeId;
use crate::cluster::jobs::Job;
use crate::cluster::members::MemberState;
use crate::cluster::wire::{Origin, WireError};
use crate::inbox::{Action, ActionId, AnswerError, Change, Item, Kind, Source, State};

fn refusal(why: impl Into<String>) -> WireError {
    WireError::new("forbidden", why)
}

impl World {
    pub(super) fn approval_item(&mut self, node: &NodeId, job: &Job, names: &[String]) -> Item {
        let n = self.secrets.next_id();
        Item {
            id: format!("secret:{}:{n}", job.id),
            kind: Kind::SecretApproval,
            title: format!(
                "A job on {} wants {}",
                self.name_of(&job.spec.node),
                names.join(", ")
            ),
            detail: format!("asked on {}", self.name_of(node)),
            source: Source {
                node: Some(job.spec.node.to_string()),
                session: None,
                job: Some(job.id.clone()),
            },
            created_ms: self.now as i64,
            expires_ms: None,
            actions: [ActionId::AllowOnce, ActionId::AllowAlways, ActionId::Deny]
                .into_iter()
                .map(|id| Action {
                    id,
                    label: format!("{id:?}"),
                    primary: false,
                })
                .collect(),
            state: State::Open,
            answer: None,
            answered_by: None,
            seen: false,
            open: None,
            closed_ms: None,
        }
    }

    pub(super) fn mirror_targets(&self, n: &NodeId) -> Vec<NodeId> {
        let Some(state) = self.state_of(n) else {
            return Vec::new();
        };
        state
            .members
            .values()
            .filter(|m| m.state == MemberState::Active && m.id != *n)
            .filter(|m| m.id == state.home || (m.client && m.can_be_master))
            .map(|m| m.id.clone())
            .collect()
    }

    pub(super) fn raise_item(&mut self, n: &NodeId, item: Item) {
        let changes = self.secrets.books.entry(n.clone()).or_default().raise(item);
        self.mirror_changes(n, &changes);
    }

    pub(super) fn note_mirror(&mut self, at: &NodeId, owner: &NodeId, changes: &[Change]) {
        let list = self.secrets.mirrors.entry(at.clone()).or_default();
        for change in changes {
            match change {
                Change::Raised(i) | Change::Updated(i) => {
                    list.insert(i.id.clone(), (owner.clone(), i.state == State::Open));
                }
                Change::Removed(id) => {
                    list.remove(id);
                }
            }
        }
    }

    pub(super) fn mirror_changes(&mut self, n: &NodeId, changes: &[Change]) {
        self.note_mirror(n, n, changes);
        for target in self.mirror_targets(n) {
            let args = json!({"owner": n, "changes": changes});
            self.forward_node_op(n, &target, "node.inbox", args);
        }
    }

    pub(super) fn mirror_in(&mut self, node: &NodeId, call: u64, args: &Value) {
        let owner = args
            .get("owner")
            .and_then(|o| serde_json::from_value::<NodeId>(o.clone()).ok());
        let changes = args
            .get("changes")
            .and_then(|c| serde_json::from_value::<Vec<Change>>(c.clone()).ok());
        let text = args.to_string();
        self.secrets.record(node, Path::Mirror, &text);
        let result = match (owner, changes) {
            (Some(owner), Some(changes)) => {
                self.note_mirror(node, &owner, &changes);
                Ok(json!({}))
            }
            _ => Err(WireError::new("bad_request", "not an inbox mirror")),
        };
        self.finish_local(node, call, result);
    }

    pub(super) fn owner_answer(&mut self, node: &NodeId, call: u64, origin: &Origin, args: &Value) {
        let id = args.get("id").and_then(Value::as_str).unwrap_or("");
        let action = match args.get("action").and_then(Value::as_str) {
            Some("deny") => ActionId::Deny,
            _ => ActionId::AllowOnce,
        };
        let now = self.now as i64;
        let book = self.secrets.books.entry(node.clone()).or_default();
        match book.answer(id, action, origin.node.as_str(), now) {
            Ok(change) => {
                *self.secrets.answered.entry(id.to_string()).or_default() += 1;
                self.secrets.stats.answered += 1;
                self.mirror_changes(node, &[change]);
                self.finish_local(node, call, Ok(json!({})));
                if let Some(pid) = self.secrets.approvals.remove(id) {
                    if action == ActionId::Deny {
                        if let Some(p) = self.secrets.pending.remove(&pid) {
                            let err = refusal("denied");
                            self.finish_local(&p.node, p.call, Err(err));
                        }
                    } else {
                        self.release(pid);
                    }
                }
            }
            Err(AnswerError::NotOpen(_)) if self.secrets.plant.answer_twice => {
                *self.secrets.answered.entry(id.to_string()).or_default() += 1;
                self.finish_local(node, call, Ok(json!({})));
            }
            Err(_) => {
                let err = WireError::new("failed", "that item isn't open");
                self.finish_local(node, call, Err(err));
            }
        }
    }

    pub fn login_failure(&mut self, node: &NodeId) {
        if self.secrets.plant.refresh {
            self.secrets.refreshes.push(node.clone());
            return;
        }
        let name = self.name_of(node);
        let item = Item {
            id: format!("login-needed:{name}:claude"),
            kind: Kind::LoginNeeded,
            title: format!("Claude Code needs a sign-in on {name}"),
            detail: format!("Run claude on {name} and sign in with /login"),
            source: Source {
                node: Some(node.to_string()),
                session: None,
                job: None,
            },
            created_ms: self.now as i64,
            expires_ms: None,
            actions: vec![Action {
                id: ActionId::Done,
                label: "Done".to_string(),
                primary: false,
            }],
            state: State::Open,
            answer: None,
            answered_by: None,
            seen: false,
            open: None,
            closed_ms: None,
        };
        self.raise_item(node, item);
    }
}
