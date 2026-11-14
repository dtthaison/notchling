use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

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
}

impl std::fmt::Display for CommandError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CommandError::NotActive(n) => write!(f, "{n} is not an active member"),
            CommandError::Unknown(n) => write!(f, "{n} is not a member"),
            CommandError::Removed(n) => write!(f, "{n} was removed"),
            CommandError::BadTransition => f.write_str("that change is not allowed now"),
            CommandError::Exists => f.write_str("already there"),
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
        } => (
            id.clone(),
            name.clone(),
            term.clone(),
            home.clone(),
            vault.clone(),
            members.clone(),
        ),
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
            if *term <= state.term {
                return Err(CommandError::BadTransition);
            }
            state.term = term.clone();
            state.home = term.node.clone();
            vec![state.cluster_change()]
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
            vec![Change::Member(m.clone())]
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
            if !job.state.may_become(*next) {
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
    if id != state.id || *term < state.term {
        return Err(CommandError::BadTransition);
    }
    let mut changes = Vec::new();
    let head_changed =
        state.name != name || state.term != *term || state.home != *home || state.vault != *vault;
    state.name = name.to_string();
    state.term = term.clone();
    state.home = home.clone();
    state.vault = vault.clone();
    if head_changed {
        changes.push(state.cluster_change());
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
