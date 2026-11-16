use crate::cluster_move::{failed, lock, Mover, Started};
use crate::cluster_move_calls::write_durably;
use notchling_engine::cluster::move_home::{
    announce_notice, next, prepare, step_name, unfenced_notice, unsaved, MoveAction, MoveEvent,
    MoveSaga, MoveStep, StopResult, RESUME_TRIES,
};
use notchling_engine::cluster::versions::RESUME_PROOF_RECORDED;
use notchling_engine::cluster::NodeId;
use notchling_engine::inbox::Kind;
use notchling_local_api::{ApiError, ErrorCode};
use serde_json::{json, Value};
use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

pub const SAGA_FILE: &str = "move-saga.json";
pub const WATCH: Duration = Duration::from_secs(2);
pub const RETRY: Duration = Duration::from_secs(1);

#[derive(Default)]
struct Run {
    saga: Option<MoveSaga>,
    busy: bool,
    nudged: bool,
}

pub struct MoveRunner {
    mover: Arc<Mover>,
    dir: PathBuf,
    proof: bool,
    watch: Duration,
    retry: Duration,
    run: Mutex<Run>,
    wake: Condvar,
    thread: Mutex<Option<std::thread::JoinHandle<()>>>,
}

impl MoveRunner {
    pub fn new(mover: Arc<Mover>, dir: PathBuf) -> Arc<MoveRunner> {
        MoveRunner::with(mover, dir, (RESUME_PROOF_RECORDED, WATCH, RETRY))
    }

    pub fn with(
        mover: Arc<Mover>,
        dir: PathBuf,
        (proof, watch, retry): (bool, Duration, Duration),
    ) -> Arc<MoveRunner> {
        let saga = std::fs::read(dir.join(SAGA_FILE))
            .ok()
            .and_then(|bytes| serde_json::from_slice::<MoveSaga>(&bytes).ok());
        Arc::new(MoveRunner {
            mover,
            dir,
            proof,
            watch,
            retry,
            run: Mutex::new(Run {
                saga,
                busy: false,
                nudged: false,
            }),
            wake: Condvar::new(),
            thread: Mutex::new(None),
        })
    }

    fn claim(&self) -> bool {
        let mut run = lock(&self.run);
        if run.busy {
            return false;
        }
        run.busy = true;
        run.nudged = false;
        true
    }

    fn release(&self) {
        lock(&self.run).busy = false;
    }

    #[cfg(test)]
    pub(crate) fn nudge_pending(&self) -> bool {
        lock(&self.run).nudged
    }

    pub fn recover(self: &Arc<Self>) {
        let saga = lock(&self.run).saga.clone();
        let Some(saga) = saga.filter(|s| !s.finished()) else {
            return;
        };
        if !self.claim() {
            return;
        }
        if self.spawn(saga, vec![MoveEvent::Restarted]).is_err() {
            self.release();
        }
    }

    pub fn status(&self) -> Value {
        match &lock(&self.run).saga {
            None => Value::Null,
            Some(s) => {
                let reason = match &s.step {
                    MoveStep::Undo(why) => Some(why.clone()),
                    _ => None,
                };
                json!({
                    "to": s.to,
                    "step": step_name(&s.step),
                    "moving": s.moving,
                    "staying": s.staying,
                    "cut": s.cut,
                    "stopped": s.stopped,
                    "failed": s.failed,
                    "unfenced": s.unfenced,
                    "reason": reason,
                })
            }
        }
    }

    pub fn start(self: &Arc<Self>, to: &NodeId, now: bool) -> Result<Value, ApiError> {
        if !self.claim() {
            return Err(failed("A move is already running"));
        }
        let begun = self.begin(to, now);
        if begun.is_err() {
            self.release();
        }
        begun
    }

    fn begin(self: &Arc<Self>, to: &NodeId, now: bool) -> Result<Value, ApiError> {
        let me = self
            .mover
            .me()
            .ok_or_else(|| failed("This machine is in no cluster"))?;
        let facts = self.mover.facts(to);
        let ready = prepare(&me, to, &facts, self.proof).map_err(failed)?;
        let saga = MoveSaga::new(me, to.clone(), self.mover.now(), now);
        self.spawn(saga, vec![MoveEvent::Checked(facts)])
            .map_err(|_| failed("Couldn't start the move"))?;
        Ok(json!({"started": true, "moving": ready.moving, "staying": ready.staying}))
    }

    pub fn move_now(&self) -> Result<Value, ApiError> {
        let mut run = lock(&self.run);
        if !run.busy {
            return Err(ApiError::new(ErrorCode::Failed, "No move is running"));
        }
        run.nudged = true;
        self.wake.notify_all();
        Ok(json!({"ok": true}))
    }

    pub fn wait_done(&self, within: Duration) -> bool {
        let handle = lock(&self.thread).take();
        let Some(handle) = handle else {
            return true;
        };
        let deadline = std::time::Instant::now() + within;
        while !handle.is_finished() && std::time::Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(5));
        }
        let done = handle.is_finished();
        if done {
            let _ = handle.join();
        } else {
            *lock(&self.thread) = Some(handle);
        }
        done
    }

    fn spawn(self: &Arc<Self>, saga: MoveSaga, first: Vec<MoveEvent>) -> std::io::Result<()> {
        lock(&self.run).saga = Some(saga.clone());
        let me = self.clone();
        let handle = std::thread::Builder::new()
            .name("cluster-move".into())
            .spawn(move || {
                me.drive(saga, first);
                me.release();
            })?;
        *lock(&self.thread) = Some(handle);
        Ok(())
    }

    fn save(&self, saga: &MoveSaga) -> Result<(), String> {
        lock(&self.run).saga = Some(saga.clone());
        let bytes = serde_json::to_vec(saga).map_err(|e| e.to_string())?;
        write_durably(&self.dir, SAGA_FILE, &bytes)
    }

    fn drive(&self, mut saga: MoveSaga, first: Vec<MoveEvent>) {
        let mut events: VecDeque<MoveEvent> = first.into();
        while let Some(event) = events.pop_front() {
            let prev = saga.clone();
            let (after, mut actions) = next(&saga, event, self.proof);
            saga = after;
            if let Err(why) = self.save(&saga) {
                if let Some((undone, undo)) = unsaved(&prev, &saga, &why) {
                    saga = undone;
                    actions = undo;
                    let _ = self.save(&saga);
                }
            }
            for action in actions {
                if let Some(result) = self.perform(&mut saga, action) {
                    events.push_back(result);
                }
            }
            if saga.finished() {
                break;
            }
        }
    }

    fn nudge_wait(&self, within: Duration) -> bool {
        let run = lock(&self.run);
        let (mut run, _) = self
            .wake
            .wait_timeout_while(run, within, |r| !r.nudged)
            .unwrap_or_else(|e| e.into_inner());
        std::mem::take(&mut run.nudged)
    }

    fn stop_journaled(&self, saga: &mut MoveSaga, sessions: &[String]) -> StopResult {
        let mut result = StopResult::default();
        for id in sessions {
            saga.mark_stopping(id);
            if let Err(why) = self.save(saga) {
                let why = format!("the move couldn't be saved: {why}");
                result.failed = Some((id.clone(), why));
                return result;
            }
            match self.mover.stop_one(id) {
                Ok(()) => {
                    saga.mark_stopped(id);
                    let _ = self.save(saga);
                    result.done.push(id.clone());
                }
                Err(why) => {
                    result.failed = Some((id.clone(), why));
                    return result;
                }
            }
        }
        result
    }

    fn resume(&self, saga: &MoveSaga, sessions: &[String]) -> Started {
        let mut last = String::new();
        for attempt in 0..RESUME_TRIES {
            if attempt > 0 {
                self.nudge_wait(self.retry);
            }
            match self.mover.resume(&saga.to, sessions) {
                Ok(rows) => return rows,
                Err(why) => last = why,
            }
        }
        sessions
            .iter()
            .map(|id| (id.clone(), Err(last.clone())))
            .collect()
    }

    fn announce(&self, saga: &MoveSaga) {
        let mover = &self.mover;
        mover.p.acts.primary_moved(&saga.to);
        let left = saga
            .final_ms
            .map(|at| mover.stragglers(at))
            .unwrap_or_default();
        let mut lines = Vec::new();
        if let Some(why) = &saga.unfenced {
            lines.push(unfenced_notice(&saga.moving, why).1);
        }
        if !left.is_empty() {
            lines.push(format!(
                "Raised on this machine during the move, still here: {}",
                left.join(", ")
            ));
        }
        let detail = lines.join("\n");
        mover.notice(
            &format!("cluster-move-{}", saga.started_ms),
            Kind::Notice,
            &announce_notice(&saga.to_name),
            &detail,
        );
    }

    fn perform(&self, saga: &mut MoveSaga, action: MoveAction) -> Option<MoveEvent> {
        let mover = self.mover.clone();
        match action {
            MoveAction::Check => Some(MoveEvent::Checked(mover.facts(&saga.to))),
            MoveAction::CatchUp => Some(MoveEvent::CaughtUp(mover.catch_up(&saga.to))),
            MoveAction::Freeze { sessions } => {
                mover.freeze(&sessions, &saga.to_name);
                Some(MoveEvent::Frozen {
                    now_ms: mover.now(),
                })
            }
            MoveAction::Poll => {
                let moved = self.nudge_wait(self.watch);
                let working = mover.working(&saga.moving);
                Some(if moved {
                    MoveEvent::MoveNow { working }
                } else {
                    MoveEvent::Working {
                        now_ms: mover.now(),
                        working,
                    }
                })
            }
            MoveAction::StopCut { sessions } => {
                Some(MoveEvent::Cut(self.stop_journaled(saga, &sessions)))
            }
            MoveAction::Fence => {
                mover.fence(&saga.to_name);
                None
            }
            MoveAction::FinalSnapshot => Some(MoveEvent::Final(
                mover.final_snapshot(&saga.to, &saga.moving),
            )),
            MoveAction::HoldSets => {
                mover.hold(&saga.to, true);
                None
            }
            MoveAction::StopOld { sessions } => {
                Some(MoveEvent::Stopped(self.stop_journaled(saga, &sessions)))
            }
            MoveAction::Switch { attempt, term } => {
                if attempt > 0 {
                    self.nudge_wait(self.retry);
                }
                Some(MoveEvent::Switched(mover.switch(
                    &saga.to,
                    &saga.moving,
                    saga.set.as_deref(),
                    term,
                )))
            }
            MoveAction::Resume { sessions } => {
                Some(MoveEvent::Resumed(self.resume(saga, &sessions)))
            }
            MoveAction::Announce => {
                self.announce(saga);
                Some(MoveEvent::Announced)
            }
            MoveAction::Guard => {
                mover.freeze(&saga.moving, &saga.to_name);
                mover.fence(&saga.to_name);
                mover.hold(&saga.to, true);
                None
            }
            MoveAction::Unfreeze => {
                mover.thaw();
                mover.unfence();
                mover.hold(&saga.to, false);
                None
            }
            MoveAction::Restart {
                sessions,
                unless_live,
            } => {
                for (id, result) in mover.restart(&sessions, &unless_live) {
                    if let Err(why) = result {
                        mover.notice(
                            &format!("cluster-move-restart-{id}"),
                            Kind::ActionFailed,
                            &format!("Couldn't restart {id}"),
                            &why,
                        );
                    }
                }
                None
            }
            MoveAction::Notice(text) => {
                mover.notice(
                    &format!("cluster-move-notice-{}", saga.started_ms),
                    Kind::Notice,
                    &text,
                    "",
                );
                None
            }
            MoveAction::ResumeFailed { session, why } => {
                mover.notice(
                    &format!("cluster-move-resume-{session}"),
                    Kind::ActionFailed,
                    &format!("Couldn't resume {session} on {}", saga.to_name),
                    &why,
                );
                None
            }
        }
    }
}
