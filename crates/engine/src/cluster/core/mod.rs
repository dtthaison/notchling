mod control;
mod dispatch;
mod forward;
mod join;
mod link;

use std::collections::{BTreeMap, BTreeSet};

use serde_json::{json, Value};

use super::id::{NodeId, Tool};
use super::jobs::JobSpec;
use super::join::{JoinToken, Tokens};
use super::members::{Desired, Member, MemberState, Term};
use super::state::{role_of, Applied, Change, ClusterState, Command, CommandError, JournalEntry};
use super::versions::Versions;
use super::wire::{Msg, Origin, OriginTier, WireError};

pub const PING_MS: u64 = 5_000;
pub const OFFLINE_MS: u64 = 20_000;
pub const CALL_MS: u64 = 30_000;
pub const JOIN_ANSWER_MS: u64 = 15_000;
pub const DIAL_BACKOFF_MS: (u64, u64) = (1_000, 60_000);
pub const DEDUPE_MS: u64 = 300_000;

#[derive(Clone, Debug)]
pub enum Input {
    Tick {
        boot_ms: u64,
        wall_ms: i64,
    },
    Linked {
        peer: NodeId,
    },
    Unlinked {
        peer: NodeId,
    },
    Msg {
        from: NodeId,
        msg: Msg,
    },
    Local {
        id: u64,
        cmd: LocalCmd,
    },
    LocalDone {
        call: u64,
        result: Result<Value, WireError>,
    },
}

#[derive(Clone, Debug)]
pub enum LocalCmd {
    Create {
        id: String,
        name: String,
        me: Member,
    },
    IssueToken {
        secret: String,
    },
    Approve {
        node: NodeId,
        client: bool,
        can_be_master: bool,
    },
    Deny {
        node: NodeId,
    },
    Remove {
        node: NodeId,
    },
    SetMember {
        node: NodeId,
        can_be_master: Option<bool>,
        client: Option<bool>,
        reference: Option<bool>,
        accepts_delegation: Option<bool>,
        desired: Option<Desired>,
    },
    SetVersions {
        versions: Versions,
    },
    Dispatch {
        key: String,
        spec: JobSpec,
        tier: OriginTier,
    },
    Forward {
        node: NodeId,
        op: String,
        args: Value,
        origin: Origin,
    },
    JoinWith {
        token: JoinToken,
        me: Member,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub enum Effect {
    Send {
        to: NodeId,
        msg: Msg,
    },
    Dial {
        to: NodeId,
        addrs: Vec<String>,
        port: u16,
        fingerprint: String,
    },
    Close {
        peer: NodeId,
    },
    Pins {
        fingerprints: Vec<String>,
        join_open: bool,
    },
    Persist {
        entry: JournalEntry,
        changes: Vec<Change>,
    },
    Publish {
        topic: String,
        name: String,
        data: Value,
    },
    RunLocal {
        call: u64,
        op: String,
        args: Value,
        origin: Origin,
        term: Term,
    },
    Answer {
        local: u64,
        result: Result<Value, WireError>,
    },
    JoinRequest {
        item: String,
        node: NodeId,
        name: String,
        os: String,
        arch: String,
        addr: Option<String>,
        can_be_master: bool,
    },
    SteppedDown {
        term: Term,
        home: NodeId,
    },
    Wipe,
}

#[derive(Clone, Debug, Default)]
struct Peer {
    linked: bool,
    greeted: bool,
    online: bool,
    last_heard: u64,
    last_ping: u64,
    last_hello: u64,
}

#[derive(Clone, Debug)]
struct Dial {
    next_at: u64,
    backoff: u64,
}

#[derive(Clone, Debug, PartialEq)]
enum Target {
    Local(u64),
    Peer(NodeId, u64),
}

#[derive(Clone, Debug)]
enum Then {
    Reply(Target),
    Dispatch {
        job: String,
        warning: Option<String>,
        target: Target,
    },
}

#[derive(Clone, Debug)]
struct OutCall {
    to: NodeId,
    deadline: u64,
    then: Then,
}

#[derive(Clone, Debug)]
struct Seen {
    at: u64,
    result: Option<Result<Value, WireError>>,
}

#[derive(Clone, Debug)]
struct Joining {
    token: JoinToken,
    me: Member,
    local: u64,
    answered: bool,
    since: u64,
}

pub struct NodeCore {
    me: NodeId,
    versions: Versions,
    client: bool,
    boot_id: String,
    state: Option<ClusterState>,
    tokens: Tokens,
    boot_ms: u64,
    wall_ms: i64,
    peers: BTreeMap<NodeId, Peer>,
    dials: BTreeMap<NodeId, Dial>,
    out_calls: BTreeMap<u64, OutCall>,
    local_runs: BTreeMap<u64, Then>,
    seen: BTreeMap<(NodeId, u64), Seen>,
    joining: Option<Joining>,
    next_wire: u64,
    next_local: u64,
    last_pins: Option<(Vec<String>, bool)>,
    changed: bool,
    #[cfg(test)]
    pub(crate) skip_term_check: bool,
}

impl NodeCore {
    pub fn new(
        me: NodeId,
        versions: Versions,
        state: Option<ClusterState>,
        client: bool,
    ) -> NodeCore {
        NodeCore {
            me,
            versions,
            client,
            boot_id: String::new(),
            state,
            tokens: Tokens::default(),
            boot_ms: 0,
            wall_ms: 0,
            peers: BTreeMap::new(),
            dials: BTreeMap::new(),
            out_calls: BTreeMap::new(),
            local_runs: BTreeMap::new(),
            seen: BTreeMap::new(),
            joining: None,
            next_wire: 1,
            next_local: 1,
            last_pins: None,
            changed: false,
            #[cfg(test)]
            skip_term_check: false,
        }
    }

    pub fn with_boot_id(mut self, boot_id: String) -> NodeCore {
        self.boot_id = boot_id;
        self
    }

    pub fn me(&self) -> &NodeId {
        &self.me
    }

    pub fn state(&self) -> Option<&ClusterState> {
        self.state.as_ref()
    }

    pub fn is_home(&self) -> bool {
        self.state.as_ref().is_some_and(|s| s.home == self.me)
    }

    pub fn online(&self) -> BTreeSet<NodeId> {
        self.peers
            .iter()
            .filter(|(_, p)| p.linked && p.greeted && p.online)
            .map(|(id, _)| id.clone())
            .collect()
    }

    pub fn highest_term(&self) -> Option<&Term> {
        self.state.as_ref().map(|s| &s.term)
    }

    pub fn status(&self) -> Value {
        let me = self.me.to_string();
        let Some(state) = &self.state else {
            return json!({"cluster": Value::Null, "me": me, "members": []});
        };
        let online = self.online();
        let members: Vec<Value> = state
            .members
            .values()
            .map(|m| {
                let role = match role_of(state, &m.id) {
                    super::id::Role::Home => "home",
                    super::id::Role::Standby => "standby",
                    super::id::Role::Worker => "worker",
                };
                let member_state = match m.state {
                    MemberState::Pending => "pending",
                    MemberState::Active => "active",
                    MemberState::Removed => "removed",
                };
                json!({
                    "id": m.id,
                    "name": m.name,
                    "role": role,
                    "canBeMaster": m.can_be_master,
                    "online": m.id == self.me || online.contains(&m.id),
                    "client": m.client,
                    "reference": m.reference,
                    "state": member_state,
                    "versions": m.versions,
                    "desired": m.desired,
                    "acceptsDelegation": m.accepts_delegation,
                })
            })
            .collect();
        json!({
            "cluster": {
                "id": state.id,
                "name": state.name,
                "term": state.term,
                "home": state.home,
                "vault": state.vault,
            },
            "me": me,
            "members": members,
        })
    }

    pub fn step(&mut self, input: Input) -> Vec<Effect> {
        let mut out = Vec::new();
        match input {
            Input::Tick { boot_ms, wall_ms } => {
                self.boot_ms = boot_ms;
                self.wall_ms = wall_ms;
                self.tick(&mut out);
            }
            Input::Linked { peer } => self.on_linked(peer, &mut out),
            Input::Unlinked { peer } => self.on_unlinked(peer, &mut out),
            Input::Msg { from, msg } => self.on_msg(from, msg, &mut out),
            Input::Local { id, cmd } => self.on_local(id, cmd, &mut out),
            Input::LocalDone { call, result } => self.local_done(call, result, &mut out),
        }
        self.flush(&mut out);
        out
    }

    #[cfg(test)]
    pub(crate) fn bump_term(&mut self) -> Vec<Effect> {
        let mut out = Vec::new();
        if let Some(state) = &self.state {
            let term = Term {
                n: state.term.n + 1,
                node: self.me.clone(),
            };
            let _ = self.apply(Command::Term { term }, &mut out);
            self.push_members(None, &mut out);
        }
        self.flush(&mut out);
        out
    }

    fn apply(
        &mut self,
        command: Command,
        out: &mut Vec<Effect>,
    ) -> Result<Vec<Change>, CommandError> {
        let now = self.wall_ms;
        let applied = match &mut self.state {
            Some(state) => super::state::apply(state, command, now)?,
            None => {
                let (state, entry, changes) = super::state::start(command, now)?;
                self.state = Some(state);
                Applied {
                    entry: Some(entry),
                    changes,
                }
            }
        };
        if let Some(entry) = applied.entry {
            out.push(Effect::Persist {
                entry,
                changes: applied.changes.clone(),
            });
        }
        self.changed = true;
        Ok(applied.changes)
    }

    fn flush(&mut self, out: &mut Vec<Effect>) {
        let (fingerprints, join_open) = self.pin_set();
        let now = (fingerprints, join_open);
        if self.last_pins.as_ref() != Some(&now) {
            out.push(Effect::Pins {
                fingerprints: now.0.clone(),
                join_open: now.1,
            });
            self.last_pins = Some(now);
        }
        if self.changed {
            self.changed = false;
            out.push(self.publish("cluster.changed", self.status()));
        }
    }

    fn pin_set(&self) -> (Vec<String>, bool) {
        let mut fps: BTreeSet<String> = BTreeSet::new();
        if let Some(state) = &self.state {
            for m in state.members.values() {
                if m.id != self.me && m.state != MemberState::Removed {
                    fps.insert(m.fingerprint.clone());
                }
            }
        }
        if let Some(j) = &self.joining {
            fps.insert(j.token.fingerprint.clone());
        }
        let open = self.is_home() && self.tokens.has_open(self.wall_ms);
        (fps.into_iter().collect(), open)
    }

    fn publish(&self, name: &str, data: Value) -> Effect {
        Effect::Publish {
            topic: "cluster".to_string(),
            name: name.to_string(),
            data,
        }
    }

    fn member_name(&self, id: &NodeId) -> String {
        self.state
            .as_ref()
            .and_then(|s| s.members.get(id))
            .map(|m| m.name.clone())
            .unwrap_or_else(|| id.to_string())
    }

    fn send(&self, to: &NodeId, msg: Msg, out: &mut Vec<Effect>) {
        out.push(Effect::Send {
            to: to.clone(),
            msg,
        });
    }

    fn is_online(&self, id: &NodeId) -> bool {
        self.peers
            .get(id)
            .is_some_and(|p| p.linked && p.greeted && p.online)
    }

    fn tool_op(tool: Tool) -> &'static str {
        match tool {
            Tool::Claude => "session.new",
            Tool::Codex => "codex.send",
            Tool::Agy => "agy.open",
            Tool::Shell => "shell.run",
        }
    }
}

#[cfg(test)]
mod core_tests;
#[cfg(test)]
mod core_tests_dispatch;
#[cfg(test)]
mod core_tests_terms;
