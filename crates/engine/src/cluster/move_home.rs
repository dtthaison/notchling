use serde::{Deserialize, Serialize};

use super::id::NodeId;
use super::snapshot::Movable;
use super::versions::Compat;

pub const MOVE_FREEZE: &str = "move.freeze";
pub const MOVE_STOP: &str = "move.stop";
pub const MOVE_SWITCH: &str = "move.switch";
pub const MOVE_RESUME: &str = "move.resume";
pub const MOVE_UNDO: &str = "move.undo";
pub const FREEZE_WAIT_MS: i64 = 600_000;
pub const SWITCH_NOTICE_TRIES: u32 = 150;
pub const NO_PROOF: &str = "Moving the primary needs the resume proof first";
pub const RESUME_START_MS: u64 = 75_000;
pub const RESUME_CAP_MS: u64 = 20 * 60_000;
pub const RESUME_TRIES: u32 = 5;
pub const IN_PROGRESS: &str = "A handover is in progress here";

pub fn resume_call_ms(sessions: usize) -> u64 {
    (RESUME_START_MS * sessions as u64 + 15_000).min(RESUME_CAP_MS)
}

pub fn moving_text(to_name: &str) -> String {
    format!("The primary is moving to {to_name}")
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub enum MoveStep {
    Prepare,
    CatchUp,
    Freeze,
    FinalSnapshot,
    StopOld,
    Switch,
    Resume,
    Announce,
    Done,
    Undo(String),
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct MoveSaga {
    pub from: NodeId,
    pub to: NodeId,
    pub step: MoveStep,
    pub started_ms: i64,
    pub moving: Vec<String>,
    pub staying: Vec<(String, String)>,
    pub now: bool,
    #[serde(default)]
    pub term: u64,
    #[serde(default)]
    pub to_name: String,
    #[serde(default)]
    pub froze_ms: Option<i64>,
    #[serde(default)]
    pub cut: Vec<String>,
    #[serde(default)]
    pub stopped: Vec<String>,
    #[serde(default)]
    pub tries: u32,
    #[serde(default)]
    pub failed: Vec<(String, String)>,
    #[serde(default)]
    pub stopping: Vec<String>,
    #[serde(default)]
    pub maybe_sent: bool,
    #[serde(default)]
    pub set: Option<String>,
    #[serde(default)]
    pub final_ms: Option<i64>,
    #[serde(default)]
    pub unfenced: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ToFacts {
    pub name: String,
    pub member: bool,
    pub can_be_master: bool,
    pub online: bool,
    pub compat: Compat,
    pub ready: Result<(), String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct MoveFacts {
    pub term: u64,
    pub is_home: bool,
    pub to: ToFacts,
    pub main: Vec<String>,
    pub set: Option<Vec<(String, Movable)>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Prepared {
    pub moving: Vec<String>,
    pub staying: Vec<(String, String)>,
}

#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct StopResult {
    pub done: Vec<String>,
    pub failed: Option<(String, String)>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FinalSet {
    pub set: String,
    pub left: Vec<(String, String)>,
    pub at_ms: i64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SwitchResult {
    Done,
    DoneUnfenced(String),
    Refused(String),
    NotSent(String),
    Lost(String),
}

#[derive(Clone, Debug, PartialEq)]
pub enum MoveEvent {
    Checked(MoveFacts),
    CaughtUp(Result<(), String>),
    Frozen { now_ms: i64 },
    Working { now_ms: i64, working: Vec<String> },
    MoveNow { working: Vec<String> },
    Cut(StopResult),
    Final(Result<FinalSet, String>),
    Stopped(StopResult),
    Switched(SwitchResult),
    Resumed(Vec<(String, Result<(), String>)>),
    Announced,
    Restarted,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MoveAction {
    Check,
    CatchUp,
    Freeze {
        sessions: Vec<String>,
    },
    Poll,
    StopCut {
        sessions: Vec<String>,
    },
    Fence,
    FinalSnapshot,
    HoldSets,
    StopOld {
        sessions: Vec<String>,
    },
    Switch {
        term: u64,
        attempt: u32,
    },
    Resume {
        sessions: Vec<String>,
    },
    Announce,
    Guard,
    Unfreeze,
    Restart {
        sessions: Vec<String>,
        unless_live: Vec<String>,
    },
    Notice(String),
    ResumeFailed {
        session: String,
        why: String,
    },
}

pub fn failure_notice(reason: &str) -> String {
    format!("Couldn't move the primary: {reason}")
}

pub fn announce_notice(name: &str) -> String {
    format!("{name} is primary now")
}

pub fn unfenced_notice(sessions: &[String], why: &str) -> (String, String) {
    let names = sessions.join(", ");
    let detail = if sessions.len() == 1 {
        format!("{why}. It may run on both machines until you stop it here.")
    } else {
        format!("{why}. They may run on both machines until you stop them here.")
    };
    (
        format!("Couldn't pause {names} on this machine after the move"),
        detail,
    )
}

pub fn step_name(step: &MoveStep) -> &'static str {
    match step {
        MoveStep::Prepare => "prepare",
        MoveStep::CatchUp => "catch_up",
        MoveStep::Freeze => "freeze",
        MoveStep::FinalSnapshot => "final_snapshot",
        MoveStep::StopOld => "stop_old",
        MoveStep::Switch => "switch",
        MoveStep::Resume => "resume",
        MoveStep::Announce => "announce",
        MoveStep::Done => "done",
        MoveStep::Undo(_) => "undone",
    }
}

pub fn prepare(
    from: &NodeId,
    to: &NodeId,
    facts: &MoveFacts,
    proof: bool,
) -> Result<Prepared, String> {
    if !proof {
        return Err(NO_PROOF.to_string());
    }
    let name = &facts.to.name;
    if !facts.is_home {
        return Err("Only the primary can hand itself over".to_string());
    }
    if from == to {
        return Err(format!("{name} is primary already"));
    }
    if !facts.to.member {
        return Err("That machine isn't in this cluster".to_string());
    }
    if !facts.to.can_be_master {
        return Err(format!("{name} can't be primary: Can Be Master is off"));
    }
    if !facts.to.online {
        return Err(format!("{name} is offline"));
    }
    match &facts.to.compat {
        Compat::Ok => {}
        Compat::Warn(why) | Compat::Refuse(why) => return Err(why.clone()),
    }
    if let Err(why) = &facts.to.ready {
        return Err(format!("{name} isn't ready: {why}"));
    }
    Ok(split(&facts.main, facts.set.as_deref()))
}

pub fn split(main: &[String], set: Option<&[(String, Movable)]>) -> Prepared {
    let mut moving = Vec::new();
    let mut staying = Vec::new();
    for id in main {
        let entry = set.and_then(|entries| entries.iter().find(|(entry, _)| entry == id));
        match entry {
            Some((_, Movable::No(why))) => staying.push((id.clone(), why.clone())),
            _ => moving.push(id.clone()),
        }
    }
    moving.sort();
    staying.sort();
    Prepared { moving, staying }
}

pub fn left_behind(moving: &[String], set: &[(String, Movable)]) -> Vec<(String, String)> {
    moving
        .iter()
        .filter_map(|id| match set.iter().find(|(entry, _)| entry == id) {
            Some((_, Movable::Yes)) => None,
            Some((_, Movable::No(why))) => Some((id.clone(), why.clone())),
            None => Some((id.clone(), "It isn't in the final snapshot".to_string())),
        })
        .collect()
}

impl MoveSaga {
    pub fn new(from: NodeId, to: NodeId, started_ms: i64, now: bool) -> MoveSaga {
        MoveSaga {
            from,
            to,
            step: MoveStep::Prepare,
            started_ms,
            moving: Vec::new(),
            staying: Vec::new(),
            now,
            term: 0,
            to_name: String::new(),
            froze_ms: None,
            cut: Vec::new(),
            stopped: Vec::new(),
            tries: 0,
            failed: Vec::new(),
            stopping: Vec::new(),
            maybe_sent: false,
            set: None,
            final_ms: None,
            unfenced: None,
        }
    }

    pub fn start(
        from: NodeId,
        to: NodeId,
        started_ms: i64,
        now: bool,
    ) -> (MoveSaga, Vec<MoveAction>) {
        (
            MoveSaga::new(from, to, started_ms, now),
            vec![MoveAction::Check],
        )
    }

    pub fn finished(&self) -> bool {
        matches!(self.step, MoveStep::Done | MoveStep::Undo(_))
    }

    pub fn switching(&self) -> bool {
        matches!(
            self.step,
            MoveStep::Switch | MoveStep::Resume | MoveStep::Announce | MoveStep::Done
        )
    }

    pub fn fenced(&self) -> bool {
        matches!(
            self.step,
            MoveStep::FinalSnapshot
                | MoveStep::StopOld
                | MoveStep::Switch
                | MoveStep::Resume
                | MoveStep::Announce
        )
    }

    pub fn mark_stopping(&mut self, id: &str) {
        self.stopping = vec![id.to_string()];
    }

    pub fn mark_stopped(&mut self, id: &str) {
        self.stopping.retain(|s| s != id);
        if !self.stopped.iter().any(|s| s == id) {
            self.stopped.push(id.to_string());
        }
    }

    fn took_stops(&mut self, result: StopResult) {
        for id in result.done {
            self.mark_stopped(&id);
        }
        if let Some((id, _)) = result.failed {
            if !self.stopping.contains(&id) && !self.stopped.contains(&id) {
                self.stopping.push(id);
            }
        }
    }

    fn undo(&self, reason: &str) -> (MoveSaga, Vec<MoveAction>) {
        if self.switching() || self.finished() {
            return (self.clone(), Vec::new());
        }
        self.compensate(reason)
    }

    fn compensate(&self, reason: &str) -> (MoveSaga, Vec<MoveAction>) {
        let mut next = self.clone();
        next.step = MoveStep::Undo(reason.to_string());
        let mut actions = vec![MoveAction::Unfreeze];
        if !self.stopped.is_empty() || !self.stopping.is_empty() {
            actions.push(MoveAction::Restart {
                sessions: self.stopped.clone(),
                unless_live: self.stopping.clone(),
            });
        }
        actions.push(MoveAction::Notice(failure_notice(reason)));
        (next, actions)
    }

    fn stop_old(&self) -> Vec<String> {
        self.moving
            .iter()
            .filter(|id| !self.stopped.contains(id))
            .cloned()
            .collect()
    }

    fn switch_action(&self) -> MoveAction {
        MoveAction::Switch {
            term: self.term + 1,
            attempt: self.tries,
        }
    }

    fn waiting(&self, now_ms: i64, working: Vec<String>) -> (MoveSaga, Vec<MoveAction>) {
        let mut next = self.clone();
        let working: Vec<String> = working
            .into_iter()
            .filter(|id| self.moving.contains(id))
            .collect();
        if working.is_empty() {
            next.step = MoveStep::FinalSnapshot;
            return (next, vec![MoveAction::Fence, MoveAction::FinalSnapshot]);
        }
        if self.now {
            next.cut = working.clone();
            return (next, vec![MoveAction::StopCut { sessions: working }]);
        }
        let started = self.froze_ms.unwrap_or(now_ms);
        if now_ms - started >= FREEZE_WAIT_MS {
            let names = working.join(", ");
            return self.undo(&format!("{names} still working after 10 minutes"));
        }
        (next, vec![MoveAction::Poll])
    }
}

pub fn next(s: &MoveSaga, ev: MoveEvent, proof: bool) -> (MoveSaga, Vec<MoveAction>) {
    if s.finished() {
        return (s.clone(), Vec::new());
    }
    match (&s.step, ev) {
        (_, MoveEvent::Restarted) => restarted(s),
        (MoveStep::Prepare, MoveEvent::Checked(facts)) => {
            match prepare(&s.from, &s.to, &facts, proof) {
                Err(why) => s.undo(&why),
                Ok(prepared) => {
                    let mut next = s.clone();
                    next.moving = prepared.moving;
                    next.staying = prepared.staying;
                    next.term = facts.term;
                    next.to_name = facts.to.name;
                    next.step = MoveStep::CatchUp;
                    (next, vec![MoveAction::CatchUp])
                }
            }
        }
        (MoveStep::CatchUp, MoveEvent::CaughtUp(Err(why))) => s.undo(&why),
        (MoveStep::CatchUp, MoveEvent::CaughtUp(Ok(()))) => {
            let mut next = s.clone();
            next.step = MoveStep::Freeze;
            (
                next,
                vec![MoveAction::Freeze {
                    sessions: s.moving.clone(),
                }],
            )
        }
        (MoveStep::Freeze, MoveEvent::Frozen { now_ms }) => {
            let mut next = s.clone();
            next.froze_ms = Some(now_ms);
            (next, vec![MoveAction::Poll])
        }
        (MoveStep::Freeze, MoveEvent::Working { now_ms, working }) => s.waiting(now_ms, working),
        (MoveStep::Freeze, MoveEvent::MoveNow { working }) => {
            let mut next = s.clone();
            next.now = true;
            let at = next.froze_ms.unwrap_or(next.started_ms);
            next.waiting(at, working)
        }
        (_, MoveEvent::MoveNow { .. }) => {
            let mut next = s.clone();
            next.now = true;
            (next, Vec::new())
        }
        (MoveStep::Freeze, MoveEvent::Cut(result)) => {
            let mut next = s.clone();
            let failed = result.failed.clone();
            next.took_stops(result);
            match failed {
                Some((id, why)) => next.undo(&format!("{id} didn't stop: {why}")),
                None => {
                    next.step = MoveStep::FinalSnapshot;
                    (next, vec![MoveAction::Fence, MoveAction::FinalSnapshot])
                }
            }
        }
        (MoveStep::FinalSnapshot, MoveEvent::Final(Err(why))) => s.undo(&why),
        (MoveStep::FinalSnapshot, MoveEvent::Final(Ok(found))) => final_taken(s, found),
        (MoveStep::StopOld, MoveEvent::Stopped(result)) => {
            let mut next = s.clone();
            let failed = result.failed.clone();
            next.took_stops(result);
            match failed {
                Some((id, why)) => next.undo(&format!("{id} didn't stop: {why}")),
                None => {
                    next.step = MoveStep::Switch;
                    let action = next.switch_action();
                    (next, vec![action])
                }
            }
        }
        (MoveStep::Switch, MoveEvent::Switched(result)) => switched(s, result),
        (MoveStep::Resume, MoveEvent::Resumed(results)) => {
            let mut next = s.clone();
            let mut actions = Vec::new();
            for (session, result) in results {
                if let Err(why) = result {
                    next.failed.push((session.clone(), why.clone()));
                    actions.push(MoveAction::ResumeFailed { session, why });
                }
            }
            next.step = MoveStep::Announce;
            actions.push(MoveAction::Announce);
            (next, actions)
        }
        (MoveStep::Announce, MoveEvent::Announced) => {
            let mut next = s.clone();
            next.step = MoveStep::Done;
            (next, vec![MoveAction::Unfreeze])
        }
        _ => (s.clone(), Vec::new()),
    }
}

pub fn unsaved(
    prev: &MoveSaga,
    after: &MoveSaga,
    why: &str,
) -> Option<(MoveSaga, Vec<MoveAction>)> {
    if prev.switching() || after.finished() {
        return None;
    }
    Some(after.compensate(&format!("the move couldn't be saved: {why}")))
}

fn final_taken(s: &MoveSaga, found: FinalSet) -> (MoveSaga, Vec<MoveAction>) {
    let mut next = s.clone();
    let left = found.left;
    next.set = Some(found.set);
    next.final_ms = Some(found.at_ms);
    next.moving
        .retain(|id| !left.iter().any(|(gone, _)| gone == id));
    let back: Vec<String> = next
        .stopped
        .iter()
        .filter(|id| left.iter().any(|(gone, _)| gone == *id))
        .cloned()
        .collect();
    next.stopped.retain(|id| !back.contains(id));
    next.staying.extend(left);
    next.staying.sort();
    next.step = MoveStep::StopOld;
    let mut actions = vec![MoveAction::HoldSets];
    if !back.is_empty() {
        actions.push(MoveAction::Restart {
            sessions: back,
            unless_live: Vec::new(),
        });
    }
    let sessions = next.stop_old();
    actions.push(MoveAction::StopOld { sessions });
    (next, actions)
}

fn switched(s: &MoveSaga, result: SwitchResult) -> (MoveSaga, Vec<MoveAction>) {
    let mut next = s.clone();
    let surely_unsent = !s.maybe_sent && s.tries == 0;
    match result {
        SwitchResult::Done => {
            next.step = MoveStep::Resume;
            let sessions = next.moving.clone();
            (next, vec![MoveAction::Resume { sessions }])
        }
        SwitchResult::DoneUnfenced(why) => {
            next.step = MoveStep::Resume;
            next.unfenced = Some(why);
            let sessions = next.moving.clone();
            (next, vec![MoveAction::Resume { sessions }])
        }
        SwitchResult::Refused(why) | SwitchResult::NotSent(why) if surely_unsent => {
            s.compensate(&why)
        }
        SwitchResult::Refused(why) | SwitchResult::NotSent(why) | SwitchResult::Lost(why) => {
            next.maybe_sent = true;
            next.tries += 1;
            let mut actions = vec![next.switch_action()];
            if next.tries == SWITCH_NOTICE_TRIES {
                actions.push(MoveAction::Notice(format!(
                    "Couldn't finish moving the primary to {}: {why}",
                    next.to_name
                )));
            }
            (next, actions)
        }
    }
}

fn restarted(s: &MoveSaga) -> (MoveSaga, Vec<MoveAction>) {
    match s.step {
        MoveStep::Prepare
        | MoveStep::CatchUp
        | MoveStep::Freeze
        | MoveStep::FinalSnapshot
        | MoveStep::StopOld => s.undo("Notchling restarted during the move"),
        MoveStep::Switch => {
            let mut next = s.clone();
            next.maybe_sent = true;
            let action = next.switch_action();
            (next, vec![MoveAction::Guard, action])
        }
        MoveStep::Resume => (
            s.clone(),
            vec![
                MoveAction::Guard,
                MoveAction::Resume {
                    sessions: s.moving.clone(),
                },
            ],
        ),
        MoveStep::Announce => (s.clone(), vec![MoveAction::Guard, MoveAction::Announce]),
        MoveStep::Done | MoveStep::Undo(_) => (s.clone(), Vec::new()),
    }
}
