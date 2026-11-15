use std::sync::Arc;

use super::claims::Claim;
use super::failover::{promote_safety, Failover, Grantor, Safety, REACH_MS};
use super::id::{NodeId, Tool};
use super::members::{MemberState, Term, MAX_TERM_N};
use super::outward::CutStep;
use super::returns::{continue_here, Notice};
use super::snapshot::repo_state::Movable;
use super::state::{ClusterState, Command};
use super::versions::{compat, Compat, Purpose, Versions};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PromoteStep {
    Check,
    Reach,
    Confirm,
    Bump,
    Restore,
    Resume,
    Held,
    Items,
    Announce,
    Done,
    Relayed,
    Failed(String),
}

#[derive(Clone, Debug, PartialEq)]
pub struct MovingSession {
    pub id: String,
    pub tool: Tool,
    pub movable: Movable,
    pub flags: Vec<String>,
    pub previous: Option<Claim>,
    pub base_len: u64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SetFacts {
    pub id: String,
    pub versions: Versions,
    pub sessions: Vec<MovingSession>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Facts {
    pub state: ClusterState,
    pub grantor: Grantor,
    pub mine: Versions,
    pub ready: Result<(), String>,
    pub peers: Vec<NodeId>,
    pub set: Option<SetFacts>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RestoredSession {
    pub session: String,
    pub cut: Vec<CutStep>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum PromoteEvent {
    Begin(Arc<Facts>),
    Answer,
    NoAnswer,
    Tick,
    Bumped(Result<(), String>),
    Restored(Result<Vec<RestoredSession>, String>),
    Resumed { failed: Vec<(String, String)> },
    HeldRun,
    Announced,
    Abort(String),
}

#[derive(Clone, Debug, PartialEq)]
pub enum PromoteAction {
    Probe {
        primary: NodeId,
        via: Vec<NodeId>,
        within_ms: u64,
    },
    Relayed {
        text: String,
    },
    NeedsConfirm {
        text: String,
    },
    Fail {
        reason: String,
    },
    Revoke,
    Journal(Box<Command>),
    PushTerm {
        term: Term,
    },
    Restore {
        sessions: Vec<String>,
    },
    Resume {
        session: String,
        flags: Vec<String>,
    },
    RunHeld,
    PausedStep {
        session: String,
        step: CutStep,
    },
    Announce(Notice),
    ClusterChanged,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Promote {
    pub node: NodeId,
    pub old_home: NodeId,
    pub auto: bool,
    pub confirmed: bool,
    pub step: PromoteStep,
    pub started: u64,
    pub step_at: u64,
    facts: Option<Arc<Facts>>,
    resumed: Vec<RestoredSession>,
}

impl Promote {
    pub fn new(node: NodeId, old_home: NodeId, auto: bool, confirmed: bool, now: u64) -> Promote {
        Promote {
            node,
            old_home,
            auto,
            confirmed,
            step: PromoteStep::Check,
            started: now,
            step_at: now,
            facts: None,
            resumed: Vec::new(),
        }
    }

    pub fn is_terminal(&self) -> bool {
        matches!(
            self.step,
            PromoteStep::Done | PromoteStep::Relayed | PromoteStep::Failed(_)
        )
    }
}

pub fn next(p: &Promote, ev: PromoteEvent, now: u64) -> (Promote, Vec<PromoteAction>) {
    let mut q = p.clone();
    let actions = match (&p.step, ev) {
        (PromoteStep::Done | PromoteStep::Relayed | PromoteStep::Failed(_), _) => Vec::new(),
        (_, PromoteEvent::Abort(reason)) => fail(&mut q, reason),
        (PromoteStep::Check, PromoteEvent::Begin(facts)) => check(&mut q, facts, now),
        (PromoteStep::Reach, PromoteEvent::Answer) if within_reach(p, now) => relay(&mut q),
        (PromoteStep::Reach, PromoteEvent::Answer | PromoteEvent::Tick)
            if now >= p.step_at.saturating_add(REACH_MS) =>
        {
            confirm(&mut q, now)
        }
        (PromoteStep::Reach, PromoteEvent::NoAnswer) => confirm(&mut q, now),
        (PromoteStep::Bump, PromoteEvent::Bumped(Ok(()))) => bumped(&mut q, now),
        (PromoteStep::Bump, PromoteEvent::Bumped(Err(reason))) => fail(&mut q, reason),
        (PromoteStep::Restore, PromoteEvent::Restored(Ok(done))) => restored(&mut q, done, now),
        (PromoteStep::Restore, PromoteEvent::Restored(Err(reason))) => fail(&mut q, reason),
        (PromoteStep::Resume, PromoteEvent::Resumed { failed }) => {
            q.resumed
                .retain(|r| !failed.iter().any(|(session, _)| *session == r.session));
            hold(&mut q, now)
        }
        (PromoteStep::Held, PromoteEvent::HeldRun) => items_and_announce(&mut q, now),
        (PromoteStep::Announce, PromoteEvent::Announced) => {
            q.step = PromoteStep::Done;
            q.step_at = now;
            Vec::new()
        }
        _ => Vec::new(),
    };
    (q, actions)
}

fn within_reach(p: &Promote, now: u64) -> bool {
    now <= p.step_at.saturating_add(REACH_MS)
}

fn fail(q: &mut Promote, reason: String) -> Vec<PromoteAction> {
    q.step = PromoteStep::Failed(reason.clone());
    vec![PromoteAction::Fail { reason }]
}

fn name(state: &ClusterState, id: &NodeId) -> String {
    state
        .members
        .get(id)
        .map_or_else(|| id.to_string(), |m| m.name.clone())
}

fn check(q: &mut Promote, facts: Arc<Facts>, now: u64) -> Vec<PromoteAction> {
    let state = &facts.state;
    let me = name(state, &q.node);
    if state.home == q.node {
        return fail(q, format!("{me} is primary already"));
    }
    let Some(member) = state.members.get(&q.node) else {
        return fail(q, format!("{me} isn't in this cluster"));
    };
    if member.state != MemberState::Active {
        return fail(q, format!("{me} isn't an active member of this cluster"));
    }
    if !member.can_be_master {
        return fail(q, format!("{me} has Can Be Master off"));
    }
    if let Err(reason) = &facts.ready {
        return fail(q, reason.clone());
    }
    if let Some(set) = &facts.set {
        if let Compat::Refuse(reason) = compat(&set.versions, &facts.mine, &me, Purpose::Promote) {
            return fail(q, reason);
        }
    }
    let probe = PromoteAction::Probe {
        primary: q.old_home.clone(),
        via: facts.peers.clone(),
        within_ms: REACH_MS,
    };
    q.facts = Some(facts);
    q.step = PromoteStep::Reach;
    q.step_at = now;
    vec![probe]
}

fn relay(q: &mut Promote) -> Vec<PromoteAction> {
    let Some(facts) = q.facts.clone() else {
        return fail(q, "promote has no facts".to_string());
    };
    let text = format!(
        "{} is reachable through {}; nothing moved",
        name(&facts.state, &q.old_home),
        name(&facts.state, &q.node)
    );
    q.step = PromoteStep::Relayed;
    vec![PromoteAction::Relayed { text }]
}

fn confirm(q: &mut Promote, now: u64) -> Vec<PromoteAction> {
    let Some(facts) = q.facts.clone() else {
        return fail(q, "promote has no facts".to_string());
    };
    q.step = PromoteStep::Confirm;
    q.step_at = now;
    match promote_safety(&q.node, &facts.state, &facts.grantor, false, now) {
        Safety::Safe => bump(q, &facts),
        Safety::Relay => relay(q),
        Safety::NeedsConfirm(_) if q.confirmed => bump(q, &facts),
        Safety::NeedsConfirm(text) => {
            q.step = PromoteStep::Failed(text.clone());
            vec![PromoteAction::NeedsConfirm { text }]
        }
    }
}

fn new_term(facts: &Facts, me: &NodeId) -> Result<Term, String> {
    match facts.state.term.n.checked_add(1) {
        Some(n) if n <= MAX_TERM_N => Ok(Term {
            n,
            node: me.clone(),
        }),
        _ => Err(format!(
            "{} can't be promoted: the cluster's term can't go any higher",
            name(&facts.state, me)
        )),
    }
}

fn bump(q: &mut Promote, facts: &Facts) -> Vec<PromoteAction> {
    let term = match new_term(facts, &q.node) {
        Ok(term) => term,
        Err(reason) => return fail(q, reason),
    };
    q.step = PromoteStep::Bump;
    vec![
        PromoteAction::Revoke,
        PromoteAction::Journal(Box::new(Command::Term { term })),
    ]
}

fn moving(facts: &Facts) -> Vec<&MovingSession> {
    facts.set.as_ref().map_or_else(Vec::new, |set| {
        set.sessions
            .iter()
            .filter(|s| s.movable == Movable::Yes && matches!(s.tool, Tool::Claude | Tool::Codex))
            .collect()
    })
}

fn bumped(q: &mut Promote, now: u64) -> Vec<PromoteAction> {
    let Some(facts) = q.facts.clone() else {
        return fail(q, "promote has no facts".to_string());
    };
    let term = match new_term(&facts, &q.node) {
        Ok(term) => term,
        Err(reason) => return fail(q, reason),
    };
    let mut actions = vec![PromoteAction::PushTerm { term }];
    let sessions: Vec<String> = moving(&facts).iter().map(|s| s.id.clone()).collect();
    if sessions.is_empty() {
        actions.extend(hold(q, now));
        return actions;
    }
    q.step = PromoteStep::Restore;
    q.step_at = now;
    actions.push(PromoteAction::Restore { sessions });
    actions
}

fn restored(q: &mut Promote, done: Vec<RestoredSession>, now: u64) -> Vec<PromoteAction> {
    let Some(facts) = q.facts.clone() else {
        return fail(q, "promote has no facts".to_string());
    };
    let Some(set) = &facts.set else {
        return hold(q, now);
    };
    let term = match new_term(&facts, &q.node) {
        Ok(term) => term,
        Err(reason) => return fail(q, reason),
    };
    let mut actions = Vec::new();
    for session in moving(&facts) {
        let Some(found) = done.iter().find(|r| r.session == session.id) else {
            continue;
        };
        let claim = continue_here(
            &session.id,
            &q.node,
            &term,
            session.previous.as_ref(),
            &set.id,
            session.base_len,
        );
        actions.push(PromoteAction::Journal(Box::new(Command::Claim { claim })));
        if session.tool == Tool::Claude {
            actions.push(PromoteAction::Resume {
                session: session.id.clone(),
                flags: session.flags.clone(),
            });
        }
        q.resumed.push(found.clone());
    }
    if !actions
        .iter()
        .any(|a| matches!(a, PromoteAction::Resume { .. }))
    {
        actions.extend(hold(q, now));
        return actions;
    }
    q.step = PromoteStep::Resume;
    q.step_at = now;
    actions
}

fn hold(q: &mut Promote, now: u64) -> Vec<PromoteAction> {
    q.step = PromoteStep::Held;
    q.step_at = now;
    vec![PromoteAction::RunHeld]
}

fn delay_text(failover: Failover) -> Option<&'static str> {
    match failover {
        Failover::Off => None,
        Failover::After30m => Some("30 minutes"),
        Failover::After1h => Some("1 hour"),
    }
}

fn items_and_announce(q: &mut Promote, now: u64) -> Vec<PromoteAction> {
    let Some(facts) = q.facts.clone() else {
        return fail(q, "promote has no facts".to_string());
    };
    let mut actions: Vec<PromoteAction> = q
        .resumed
        .iter()
        .flat_map(|r| {
            r.cut.iter().map(|step| PromoteAction::PausedStep {
                session: r.session.clone(),
                step: step.clone(),
            })
        })
        .collect();
    let mut text = format!("{} is primary now", name(&facts.state, &q.node));
    if let (true, Some(delay)) = (q.auto, delay_text(facts.state.failover)) {
        text.push_str(&format!(
            ", after {} was unreachable for {delay}",
            name(&facts.state, &q.old_home)
        ));
    }
    q.step = PromoteStep::Announce;
    q.step_at = now;
    actions.push(PromoteAction::Announce(Notice {
        kind: "failover".to_string(),
        text,
        actions: Vec::new(),
    }));
    actions.push(PromoteAction::ClusterChanged);
    actions
}
