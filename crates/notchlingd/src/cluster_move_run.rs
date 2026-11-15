use crate::cluster_move::{failed, lock, Mover};
use notchling_engine::cluster::move_home::{
    announce_notice, next, prepare, MoveAction, MoveEvent, MoveSaga, MoveStep,
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

    pub fn recover(self: &Arc<Self>) {
        let saga = lock(&self.run).saga.clone();
        let Some(saga) = saga.filter(|s| !s.finished()) else {
            return;
        };
        self.spawn(saga, vec![MoveEvent::Restarted]);
    }

    pub fn status(&self) -> Value {
        match &lock(&self.run).saga {
            None => Value::Null,
            Some(s) => {
                let (step, reason) = match &s.step {
                    MoveStep::Undo(why) => ("undone".to_string(), Some(why.clone())),
                    other => (format!("{other:?}").to_lowercase(), None),
                };
                json!({
                    "to": s.to,
                    "step": step,
                    "moving": s.moving,
                    "staying": s.staying,
                    "cut": s.cut,
                    "stopped": s.stopped,
                    "failed": s.failed,
                    "reason": reason,
                })
            }
        }
    }

    pub fn start(self: &Arc<Self>, to: &NodeId, now: bool) -> Result<Value, ApiError> {
        if lock(&self.run).busy {
            return Err(failed("A move is already running"));
        }
        let me = self
            .mover
            .me()
            .ok_or_else(|| failed("This machine is in no cluster"))?;
        let facts = self.mover.facts(to);
        let ready = prepare(&me, to, &facts, self.proof).map_err(failed)?;
        let saga = MoveSaga::new(me, to.clone(), self.mover.now(), now);
        self.spawn(saga, vec![MoveEvent::Checked(facts)]);
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

    fn spawn(self: &Arc<Self>, saga: MoveSaga, first: Vec<MoveEvent>) {
        lock(&self.run).busy = true;
        lock(&self.run).saga = Some(saga.clone());
        let me = self.clone();
        let handle = std::thread::Builder::new()
            .name("cluster-move".into())
            .spawn(move || {
                me.drive(saga, first);
                lock(&me.run).busy = false;
            })
            .ok();
        *lock(&self.thread) = handle;
    }

    fn save(&self, saga: &MoveSaga) {
        lock(&self.run).saga = Some(saga.clone());
        let Ok(bytes) = serde_json::to_vec(saga) else {
            return;
        };
        let temp = self.dir.join(format!("{SAGA_FILE}.tmp"));
        if std::fs::write(&temp, bytes).is_ok() {
            let _ = std::fs::rename(&temp, self.dir.join(SAGA_FILE));
        }
    }

    fn drive(&self, mut saga: MoveSaga, first: Vec<MoveEvent>) {
        let mut events: VecDeque<MoveEvent> = first.into();
        while let Some(event) = events.pop_front() {
            let (after, actions) = next(&saga, event, self.proof);
            saga = after;
            self.save(&saga);
            for action in actions {
                if let Some(result) = self.perform(&saga, action) {
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

    fn perform(&self, saga: &MoveSaga, action: MoveAction) -> Option<MoveEvent> {
        let mover = &self.mover;
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
            MoveAction::StopCut { sessions } => Some(MoveEvent::Cut(mover.stop(&sessions))),
            MoveAction::FinalSnapshot => Some(MoveEvent::Final(
                mover.final_snapshot(&saga.to, &saga.moving),
            )),
            MoveAction::StopOld { sessions } => Some(MoveEvent::Stopped(mover.stop(&sessions))),
            MoveAction::Switch { attempt, .. } => {
                if attempt > 0 {
                    self.nudge_wait(self.retry);
                }
                Some(MoveEvent::Switched(mover.switch(&saga.to, &saga.moving)))
            }
            MoveAction::Resume { sessions } => {
                Some(MoveEvent::Resumed(mover.resume(&saga.to, &sessions)))
            }
            MoveAction::Announce => {
                mover.p.acts.primary_moved(&saga.to);
                mover.notice(
                    &format!("cluster-move-{}", saga.started_ms),
                    Kind::Notice,
                    &announce_notice(&saga.to_name),
                    "",
                );
                Some(MoveEvent::Announced)
            }
            MoveAction::Unfreeze => {
                mover.thaw();
                None
            }
            MoveAction::Restart { sessions } => {
                for (id, result) in mover.restart(&sessions) {
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
