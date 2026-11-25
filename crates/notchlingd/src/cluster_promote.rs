use crate::acting::Acting;
use crate::cluster_fence_keeper::Boot;
use crate::cluster_move::{failed, lock, LateLink, Link, Mover};
use crate::cluster_node::LocalCalls;
use crate::cluster_promote_items::Items;
use notchling_engine::cluster::failover::REACH_MS;
use notchling_engine::cluster::move_home::NO_PROOF;
use notchling_engine::cluster::promote::{next, Facts, Promote, PromoteEvent, PromoteStep};
use notchling_engine::cluster::state::{ClusterState, Command};
use notchling_engine::cluster::versions::RESUME_PROOF_RECORDED;
use notchling_engine::cluster::wire::{Origin, OriginTier, WireError};
use notchling_engine::cluster::{LocalCmd, NodeId};
use notchling_local_api::ApiError;
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, VecDeque};
use std::sync::mpsc::{channel, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub const ANSWER_SLACK: Duration = Duration::from_secs(20);
pub const RECORD_WAIT: Duration = Duration::from_secs(30);

pub trait Cluster: Send + Sync {
    fn state(&self) -> Option<ClusterState>;
    fn status(&self) -> Value;
    fn call(
        &self,
        to: &NodeId,
        op: &str,
        args: Value,
        within: Duration,
    ) -> Result<Value, WireError>;
    fn record(&self, command: Command) -> Result<(), WireError>;
}

impl Cluster for LateLink {
    fn state(&self) -> Option<ClusterState> {
        self.0.get().and_then(|node| node.state())
    }

    fn status(&self) -> Value {
        Link::status(self)
    }

    fn call(
        &self,
        to: &NodeId,
        op: &str,
        args: Value,
        within: Duration,
    ) -> Result<Value, WireError> {
        Link::call(self, to, op, args, within)
    }

    fn record(&self, command: Command) -> Result<(), WireError> {
        self.local(
            LocalCmd::Record {
                command,
                tier: OriginTier::Node,
            },
            RECORD_WAIT,
        )
        .map(|_| ())
    }
}

pub trait Replier: Send + Sync {
    fn reply(&self, session: &str, text: &str) -> Result<(), String>;
}

pub struct DaemonReplier {
    pub daemon: Arc<dyn LocalCalls>,
    pub me: Arc<dyn Fn() -> Option<NodeId> + Send + Sync>,
}

impl Replier for DaemonReplier {
    fn reply(&self, session: &str, text: &str) -> Result<(), String> {
        let node = (self.me)().ok_or_else(|| "this node isn't in a cluster".to_string())?;
        let origin = Origin {
            node,
            tier: OriginTier::App,
        };
        self.daemon
            .call(
                "session.reply",
                &json!({"id": session, "text": text}),
                &origin,
            )
            .map(|_| ())
            .map_err(|e| e.message)
    }
}

pub struct PromoteParts {
    pub mover: Arc<Mover>,
    pub cluster: Arc<dyn Cluster>,
    pub replier: Arc<dyn Replier>,
    pub acting: Arc<Acting>,
    pub boot: Boot,
}

pub(crate) enum Answer {
    Relayed(String),
    NeedsConfirm(String),
    Step(String),
    Failed(String),
}

#[derive(Default)]
pub(crate) struct Run {
    pub promote: Option<Promote>,
    pub busy: bool,
    pub started_ms: i64,
    pub error: Option<String>,
    pub text: Option<String>,
    pub confirm: bool,
}

pub struct Promoter {
    pub(crate) p: PromoteParts,
    pub(crate) proof: bool,
    pub(crate) run: Mutex<Run>,
    pub(crate) thread: Mutex<Option<std::thread::JoinHandle<()>>>,
}

pub fn step_name(step: &PromoteStep) -> &'static str {
    match step {
        PromoteStep::Check => "check",
        PromoteStep::Reach => "reach",
        PromoteStep::Confirm => "confirm",
        PromoteStep::Bump => "bump",
        PromoteStep::Restore => "restore",
        PromoteStep::Resume => "resume",
        PromoteStep::Held => "held",
        PromoteStep::Items => "items",
        PromoteStep::Announce => "announce",
        PromoteStep::Done => "done",
        PromoteStep::Relayed => "relayed",
        PromoteStep::Failed(_) => "failed",
    }
}

pub(crate) struct Ctx {
    pub facts: Arc<Facts>,
    pub tx: Option<Sender<Answer>>,
    pub resumed: bool,
    pub failed: Vec<(String, String)>,
    pub unclaimed: BTreeMap<String, String>,
    pub steps: BTreeMap<String, usize>,
    pub run: u64,
}

impl Ctx {
    pub fn answer(&mut self, answer: Answer) -> bool {
        match self.tx.take() {
            Some(tx) => {
                let _ = tx.send(answer);
                true
            }
            None => false,
        }
    }

    fn answer_going_on(&mut self, promote: &Promote) {
        let going = matches!(
            promote.step,
            PromoteStep::Bump
                | PromoteStep::Restore
                | PromoteStep::Resume
                | PromoteStep::Held
                | PromoteStep::Items
                | PromoteStep::Announce
        );
        if going {
            self.answer(Answer::Step(step_name(&promote.step).to_string()));
        }
    }
}

impl Promoter {
    pub fn new(parts: PromoteParts) -> Arc<Promoter> {
        Promoter::with(parts, RESUME_PROOF_RECORDED)
    }

    pub fn with(parts: PromoteParts, proof: bool) -> Arc<Promoter> {
        Arc::new(Promoter {
            p: parts,
            proof,
            run: Mutex::new(Run::default()),
            thread: Mutex::new(None),
        })
    }

    pub fn items(&self) -> Items {
        Items::new(&self.p)
    }

    pub fn status(&self) -> Value {
        let run = lock(&self.run);
        let Some(promote) = &run.promote else {
            return Value::Null;
        };
        let step = if run.confirm {
            "confirm"
        } else {
            step_name(&promote.step)
        };
        let mut out = Map::new();
        out.insert("node".to_string(), json!(promote.node));
        out.insert("step".to_string(), json!(step));
        out.insert("started".to_string(), json!(run.started_ms));
        if let Some(error) = &run.error {
            out.insert("error".to_string(), json!(error));
        }
        if let Some(text) = &run.text {
            out.insert("text".to_string(), json!(text));
        }
        Value::Object(out)
    }

    pub fn running(&self) -> bool {
        lock(&self.run).busy
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

    pub fn start(self: &Arc<Self>, confirmed: bool) -> Result<Value, ApiError> {
        {
            let mut run = lock(&self.run);
            if run.busy {
                return Err(failed("A promote is already running"));
            }
            run.busy = true;
        }
        let began = self.begin(confirmed);
        if began.is_err() {
            let running = lock(&self.thread)
                .as_ref()
                .is_some_and(|handle| !handle.is_finished());
            if !running {
                lock(&self.run).busy = false;
            }
        }
        began
    }

    fn begin(self: &Arc<Self>, confirmed: bool) -> Result<Value, ApiError> {
        let me = self
            .p
            .mover
            .me()
            .ok_or_else(|| failed("This machine is in no cluster"))?;
        let state = self
            .p
            .cluster
            .state()
            .ok_or_else(|| failed("This machine is in no cluster"))?;
        let facts = Arc::new(self.facts(state, &me));
        if !self.proof
            && facts
                .set
                .as_ref()
                .is_some_and(crate::cluster_promote_facts::has_moving)
        {
            return Err(failed(NO_PROOF));
        }
        let now = (self.p.boot.now)();
        let promote = Promote::new(me, facts.state.home.clone(), false, confirmed, now);
        *lock(&self.run) = Run {
            promote: Some(promote.clone()),
            busy: true,
            started_ms: self.p.mover.now(),
            ..Run::default()
        };
        let (tx, rx) = channel();
        let runner = self.clone();
        let handle = std::thread::Builder::new()
            .name("cluster-promote".into())
            .spawn(move || {
                runner.drive(promote, facts, tx);
                lock(&runner.run).busy = false;
            })
            .map_err(|_| failed("Couldn't start the promote"))?;
        *lock(&self.thread) = Some(handle);
        match rx.recv_timeout(Duration::from_millis(REACH_MS) + ANSWER_SLACK) {
            Ok(Answer::Relayed(text)) => Ok(json!({"relayed": true, "text": text})),
            Ok(Answer::NeedsConfirm(text)) => Ok(json!({"needsConfirm": text})),
            Ok(Answer::Step(step)) => Ok(json!({"step": step})),
            Ok(Answer::Failed(reason)) => Err(failed(reason)),
            Err(_) => Err(failed("The promote didn't answer")),
        }
    }

    fn drive(&self, mut promote: Promote, facts: Arc<Facts>, tx: std::sync::mpsc::Sender<Answer>) {
        let mut ctx = Ctx {
            facts: facts.clone(),
            tx: Some(tx),
            resumed: false,
            failed: Vec::new(),
            unclaimed: BTreeMap::new(),
            steps: BTreeMap::new(),
            run: (self.p.boot.now)(),
        };
        let mut events: VecDeque<PromoteEvent> = VecDeque::from([PromoteEvent::Begin(facts)]);
        while let Some(event) = events.pop_front() {
            let (after, actions) = next(&promote, event, (self.p.boot.now)());
            promote = after;
            lock(&self.run).promote = Some(promote.clone());
            ctx.answer_going_on(&promote);
            for action in actions {
                events.extend(self.perform(&mut ctx, action));
            }
            events.extend(self.after_batch(&mut ctx));
            if promote.is_terminal() {
                break;
            }
        }
    }
}
