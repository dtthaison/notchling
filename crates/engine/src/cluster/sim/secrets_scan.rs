use serde::Serialize;
use serde_json::{json, Value};

use super::secrets::{LeakAt, Path};
use super::world::World;
use crate::cluster::core::Effect;
use crate::cluster::id::NodeId;
use crate::cluster::members::Term;
use crate::cluster::wire::{Msg, Origin, OriginTier, WireError};

pub(super) fn ser<T: Serialize>(value: &T) -> String {
    match serde_json::to_string(value) {
        Ok(text) => text,
        Err(_) => panic!("sim: a value the recorder must read can't be serialised"),
    }
}

fn job_of(args: &Value) -> Option<&str> {
    args.get("job").and_then(Value::as_str)
}

impl World {
    pub(super) fn scan_wire(&mut self, from: &NodeId, to: &NodeId, msg: &Msg) {
        let text = ser(msg);
        let (op, job) = match msg {
            Msg::Call { op, args, .. } => (op.as_str(), job_of(args)),
            _ => ("msg", None),
        };
        self.secrets.record(from, Path::Wire { op, to, job }, &text);
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
            Effect::RunLocal { op, args, .. } => {
                let path = Path::Run {
                    op,
                    job: job_of(args),
                };
                self.secrets.record(n, path, &args.to_string());
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

    pub(super) fn plant_leak(&mut self, n: &NodeId, at: LeakAt, marker: &str) {
        let args = json!({"job": "j-leak", "note": marker});
        match at {
            LeakAt::Log => self.push_trace(n, &format!("released {marker}")),
            LeakAt::Journal => self.scan_persisted(n, &format!("{{\"jobs\":\"{marker}\"}}")),
            LeakAt::Publish => {
                let effect = Effect::Publish {
                    topic: "sessions".to_string(),
                    name: "session.changed".to_string(),
                    data: args,
                };
                self.scan_effect(n, &effect);
            }
            LeakAt::Mirror => self.scan_mirror(n, &args),
            LeakAt::Argv => self.secrets_started(n, &json!({"job": "j-leak", "prompt": marker})),
            LeakAt::Run => {
                let effect = Effect::RunLocal {
                    call: 0,
                    op: "shell.run".to_string(),
                    args: args.into(),
                    origin: Origin {
                        node: n.clone(),
                        tier: OriginTier::Node,
                    },
                    term: self.state_of(n).map(|s| s.term).unwrap_or(Term {
                        n: 0,
                        node: n.clone(),
                    }),
                };
                self.scan_effect(n, &effect);
            }
            LeakAt::Answer => {
                let effect = Effect::Answer {
                    local: 0,
                    result: Err(WireError::new("failed", marker)),
                };
                self.scan_effect(n, &effect);
            }
        }
    }
}
