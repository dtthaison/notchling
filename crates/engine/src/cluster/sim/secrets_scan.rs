use serde::Serialize;
use serde_json::{json, Value};

use super::host::START_OPS;
use super::secrets::{LeakAt, Path};
use super::secrets_broker::BrokerEv;
use super::world::{Event, World};
use crate::cluster::core::Effect;
use crate::cluster::id::NodeId;
use crate::cluster::jobs::JobSpec;
use crate::cluster::members::Term;
use crate::cluster::state::{Command, JournalEntry};
use crate::cluster::wire::{Msg, Origin, OriginTier, WireError};

pub(super) fn ser<T: Serialize>(value: &T) -> String {
    match serde_json::to_string(value) {
        Ok(text) => text,
        Err(_) => panic!("sim: a value the recorder must read can't be serialised"),
    }
}

impl World {
    pub(super) fn scan_wire(&mut self, from: &NodeId, to: &NodeId, msg: &Msg) {
        let text = ser(msg);
        let op = match msg {
            Msg::Call { op, .. } => op.as_str(),
            _ => "msg",
        };
        self.secrets.record(from, Path::Wire { op, to }, &text);
    }

    pub(super) fn scan_mirror(&mut self, n: &NodeId, body: &Value) {
        self.secrets.record(n, Path::Mirror, &body.to_string());
    }

    pub(super) fn scan_persisted(&mut self, n: &NodeId, text: &str) {
        self.secrets.record(n, Path::Journal, text);
    }

    pub(super) fn scan_effect(&mut self, n: &NodeId, effect: &Effect) {
        match effect {
            Effect::Persist { entry, changes } => {
                let text = format!("{}{}", ser(entry), ser(changes));
                self.scan_persisted(n, &text);
            }
            Effect::Publish { topic, name, data } => {
                let text = format!("{topic} {name} {data}");
                self.secrets.record(n, Path::Publish, &text);
            }
            Effect::RunLocal { op, args, .. } if !START_OPS.contains(&op.as_str()) => {
                self.secrets.record(n, Path::Run { op }, &args.to_string());
            }
            Effect::Answer { result, .. } => {
                let text = match result {
                    Ok(v) => v.to_string(),
                    Err(e) => format!("{} {}", e.code, e.message),
                };
                self.secrets.record(n, Path::Answer, &text);
            }
            _ => {}
        }
    }

    pub(super) fn push_trace(&mut self, n: &NodeId, text: &str) {
        self.secrets.record(n, Path::Log, text);
        let name = self.name_of(n);
        self.trace
            .push(format!("{:>8} {:<4} {}", self.now, name, text));
    }

    /// Plants go in through the same doors honest data uses: an effect the
    /// node "emits" on its next step, a message to its inbox handler, a
    /// trace line, a Book item.
    pub(super) fn plant_leak(&mut self, n: &NodeId, at: LeakAt, marker: &str) {
        let args = json!({"job": "j-leak", "note": marker});
        let term = self.state_of(n).map(|s| s.term).unwrap_or(Term {
            n: 0,
            node: n.clone(),
        });
        let effect = match at {
            LeakAt::Log => {
                self.push_trace(n, &format!("released {marker}"));
                return;
            }
            LeakAt::Mirror => {
                let ev = BrokerEv::Run {
                    node: n.clone(),
                    call: 0,
                    op: "node.inbox".to_string(),
                    args,
                    origin: Origin {
                        node: n.clone(),
                        tier: OriginTier::Node,
                    },
                    arrived: (self.now, self.secrets.revoke_seq),
                };
                let when = self.now + 1;
                self.schedule(when, Event::Broker(ev));
                return;
            }
            LeakAt::Inbox => {
                let item = self.login_item(n, marker);
                self.raise_item(n, item);
                return;
            }
            LeakAt::Journal => {
                let spec = JobSpec {
                    node: n.clone(),
                    tool: crate::cluster::id::Tool::Shell,
                    cwd: "/srv/repo".to_string(),
                    args,
                    secrets: Vec::new(),
                };
                Effect::Persist {
                    entry: JournalEntry {
                        seq: 0,
                        at_ms: self.now as i64,
                        term,
                        command: Command::Dispatch {
                            key: "leak".to_string(),
                            spec,
                        },
                    },
                    changes: Vec::new(),
                }
            }
            LeakAt::Publish => Effect::Publish {
                topic: "sessions".to_string(),
                name: "session.changed".to_string(),
                data: args,
            },
            LeakAt::Argv => Effect::RunLocal {
                call: 0,
                op: "shell.run".to_string(),
                args: json!({"job": "j-leak", "prompt": marker}).into(),
                origin: Origin {
                    node: n.clone(),
                    tier: OriginTier::Node,
                },
                term,
            },
            LeakAt::Run => Effect::RunLocal {
                call: 0,
                op: "session.reply".to_string(),
                args: args.into(),
                origin: Origin {
                    node: n.clone(),
                    tier: OriginTier::Node,
                },
                term,
            },
            LeakAt::Answer => Effect::Answer {
                local: 0,
                result: Err(WireError::new("failed", marker)),
            },
        };
        self.secrets.planted.push((n.clone(), effect));
    }

    pub(super) fn take_planted(&mut self, n: &NodeId) -> Vec<Effect> {
        let (mine, rest): (Vec<_>, Vec<_>) = std::mem::take(&mut self.secrets.planted)
            .into_iter()
            .partition(|(node, _)| node == n);
        self.secrets.planted = rest;
        mine.into_iter().map(|(_, e)| e).collect()
    }
}
