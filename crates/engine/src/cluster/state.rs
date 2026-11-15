use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::claims::{hide_side_copies, put_claim, put_side_copy, touch, Claim, SideCopy};
use super::failover::{ArmingId, Failover};
use super::grants::SecretAllow;
use super::id::{NodeId, Role};
use super::jobs::{job_id, Job, JobSpec, JobState};
use super::members::{Desired, Member, MemberState, Term};
use super::versions::Versions;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct ClusterState {
    pub id: String,
    pub name: String,
    pub term: Term,
    pub home: NodeId,
    pub vault: NodeId,
    pub members: BTreeMap<NodeId, Member>,
    pub jobs: BTreeMap<String, Job>,
    pub seq: u64,
    #[serde(default)]
    pub failover: Failover,
    #[serde(default)]
    pub armed: Option<(NodeId, Term)>,
    #[serde(default)]
    pub holders: BTreeMap<NodeId, ArmingId>,
    #[serde(default)]
    pub claims: BTreeMap<String, Claim>,
    #[serde(default)]
    pub side_copies: Vec<SideCopy>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Command {
    Create {
        id: String,
        name: String,
        home: Member,
    },
    Sync {
        id: String,
        name: String,
        term: Term,
        home: NodeId,
        vault: NodeId,
        members: Vec<Member>,
    },
    Term {
        term: Term,
    },
    ProposeJoin {
        member: Member,
        item: String,
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
        #[serde(default)]
        headless: Option<bool>,
        desired: Option<Desired>,
    },
    SetAllow {
        node: NodeId,
        allow: SecretAllow,
    },
    Versions {
        node: NodeId,
        versions: Versions,
    },
    Dispatch {
        key: String,
        spec: JobSpec,
    },
    JobState {
        id: String,
        state: JobState,
        session: Option<String>,
        detail: Option<String>,
    },
    SetFailover {
        failover: Failover,
    },
    Armed {
        node: NodeId,
        term: Term,
        id: ArmingId,
    },
    Released {
        node: NodeId,
        seen: Option<ArmingId>,
    },
    Claim {
        claim: Claim,
    },
    Touched {
        session: String,
    },
    SideCopy {
        copy: SideCopy,
    },
    HideSideCopy {
        session: String,
        node: NodeId,
    },
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct JournalEntry {
    pub seq: u64,
    pub at_ms: i64,
    pub term: Term,
    pub command: Command,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum Change {
    Member(Member),
    Dropped(NodeId),
    Job(Job),
    Failover(Failover),
    Armed(Option<(NodeId, Term)>),
    Holders(BTreeMap<NodeId, ArmingId>),
    Claims(BTreeMap<String, Claim>),
    SideCopies(Vec<SideCopy>),
    Cluster {
        id: String,
        name: String,
        term: Term,
        home: NodeId,
        vault: NodeId,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CommandError {
    NotActive(NodeId),
    Unknown(NodeId),
    Removed(NodeId),
    BadTransition,
    Exists,
    TermTooLarge,
}

impl std::fmt::Display for CommandError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CommandError::NotActive(n) => write!(f, "{n} is not an active member"),
            CommandError::Unknown(n) => write!(f, "{n} is not a member"),
            CommandError::Removed(n) => write!(f, "{n} was removed"),
            CommandError::BadTransition => f.write_str("that change is not allowed now"),
            CommandError::Exists => f.write_str("already there"),
            CommandError::TermTooLarge => f.write_str("that term is too large"),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Applied {
    pub entry: Option<JournalEntry>,
    pub changes: Vec<Change>,
}

impl Applied {
    fn none() -> Applied {
        Applied {
            entry: None,
            changes: Vec::new(),
        }
    }
}

impl ClusterState {
    pub fn rejoin_home(&self, me: &NodeId) -> Option<NodeId> {
        if self.home == *me {
            return None;
        }
        let mine = self.members.get(me)?;
        let home = self.members.get(&self.home)?;
        let rejoins = mine.state == MemberState::Active
            && home.state == MemberState::Active
            && mine.headless
            && !mine.client;
        rejoins.then(|| self.home.clone())
    }

    pub fn job_by_key(&self, key: &str) -> Option<&Job> {
        self.jobs.values().find(|j| j.key == key)
    }

    pub fn active(&self) -> impl Iterator<Item = &Member> {
        self.members
            .values()
            .filter(|m| m.state == MemberState::Active)
    }

    fn cluster_change(&self) -> Change {
        Change::Cluster {
            id: self.id.clone(),
            name: self.name.clone(),
            term: self.term.clone(),
            home: self.home.clone(),
            vault: self.vault.clone(),
        }
    }

    fn disarm(&mut self, node: &NodeId, changes: &mut Vec<Change>) {
        if self.armed.as_ref().is_some_and(|(n, _)| n == node) {
            self.armed = None;
            changes.push(Change::Armed(None));
        }
    }

    fn forget_arming(&mut self, changes: &mut Vec<Change>) {
        if self.armed.take().is_some() {
            changes.push(Change::Armed(None));
        }
        if self.holders.remove(&self.home).is_some() {
            changes.push(Change::Holders(self.holders.clone()));
        }
    }

    fn entry(&mut self, now_ms: i64, command: Command) -> JournalEntry {
        self.seq += 1;
        JournalEntry {
            seq: self.seq,
            at_ms: now_ms,
            term: self.term.clone(),
            command,
        }
    }
}

pub fn role_of(state: &ClusterState, id: &NodeId) -> Role {
    if *id == state.home {
        return Role::Home;
    }
    match state.members.get(id) {
        Some(m) if m.can_be_master => Role::Standby,
        _ => Role::Worker,
    }
}

pub fn start(
    command: Command,
    now_ms: i64,
) -> Result<(ClusterState, JournalEntry, Vec<Change>), CommandError> {
    let (id, name, term, home_id, vault, members) = match &command {
        Command::Create { id, name, home } => {
            let mut home = home.clone();
            home.state = MemberState::Active;
            home.can_be_master = true;
            home.joined_ms = now_ms;
            (
                id.clone(),
                name.clone(),
                Term {
                    n: 1,
                    node: home.id.clone(),
                },
                home.id.clone(),
                home.id.clone(),
                vec![home],
            )
        }
        Command::Sync {
            id,
            name,
            term,
            home,
            vault,
            members,
        } => {
            if !term.storable() {
                return Err(CommandError::TermTooLarge);
            }
            (
                id.clone(),
                name.clone(),
                term.clone(),
                home.clone(),
                vault.clone(),
                members.clone(),
            )
        }
        _ => return Err(CommandError::BadTransition),
    };
    let mut state = ClusterState {
        id,
        name,
        term,
        home: home_id,
        vault,
        members: members.iter().map(|m| (m.id.clone(), m.clone())).collect(),
        jobs: BTreeMap::new(),
        seq: 0,
        failover: Failover::Off,
        armed: None,
        holders: BTreeMap::new(),
        claims: BTreeMap::new(),
        side_copies: Vec::new(),
    };
    let mut changes = vec![state.cluster_change()];
    changes.extend(members.into_iter().map(|m| {
        let stored = state.members.get(&m.id).cloned().unwrap_or(m);
        Change::Member(stored)
    }));
    let entry = state.entry(now_ms, command);
    Ok((state, entry, changes))
}

pub fn apply(
    state: &mut ClusterState,
    command: Command,
    now_ms: i64,
) -> Result<Applied, CommandError> {
    let changes = match &command {
        Command::Create { .. } => return Err(CommandError::Exists),
        Command::Sync {
            id,
            name,
            term,
            home,
            vault,
            members,
        } => sync(state, id, name, term, home, vault, members)?,
        Command::Term { term } => {
            if !term.storable() {
                return Err(CommandError::TermTooLarge);
            }
            if *term <= state.term {
                return Err(CommandError::BadTransition);
            }
            state.term = term.clone();
            state.home = term.node.clone();
            let mut changes = vec![state.cluster_change()];
            state.forget_arming(&mut changes);
            changes
        }
        Command::ProposeJoin { member, .. } => propose(state, member, now_ms)?,
        Command::Approve {
            node,
            client,
            can_be_master,
        } => {
            let m = state
                .members
                .get_mut(node)
                .ok_or_else(|| CommandError::Unknown(node.clone()))?;
            if m.state != MemberState::Pending {
                return Err(CommandError::BadTransition);
            }
            m.state = MemberState::Active;
            m.client = *client;
            m.can_be_master = *can_be_master;
            m.joined_ms = now_ms;
            vec![Change::Member(m.clone())]
        }
        Command::Deny { node } => {
            let m = state
                .members
                .get(node)
                .ok_or_else(|| CommandError::Unknown(node.clone()))?;
            if m.state != MemberState::Pending {
                return Err(CommandError::BadTransition);
            }
            state.members.remove(node);
            vec![Change::Dropped(node.clone())]
        }
        Command::Remove { node } => {
            if *node == state.home {
                return Err(CommandError::BadTransition);
            }
            let m = active_mut(state, node)?;
            m.state = MemberState::Removed;
            let mut changes = vec![Change::Member(m.clone())];
            state.disarm(node, &mut changes);
            changes
        }
        Command::SetMember {
            node,
            can_be_master,
            client,
            reference,
            accepts_delegation,
            headless,
            desired,
        } => {
            let m = active_mut(state, node)?;
            if let Some(v) = headless {
                m.headless = *v;
            }
            if let Some(v) = can_be_master {
                m.can_be_master = *v;
            }
            if let Some(v) = client {
                m.client = *v;
            }
            if let Some(v) = reference {
                m.reference = *v;
            }
            if let Some(v) = accepts_delegation {
                m.accepts_delegation = *v;
            }
            if let Some(v) = desired {
                m.desired = *v;
            }
            let mut changes = vec![Change::Member(m.clone())];
            if *can_be_master == Some(false) {
                state.disarm(node, &mut changes);
            }
            changes
        }
        Command::SetAllow { node, allow } => {
            let next = SecretAllow::new(allow.names.iter().cloned(), allow.any);
            if next.check().is_err() {
                return Err(CommandError::BadTransition);
            }
            let m = active_mut(state, node)?;
            m.secrets = next;
            vec![Change::Member(m.clone())]
        }
        Command::Versions { node, versions } => {
            let m = state
                .members
                .get_mut(node)
                .ok_or_else(|| CommandError::Unknown(node.clone()))?;
            if m.state == MemberState::Removed {
                return Err(CommandError::Removed(node.clone()));
            }
            if m.versions.as_ref() == Some(versions) {
                return Ok(Applied::none());
            }
            m.versions = Some(versions.clone());
            vec![Change::Member(m.clone())]
        }
        Command::Dispatch { key, spec } => match dispatch(state, key, spec, now_ms)? {
            Some(change) => vec![change],
            None => return Ok(Applied::none()),
        },
        Command::JobState {
            id,
            state: next,
            session,
            detail,
        } => {
            let term = state.term.clone();
            let job = state.jobs.get_mut(id).ok_or(CommandError::BadTransition)?;
            let records_session = *next == job.state && job.session.is_none() && session.is_some();
            if !records_session && !job.state.may_become(*next) {
                return Err(CommandError::BadTransition);
            }
            job.state = *next;
            job.term = term;
            if session.is_some() {
                job.session = session.clone();
            }
            if detail.is_some() {
                job.detail = detail.clone();
            }
            vec![Change::Job(job.clone())]
        }
        Command::SetFailover { failover } => {
            if state.failover == *failover {
                return Ok(Applied::none());
            }
            state.failover = *failover;
            state.armed = None;
            vec![Change::Failover(*failover), Change::Armed(None)]
        }
        Command::Armed { node, term, id } => {
            if *term != state.term || state.failover == Failover::Off || *node == state.home {
                return Err(CommandError::BadTransition);
            }
            if state.holders.keys().any(|other| other != node) {
                return Err(CommandError::BadTransition);
            }
            let held = state.holders.get(node).cloned();
            if held.as_ref().is_some_and(|h| id < h) {
                return Err(CommandError::BadTransition);
            }
            if !active_mut(state, node)?.can_be_master {
                return Err(CommandError::BadTransition);
            }
            let armed = Some((node.clone(), term.clone()));
            if state.armed == armed && held.as_ref() == Some(id) {
                return Ok(Applied::none());
            }
            state.armed = armed.clone();
            state.holders.insert(node.clone(), id.clone());
            vec![Change::Armed(armed), Change::Holders(state.holders.clone())]
        }
        Command::Released { node, seen } => {
            if state
                .holders
                .get(node)
                .is_some_and(|armed_at| seen.as_ref().is_none_or(|s| armed_at > s))
            {
                return Ok(Applied::none());
            }
            let held = state.holders.remove(node).is_some();
            let mut changes = Vec::new();
            if held {
                changes.push(Change::Holders(state.holders.clone()));
            }
            let armed = state.armed.as_ref().is_some_and(|(n, _)| n == node);
            if armed {
                state.armed = None;
                changes.push(Change::Armed(None));
            }
            if changes.is_empty() {
                return Ok(Applied::none());
            }
            changes
        }
        Command::Claim { claim } => {
            if !claim.term.storable() {
                return Err(CommandError::TermTooLarge);
            }
            active_mut(state, &claim.node)?;
            if !put_claim(&mut state.claims, claim) {
                return Ok(Applied::none());
            }
            vec![Change::Claims(state.claims.clone())]
        }
        Command::Touched { session } => match touch(&mut state.claims, session) {
            None => return Err(CommandError::BadTransition),
            Some(false) => return Ok(Applied::none()),
            Some(true) => vec![Change::Claims(state.claims.clone())],
        },
        Command::SideCopy { copy } => {
            if !put_side_copy(&mut state.side_copies, copy, now_ms) {
                return Ok(Applied::none());
            }
            vec![Change::SideCopies(state.side_copies.clone())]
        }
        Command::HideSideCopy { session, node } => {
            match hide_side_copies(&mut state.side_copies, session, node) {
                None => return Err(CommandError::BadTransition),
                Some(false) => return Ok(Applied::none()),
                Some(true) => vec![Change::SideCopies(state.side_copies.clone())],
            }
        }
    };
    let entry = state.entry(now_ms, command);
    Ok(Applied {
        entry: Some(entry),
        changes,
    })
}

fn active_mut<'a>(
    state: &'a mut ClusterState,
    node: &NodeId,
) -> Result<&'a mut Member, CommandError> {
    let m = state
        .members
        .get_mut(node)
        .ok_or_else(|| CommandError::Unknown(node.clone()))?;
    match m.state {
        MemberState::Active => Ok(m),
        MemberState::Removed => Err(CommandError::Removed(node.clone())),
        MemberState::Pending => Err(CommandError::NotActive(node.clone())),
    }
}

fn propose(
    state: &mut ClusterState,
    member: &Member,
    now_ms: i64,
) -> Result<Vec<Change>, CommandError> {
    if let Some(existing) = state.members.get(&member.id) {
        if existing.state != MemberState::Removed {
            return Err(CommandError::Exists);
        }
    }
    let mut member = member.clone();
    member.state = MemberState::Pending;
    member.client = false;
    member.can_be_master = false;
    member.reference = false;
    member.accepts_delegation = false;
    member.desired = Desired::default();
    member.secrets = SecretAllow::default();
    member.joined_ms = now_ms;
    state.members.insert(member.id.clone(), member.clone());
    Ok(vec![Change::Member(member)])
}

fn dispatch(
    state: &mut ClusterState,
    key: &str,
    spec: &JobSpec,
    now_ms: i64,
) -> Result<Option<Change>, CommandError> {
    if state.job_by_key(key).is_some() {
        return Ok(None);
    }
    active_mut(state, &spec.node)?;
    let id = job_id(key);
    if state.jobs.contains_key(&id) {
        return Err(CommandError::Exists);
    }
    let job = Job {
        id: id.clone(),
        key: key.to_string(),
        spec: spec.clone(),
        state: JobState::Dispatching,
        term: state.term.clone(),
        created_ms: now_ms,
        session: None,
        detail: None,
    };
    state.jobs.insert(id, job.clone());
    Ok(Some(Change::Job(job)))
}

fn sync(
    state: &mut ClusterState,
    id: &str,
    name: &str,
    term: &Term,
    home: &NodeId,
    vault: &NodeId,
    members: &[Member],
) -> Result<Vec<Change>, CommandError> {
    if !term.storable() {
        return Err(CommandError::TermTooLarge);
    }
    if id != state.id || *term < state.term {
        return Err(CommandError::BadTransition);
    }
    let mut changes = Vec::new();
    let term_changed = state.term != *term;
    let head_changed =
        state.name != name || state.term != *term || state.home != *home || state.vault != *vault;
    state.name = name.to_string();
    state.term = term.clone();
    state.home = home.clone();
    state.vault = vault.clone();
    if head_changed {
        changes.push(state.cluster_change());
    }
    if term_changed {
        state.forget_arming(&mut changes);
    }
    let gone: Vec<NodeId> = state
        .members
        .keys()
        .filter(|k| !members.iter().any(|m| &m.id == *k))
        .cloned()
        .collect();
    for node in gone {
        state.members.remove(&node);
        changes.push(Change::Dropped(node));
    }
    for m in members {
        if state.members.get(&m.id) != Some(m) {
            state.members.insert(m.id.clone(), m.clone());
            changes.push(Change::Member(m.clone()));
        }
    }
    Ok(changes)
}
