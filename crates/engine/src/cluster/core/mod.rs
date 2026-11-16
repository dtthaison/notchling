mod control;
mod dispatch;
mod failover_set;
mod forward;
mod join;
mod link;
mod record;
mod snap;
mod takeover;

use std::collections::{BTreeMap, BTreeSet};

use serde_json::{json, Value};

use super::failover::Failover;
use super::id::{NodeId, Tool};
use super::jobs::JobSpec;
use super::join::{JoinToken, Tokens};
use super::members::{Desired, Member, MemberState, Term};
use super::snapshot::{Observed, Trigger};
use super::state::{role_of, Applied, Change, ClusterState, Command, CommandError, JournalEntry};
use super::text::{plain, NAME_MAX};
use super::versions::Versions;
use super::wire::{Args, Msg, Origin, OriginTier, WireError};
use crate::sessions::Agent;

pub use forward::call_wait;
pub use snap::SnapAsk;
pub use takeover::{eligible, Takeover, NOT_SENT, REFUSED};

pub const PING_MS: u64 = 5_000;
pub const OFFLINE_MS: u64 = 20_000;
pub const CALL_MS: u64 = 30_000;
pub const DELEGATE_CALL_MS: u64 = 36 * 60_000;
pub const WORKSPACE_CALL_MS: u64 = 20 * 60_000;
pub const MOVE_CALL_MS: u64 = 5 * 60_000;
pub const STATUS_JOBS: usize = 50;
pub const JOIN_ANSWER_MS: u64 = 15_000;
pub const DIAL_BACKOFF_MS: (u64, u64) = (1_000, 60_000);
pub const DEDUPE_MS: u64 = 300_000;
pub const MAX_TERM_STEP: u64 = 64;
pub const REASON_MAX: usize = 200;
pub const PEER_STALE_TERM: &str = "peer_stale_term";
pub const VERSIONS_GAP_MS: u64 = 10_000;
pub const ATTACH_CALL_MS: u64 = 8 * 60_000;
pub const GRANT_ATTACH: &str = "grant.attach";
pub const GRANT_REVOKE_JOB: &str = "grant.revoke_job";

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
    Observed {
        observed: Observed,
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
        minter: NodeId,
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
        headless: Option<bool>,
        desired: Option<Desired>,
    },
    SetVersions {
        versions: Versions,
    },
    SetFailover {
        failover: Failover,
    },
    SetAllow {
        node: NodeId,
        names: Option<Vec<String>>,
        any: Option<bool>,
    },
    Dispatch {
        key: String,
        spec: JobSpec,
        tier: OriginTier,
    },
    Forward {
        node: NodeId,
        op: String,
        args: Args,
        origin: Origin,
    },
    JobEvent {
        name: String,
        data: Value,
    },
    JoinWith {
        token: JoinToken,
        me: Member,
    },
    SetMain {
        session: String,
        main: bool,
    },
    MainSeen {
        agents: Vec<Agent>,
        codex_exec: BTreeSet<String>,
        respawn: BTreeMap<String, Vec<String>>,
    },
    Snapshot {
        trigger: Trigger,
    },
    TakePrimary {
        takeover: Box<Takeover>,
    },
    MoveSwitch {
        to: NodeId,
        sessions: Vec<String>,
        set: Option<String>,
    },
    Fence {
        reason: Option<String>,
    },
    Record {
        command: Command,
        tier: OriginTier,
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
        args: Args,
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
        headless: bool,
        minter: NodeId,
        minter_name: String,
    },
    SteppedDown {
        term: Term,
        home: NodeId,
    },
    Wipe,
    Snapshot {
        plan: SnapAsk,
    },
    AdoptStaged,
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

#[derive(Clone, Debug, Default)]
struct VersionsGate {
    last: Option<u64>,
    held: Option<Versions>,
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
    Nobody,
}

#[derive(Clone, Debug)]
enum Then {
    Reply(Target),
    Dispatch {
        job: String,
        warning: Option<String>,
        target: Target,
    },
    Attach {
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
    since_ms: i64,
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
    versions_gate: BTreeMap<NodeId, VersionsGate>,
    versions_gap_ms: u64,
    joining: Option<Joining>,
    next_wire: u64,
    next_local: u64,
    last_pins: Option<(Vec<String>, bool)>,
    changed: bool,
    aliases: BTreeMap<String, String>,
    snap: snap::SnapCore,
    fence: Option<String>,
    primary_armed: bool,
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
            versions_gate: BTreeMap::new(),
            versions_gap_ms: VERSIONS_GAP_MS,
            joining: None,
            next_wire: 1,
            next_local: 1,
            last_pins: None,
            changed: false,
            aliases: BTreeMap::new(),
            snap: snap::SnapCore::default(),
            fence: None,
            primary_armed: false,
            #[cfg(test)]
            skip_term_check: false,
        }
    }

    pub fn with_boot_id(mut self, boot_id: String) -> NodeCore {
        self.boot_id = boot_id;
        self
    }

    pub fn with_versions_gap(mut self, ms: u64) -> NodeCore {
        self.versions_gap_ms = ms;
        self
    }

    pub fn counters(&self) -> (u64, u64) {
        (self.next_wire, self.next_local)
    }

    pub fn with_counters(mut self, (wire, local): (u64, u64)) -> NodeCore {
        self.next_wire = self.next_wire.max(wire);
        self.next_local = self.next_local.max(local);
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
            return json!({"cluster": Value::Null, "joining": self.joining_status(), "me": me, "members": [], "jobs": []});
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
                    "name": shown_name(&m.name, &m.id),
                    "role": role,
                    "canBeMaster": m.can_be_master,
                    "online": m.id == self.me || online.contains(&m.id),
                    "client": m.client,
                    "headless": m.headless,
                    "reference": m.reference,
                    "state": member_state,
                    "versions": m.versions,
                    "desired": m.desired,
                    "acceptsDelegation": m.accepts_delegation,
                    "secrets": m.secrets,
                })
            })
            .collect();
        let mut jobs: Vec<&super::jobs::Job> = state.jobs.values().collect();
        jobs.sort_by(|a, b| b.created_ms.cmp(&a.created_ms).then(a.id.cmp(&b.id)));
        let jobs: Vec<Value> = jobs
            .into_iter()
            .take(STATUS_JOBS)
            .map(|j| {
                let qualified = j
                    .session
                    .as_ref()
                    .map(|s| super::id::qualify(&j.spec.node, s));
                json!({
                    "id": j.id,
                    "node": j.spec.node,
                    "tool": j.spec.tool,
                    "cwd": j.spec.cwd,
                    "state": j.state,
                    "session": j.session,
                    "qualified": qualified,
                    "detail": j.detail,
                    "createdMs": j.created_ms,
                })
            })
            .collect();
        json!({
            "cluster": {
                "id": state.id,
                "name": plain(&state.name, NAME_MAX),
                "term": state.term,
                "home": state.home,
                "vault": state.vault,
            },
            "joining": Value::Null,
            "me": me,
            "members": members,
            "jobs": jobs,
            "main": state.main.keys().collect::<Vec<_>>(),
            "kept": state.kept,
            "failover": self.failover_status(state),
            "claims": state.claims.values().collect::<Vec<_>>(),
            "sideCopies": state.side_copies,
        })
    }

    fn joining_status(&self) -> Value {
        match &self.joining {
            Some(j) => json!({
                "since": j.since_ms,
                "cluster": j.token.cluster,
                "primary": j.token.home,
                "answered": j.answered,
            }),
            None => Value::Null,
        }
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
            Input::Observed { observed } => self.on_observed(observed, &mut out),
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
            self.changed = true;
        }
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
            .map(|m| shown_name(&m.name, &m.id))
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
            Tool::Agy => "agy.send",
            Tool::Shell => "shell.run",
        }
    }
}

pub(super) fn shown_name(name: &str, id: &NodeId) -> String {
    let shown = plain(name, NAME_MAX);
    if shown.is_empty() {
        return id.to_string();
    }
    shown
}

pub(super) fn shown_members(members: Vec<Member>) -> Vec<Member> {
    members
        .into_iter()
        .map(|mut m| {
            m.name = shown_name(&m.name, &m.id);
            m
        })
        .collect()
}

#[cfg(test)]
mod core_tests;
#[cfg(test)]
mod core_tests_adopt;
#[cfg(test)]
mod core_tests_claims;
#[cfg(test)]
mod core_tests_clean;
#[cfg(test)]
mod core_tests_dispatch;
#[cfg(test)]
mod core_tests_failover;
#[cfg(test)]
mod core_tests_flood;
#[cfg(test)]
mod core_tests_grants_fix;
#[cfg(test)]
mod core_tests_headless;
#[cfg(test)]
mod core_tests_heal;
#[cfg(test)]
mod core_tests_jobs;
#[cfg(test)]
mod core_tests_joining;
#[cfg(test)]
mod core_tests_main;
#[cfg(test)]
mod core_tests_minter;
#[cfg(test)]
mod core_tests_refused;
#[cfg(test)]
mod core_tests_relay;
#[cfg(test)]
mod core_tests_secrets;
#[cfg(test)]
mod core_tests_snap;
#[cfg(test)]
mod core_tests_takeover;
#[cfg(test)]
mod core_tests_takeover_members;
#[cfg(test)]
mod core_tests_terms;
