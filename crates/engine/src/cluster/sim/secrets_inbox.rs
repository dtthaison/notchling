use std::path::Path as FsPath;
use std::sync::Mutex;
use std::time::Duration;

use serde_json::{json, Value};

use super::secrets::{Path as SecretPath, RESYNC_MS};
use super::secrets_scan::ser;
use super::world::World;
use crate::cluster::id::{qualify, NodeId, Qualified};
use crate::cluster::jobs::Job;
use crate::cluster::logins::{probe, Sources, StatusRunner, TIMEOUT};
use crate::cluster::members::MemberState;
use crate::cluster::wire::{Origin, WireError};
use crate::inbox::{Action, ActionId, AnswerError, Change, Item, Kind, Source, State};

fn refusal(why: impl Into<String>) -> WireError {
    WireError::new("forbidden", why)
}

#[derive(Default)]
struct Recorder {
    calls: Mutex<Vec<Vec<String>>>,
}

impl StatusRunner for Recorder {
    fn run(&self, argv: &[String], _timeout: Duration) -> Option<(i32, String)> {
        if let Ok(mut calls) = self.calls.lock() {
            calls.push(argv.to_vec());
        }
        Some((1, "Not logged in".to_string()))
    }
}

impl World {
    pub(super) fn approval_item(&mut self, node: &NodeId, job: &Job, names: &[String]) -> Item {
        let n = self.secrets.next_id();
        Item {
            id: format!("secret:{}:{n}", job.id),
            kind: Kind::SecretApproval,
            title: format!("A job on {} wants a secret", self.name_of(&job.spec.node)),
            detail: names.join(","),
            source: Source {
                node: Some(node.to_string()),
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
        let text = ser(&item);
        self.secrets.record(n, SecretPath::Inbox, &text);
        let changes = self.secrets.books.entry(n.clone()).or_default().raise(item);
        self.mirror_changes(n, &changes);
    }

    fn note_own(&mut self, n: &NodeId, changes: &[Change]) {
        let list = self.secrets.mirrors.entry(n.clone()).or_default();
        for change in changes {
            match change {
                Change::Raised(i) | Change::Updated(i) => {
                    list.insert(i.id.clone(), i.state == State::Open);
                }
                Change::Removed(id) => {
                    list.remove(id);
                }
            }
        }
    }

    pub(super) fn mirror_changes(&mut self, n: &NodeId, changes: &[Change]) {
        self.note_own(n, changes);
        for target in self.mirror_targets(n) {
            if !self
                .secrets
                .synced
                .contains_key(&(n.clone(), target.clone()))
            {
                continue;
            }
            for change in changes {
                let args = match change {
                    Change::Raised(i) | Change::Updated(i) => {
                        json!({"event": "changed", "item": i})
                    }
                    Change::Removed(id) => json!({"event": "removed", "id": id}),
                };
                self.forward_node_op(n, &target, "node.inbox", args);
            }
        }
    }

    pub(super) fn mirror_in(&mut self, node: &NodeId, call: u64, origin: &Origin, args: &Value) {
        self.scan_mirror(node, args);
        let owner = &origin.node;
        let event = args.get("event").and_then(Value::as_str).unwrap_or("");
        let item_of = |v: &Value| serde_json::from_value::<Item>(v.clone()).ok();
        let bad = || Err(WireError::new("bad_request", "not an inbox mirror"));
        let result = if owner == node {
            Err(refusal("only another node sends its inbox items"))
        } else {
            let list = self.secrets.mirrors.entry(node.clone()).or_default();
            match event {
                "changed" => match args.get("item").and_then(item_of) {
                    Some(item) => {
                        list.insert(qualify(owner, &item.id), item.state == State::Open);
                        Ok(json!({"ok": true}))
                    }
                    None => bad(),
                },
                "removed" => match args.get("id").and_then(Value::as_str) {
                    Some(id) => {
                        list.remove(&qualify(owner, id));
                        Ok(json!({"ok": true}))
                    }
                    None => bad(),
                },
                "sync" => match args.get("items").and_then(Value::as_array) {
                    Some(items) => {
                        let prefix = qualify(owner, "");
                        list.retain(|id, _| !id.starts_with(&prefix));
                        for item in items.iter().filter_map(item_of) {
                            list.insert(qualify(owner, &item.id), item.state == State::Open);
                        }
                        Ok(json!({"ok": true}))
                    }
                    None => bad(),
                },
                _ => bad(),
            }
        };
        self.finish_local(node, call, result);
    }

    pub(super) fn mirrors_lost(&mut self, n: &NodeId) {
        self.secrets.mirrors.remove(n);
        self.secrets.synced.retain(|(owner, _), _| owner != n);
        let own: Vec<Change> = self
            .secrets
            .books
            .get(n)
            .map(|b| b.items().into_iter().map(Change::Updated).collect())
            .unwrap_or_default();
        self.note_own(n, &own);
    }

    /// The daemon's outbox: a full sync to a target it has not synced (or
    /// not in the last 60 s), changes only to synced ones, a target that
    /// goes offline dropped from the synced set.
    pub(super) fn secrets_resync(&mut self, owner: &NodeId) {
        let now = self.now;
        let targets: Vec<NodeId> = self
            .mirror_targets(owner)
            .into_iter()
            .filter(|t| self.is_up(t) && self.net.linked(owner, t))
            .collect();
        self.secrets.synced.retain(|(o, t), at| {
            o != owner || (targets.contains(t) && now.saturating_sub(*at) < RESYNC_MS)
        });
        let items: Vec<Item> = self
            .secrets
            .books
            .get(owner)
            .map(|b| {
                b.items()
                    .into_iter()
                    .filter(|i| i.state == State::Open)
                    .collect()
            })
            .unwrap_or_default();
        for target in targets {
            let key = (owner.clone(), target.clone());
            if self.secrets.synced.contains_key(&key) {
                continue;
            }
            self.secrets.synced.insert(key, now);
            let args = json!({"event": "sync", "items": items});
            self.forward_node_op(owner, &target, "node.inbox", args);
            if !items.is_empty() {
                self.secrets.stats.resynced += 1;
            }
        }
    }

    pub(super) fn owner_answer(&mut self, node: &NodeId, call: u64, origin: &Origin, args: &Value) {
        let id = args.get("id").and_then(Value::as_str).unwrap_or("");
        let action = match args.get("action").and_then(Value::as_str) {
            Some("deny") => ActionId::Deny,
            _ => ActionId::AllowOnce,
        };
        let now = self.now as i64;
        let key = qualify(node, id);
        let book = self.secrets.books.entry(node.clone()).or_default();
        match book.answer(id, action, origin.node.as_str(), now) {
            Ok(change) => {
                *self.secrets.answered.entry(key).or_default() += 1;
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
                *self.secrets.answered.entry(key).or_default() += 1;
                self.finish_local(node, call, Ok(json!({})));
            }
            Err(_) => {
                let err = WireError::new("failed", "that item isn't open");
                self.finish_local(node, call, Err(err));
            }
        }
    }

    pub fn answer_item(&mut self, from: &NodeId, item: &str, allow: bool) {
        let (owner, plain) = match Qualified::parse(item) {
            Some(q) => (q.node, q.id),
            None => (from.clone(), item.to_string()),
        };
        let action = if allow { "allow_once" } else { "deny" };
        let cmd = crate::cluster::core::LocalCmd::Forward {
            node: owner,
            op: "inbox.answer".to_string(),
            args: json!({"id": plain, "action": action}).into(),
            origin: Origin {
                node: from.clone(),
                tier: crate::cluster::wire::OriginTier::App,
            },
        };
        self.local(from, cmd);
    }

    pub fn login_failure(&mut self, node: &NodeId) {
        let name = self.name_of(node);
        let os = if self.is_client(node) {
            "macos"
        } else {
            "linux"
        };
        let runner = Recorder::default();
        let sources = Sources {
            codex: Some("codex"),
            agy_installed: true,
            os,
            home: FsPath::new("/home/sim"),
        };
        let _ = probe(&sources, &runner, &|_| false);
        if self.secrets.plant.refresh {
            let argv: Vec<String> = ["claude", "auth", "refresh"]
                .iter()
                .map(|s| s.to_string())
                .collect();
            let _ = runner.run(&argv, TIMEOUT);
        }
        let calls = runner.calls.lock().map(|c| c.clone()).unwrap_or_default();
        for argv in calls {
            self.secrets.login_runs.push((node.clone(), argv));
        }
        let detail = format!("Run claude on {name} and sign in with /login");
        let item = self.login_item(node, &detail);
        self.raise_item(node, item);
    }

    pub(super) fn login_item(&self, node: &NodeId, detail: &str) -> Item {
        let name = self.name_of(node);
        Item {
            id: format!("login-needed:{name}:claude"),
            kind: Kind::LoginNeeded,
            title: format!("Claude Code needs a sign-in on {name}"),
            detail: detail.to_string(),
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
        }
    }
}
