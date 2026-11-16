use crate::cluster_fence::{clear_moved, fence_keys, mark_moved_in};
use crate::cluster_node::ClusterNode;
use crate::cluster_sessions::LocalRows;
use crate::inbox_ops::InboxOps;
use notchling_engine::cluster::core::Takeover;
use notchling_engine::cluster::main_sessions::MainSession;
use notchling_engine::cluster::members::Term;
use notchling_engine::cluster::move_home::{
    left_behind, moving_text, resume_call_ms, unfenced_notice, FinalSet, MoveFacts, StopResult,
    SwitchResult, ToFacts,
};
use notchling_engine::cluster::move_home::{
    MOVE_FREEZE, MOVE_RESUME, MOVE_STOP, MOVE_SWITCH, MOVE_UNDO,
};
use notchling_engine::cluster::persist;
use notchling_engine::cluster::snapshot::{Manifest, Movable};
use notchling_engine::cluster::versions::{compat, Compat, Purpose, Versions};
use notchling_engine::cluster::wire::{Origin, OriginTier, WireError};
use notchling_engine::cluster::{LocalCmd, NodeId, Tool};
use notchling_engine::control::session::{conversation_of, resume_plan_step};
use notchling_engine::control::{execute, Outcome as PlanOutcome, Plan, Step};
use notchling_engine::delegate::agy_trust::claude_trusts;
use notchling_engine::inbox::{Item, Kind, Source, State as ItemState};
use notchling_engine::sessions::agent::{Agent, AgentTarget};
use notchling_engine::sessions::lanes::Roots;
use notchling_local_api::{ops, ApiError, ErrorCode};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

pub const MOVE_OPS: [&str; 5] = [MOVE_FREEZE, MOVE_STOP, MOVE_SWITCH, MOVE_RESUME, MOVE_UNDO];
pub const SNAP_WAIT: Duration = Duration::from_secs(5 * 60);
pub const SWITCH_WAIT: Duration = Duration::from_secs(5 * 60 + 5);
pub const FENCE_TRIES: u32 = 3;
pub const FENCE_PAUSE: Duration = Duration::from_millis(150);
pub const FENCED_OPS: [&str; 5] = [
    ops::SESSION_NEW,
    ops::SESSION_MOVE,
    ops::CLUSTER_DISPATCH,
    ops::CLUSTER_MAIN_SET,
    ops::CLUSTER_RETURN,
];

pub type Started = Vec<(String, Result<(), String>)>;

pub fn resume_wait(sessions: usize) -> Duration {
    Duration::from_millis(resume_call_ms(sessions) + 5_000)
}

pub(crate) fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

pub fn failed(why: impl Into<String>) -> ApiError {
    ApiError::new(ErrorCode::Failed, why)
}

#[derive(Default)]
pub struct MoveGate {
    held: Mutex<Option<(BTreeSet<String>, String)>>,
    fence: Mutex<Option<String>>,
}

impl MoveGate {
    pub fn freeze(&self, sessions: &[String], to_name: &str) {
        *lock(&self.held) = Some((sessions.iter().cloned().collect(), to_name.to_string()));
    }

    pub fn thaw(&self) {
        *lock(&self.held) = None;
    }

    pub fn fence(&self, to_name: &str) {
        *lock(&self.fence) = Some(moving_text(to_name));
    }

    pub fn unfence(&self) {
        *lock(&self.fence) = None;
    }

    pub fn frozen(&self) -> Vec<String> {
        lock(&self.held)
            .as_ref()
            .map(|(ids, _)| ids.iter().cloned().collect())
            .unwrap_or_default()
    }

    pub fn check(
        &self,
        op: &str,
        args: &Value,
        prompt_session: &dyn Fn(&str) -> Option<String>,
    ) -> Result<(), ApiError> {
        if let Some(why) = lock(&self.fence).as_ref() {
            if FENCED_OPS.contains(&op) {
                return Err(failed(why.clone()));
            }
        }
        let held = lock(&self.held);
        let Some((ids, to_name)) = held.as_ref() else {
            return Ok(());
        };
        if !matches!(
            op,
            "session.reply" | "session.answer" | "session.permission" | "codex.send"
        ) {
            return Ok(());
        }
        let Some(id) = args.get("id").and_then(Value::as_str) else {
            return Ok(());
        };
        let moving = ids.contains(id) || prompt_session(id).is_some_and(|s| ids.contains(&s));
        if moving {
            return Err(failed(format!("That session is moving to {to_name}")));
        }
        Ok(())
    }
}

pub trait Link: Send + Sync {
    fn status(&self) -> Value;
    fn switch(
        &self,
        to: &NodeId,
        sessions: &[String],
        set: Option<&str>,
    ) -> Result<Value, WireError>;
    fn call(
        &self,
        to: &NodeId,
        op: &str,
        args: Value,
        within: Duration,
    ) -> Result<Value, WireError>;
    fn take_primary(&self, takeover: Takeover) -> Result<Value, WireError>;
    fn fence(&self, reason: Option<String>);
}

#[derive(Default)]
pub struct LateLink(pub std::sync::OnceLock<Arc<ClusterNode>>);

impl LateLink {
    pub fn local(&self, cmd: LocalCmd, within: Duration) -> Result<Value, WireError> {
        self.node()?.local_wire(cmd, within)
    }

    fn node(&self) -> Result<&Arc<ClusterNode>, WireError> {
        self.0
            .get()
            .ok_or_else(|| WireError::new("not_sent", "the cluster node isn't running"))
    }
}

impl Link for LateLink {
    fn status(&self) -> Value {
        self.0
            .get()
            .map(|node| node.status())
            .unwrap_or_else(|| json!({"cluster": Value::Null, "members": []}))
    }

    fn switch(
        &self,
        to: &NodeId,
        sessions: &[String],
        set: Option<&str>,
    ) -> Result<Value, WireError> {
        self.node()?.local_wire(
            LocalCmd::MoveSwitch {
                to: to.clone(),
                sessions: sessions.to_vec(),
                set: set.map(str::to_string),
            },
            SWITCH_WAIT,
        )
    }

    fn call(
        &self,
        to: &NodeId,
        op: &str,
        args: Value,
        within: Duration,
    ) -> Result<Value, WireError> {
        let node = self.node()?;
        let origin = Origin {
            node: node.me().clone(),
            tier: OriginTier::Node,
        };
        node.local_wire(
            LocalCmd::Forward {
                node: to.clone(),
                op: op.to_string(),
                args: args.into(),
                origin,
            },
            within,
        )
    }

    fn take_primary(&self, takeover: Takeover) -> Result<Value, WireError> {
        self.node()?.local_wire(
            LocalCmd::TakePrimary {
                takeover: Box::new(takeover),
            },
            Duration::from_secs(30),
        )
    }

    fn fence(&self, reason: Option<String>) {
        let _ = self.local(LocalCmd::Fence { reason }, Duration::from_secs(30));
    }
}

pub trait Acts: Send + Sync {
    fn stop_claude(&self, id: &str) -> Result<(), String>;
    fn stop_codex(&self, id: &str);
    fn start_claude(&self, session: &MainSession) -> Result<(), String>;

    fn primary_moved(&self, _to: &NodeId) {}

    fn db_replaced(&self) {}
}

pub struct RealActs {
    pub daemon: Arc<dyn crate::cluster_node::LocalCalls>,
    pub me: Arc<dyn Fn() -> Option<NodeId> + Send + Sync>,
    pub launcher: Arc<dyn notchling_engine::control::Launcher>,
    pub roots: Roots,
    pub pause: Arc<dyn Fn(u64) + Send + Sync>,
}

impl RealActs {
    fn origin(&self) -> Result<Origin, String> {
        let node = (self.me)().ok_or_else(|| "this node isn't in a cluster".to_string())?;
        Ok(Origin {
            node,
            tier: OriginTier::App,
        })
    }
}

impl Acts for RealActs {
    fn stop_claude(&self, id: &str) -> Result<(), String> {
        self.daemon
            .call("session.stop", &json!({"id": id}), &self.origin()?)
            .map(|_| ())
            .map_err(|e| e.message)
    }

    fn stop_codex(&self, id: &str) {
        if let Ok(origin) = self.origin() {
            let _ = self.daemon.call("codex.stop", &json!({"id": id}), &origin);
        }
    }

    fn start_claude(&self, session: &MainSession) -> Result<(), String> {
        let transcript = session
            .transcript
            .as_deref()
            .ok_or_else(|| "it has no transcript to resume".to_string())?;
        let conversation = conversation_of(transcript)
            .ok_or_else(|| "its transcript has no conversation id".to_string())?;
        let step = resume_plan_step(
            &self.roots,
            &session.id,
            &conversation,
            &session.cwd,
            session.config_dir.as_deref(),
            &session.respawn,
        );
        let plan = Plan {
            steps: vec![Step::Run(step)],
            outcome: PlanOutcome::NewJob,
        };
        let pause = |ms: u64| (self.pause)(ms);
        execute(&plan, self.launcher.as_ref(), &pause)
            .map(|_| ())
            .map_err(|e| e.to_string())
    }
}

pub type ReadyCheck = Arc<dyn Fn(&Value, &NodeId) -> Result<(), String> + Send + Sync>;

pub struct MoveParts {
    pub link: Arc<dyn Link>,
    pub acts: Arc<dyn Acts>,
    pub rows: Arc<dyn LocalRows>,
    pub snaps: Arc<dyn Snaps>,
    pub inbox: Arc<InboxOps>,
    pub gate: Arc<MoveGate>,
    pub cluster: PathBuf,
    pub clock: Arc<dyn Fn() -> i64 + Send + Sync>,
    pub ready: ReadyCheck,
    pub restore: Arc<dyn Restorer>,
}

pub trait Snaps: Send + Sync {
    fn latest(&self) -> Option<Manifest>;
    fn catch_up(&self, to: &NodeId, name: &str) -> Result<(), String>;
    fn final_set(&self, to: &NodeId, name: &str) -> Result<Manifest, String>;
    fn hold(&self, to: &NodeId, on: bool);
}

pub trait Restorer: Send + Sync {
    fn check(
        &self,
        primary: &NodeId,
        node: &str,
        set: Option<&str>,
        sessions: &BTreeSet<String>,
    ) -> Result<(), String>;
    fn restore(
        &self,
        primary: &NodeId,
        node: &str,
        sessions: &BTreeSet<String>,
        set: Option<&str>,
        stage: &Path,
    ) -> Result<(), String>;
}

fn claude_json_of(home: &Path, transcript: Option<&PathBuf>) -> PathBuf {
    let config = transcript.and_then(|t| t.ancestors().nth(3));
    match config {
        Some(dir) if dir != home.join(".claude") => dir.join(".claude.json"),
        _ => home.join(".claude.json"),
    }
}

pub fn trust_check(
    home: &Path,
    manifest: &Manifest,
    sessions: &BTreeSet<String>,
    node: &str,
) -> Result<(), String> {
    for entry in &manifest.sessions {
        if entry.tool != Tool::Claude || !sessions.contains(&entry.id) {
            continue;
        }
        let file = claude_json_of(home, entry.transcripts.first());
        let text = std::fs::read_to_string(&file).unwrap_or_default();
        if !claude_trusts(&text, Path::new(&entry.cwd)) {
            return Err(format!(
                "{} isn't trusted by Claude Code on {node}",
                entry.cwd
            ));
        }
    }
    Ok(())
}

pub struct RealRestorer {
    pub cluster: PathBuf,
    pub home: PathBuf,
    pub git: Arc<dyn notchling_engine::control::Launcher>,
    pub read: Arc<dyn notchling_engine::control::Launcher>,
}

impl Restorer for RealRestorer {
    fn check(
        &self,
        primary: &NodeId,
        node: &str,
        set: Option<&str>,
        sessions: &BTreeSet<String>,
    ) -> Result<(), String> {
        let manifest = crate::snap_restore::chosen_set(&self.cluster, primary, set, node)?;
        trust_check(&self.home, &manifest, sessions, node)
    }

    fn restore(
        &self,
        primary: &NodeId,
        node: &str,
        sessions: &BTreeSet<String>,
        set: Option<&str>,
        stage: &Path,
    ) -> Result<(), String> {
        let hooks = crate::snap_restore::DbHooks {
            close: &|| Ok(()),
            reopen: &|| Ok(()),
        };
        let login_path = crate::snap_restore::login_path(
            self.git.as_ref(),
            std::env::var("SHELL").ok().as_deref(),
        );
        crate::snap_restore::restore(&crate::snap_restore::Request {
            cluster: &self.cluster,
            primary,
            home: &self.home,
            node,
            sessions,
            aside: false,
            shared: true,
            git: self.git.as_ref(),
            read: self.read.as_ref(),
            login_path: &login_path,
            db: &hooks,
            set,
            stage: Some(stage),
        })
        .map(|_| ())
    }
}

pub struct Mover {
    pub(crate) p: MoveParts,
    pub(crate) switching: Mutex<Option<u64>>,
    pub(crate) resuming: Mutex<bool>,
    pub(crate) resumed: std::sync::Condvar,
}

fn text_of<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value.get(key).and_then(Value::as_str)
}

fn versions_of(member: Option<&Value>) -> Option<Versions> {
    member
        .and_then(|m| m.get("versions"))
        .and_then(|v| serde_json::from_value(v.clone()).ok())
}

fn member_of<'a>(status: &'a Value, id: &str) -> Option<&'a Value> {
    status["members"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|m| text_of(m, "id") == Some(id))
}

pub fn promote_compat(status: &Value, to: &NodeId, name: &str) -> Compat {
    let reference = status["members"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|m| m["reference"].as_bool() == Some(true) && text_of(m, "state") == Some("active"))
        .or_else(|| text_of(status, "me").and_then(|me| member_of(status, me)));
    let Some(mine) = versions_of(reference) else {
        return Compat::Refuse("this machine's versions aren't known yet".to_string());
    };
    let Some(theirs) = versions_of(member_of(status, to.as_str())) else {
        return Compat::Refuse(format!("{name} hasn't reported its versions"));
    };
    compat(&mine, &theirs, name, Purpose::Promote)
}

pub fn switched_to(status: &Value, to: &NodeId, term: u64) -> bool {
    status["cluster"]["home"].as_str() == Some(to.as_str())
        && status["cluster"]["term"]["n"].as_u64().unwrap_or(0) >= term
}

impl Mover {
    pub fn new(parts: MoveParts) -> Arc<Mover> {
        Arc::new(Mover {
            p: parts,
            switching: Mutex::new(None),
            resuming: Mutex::new(false),
            resumed: std::sync::Condvar::new(),
        })
    }

    pub fn gate(&self) -> &Arc<MoveGate> {
        &self.p.gate
    }

    pub fn now(&self) -> i64 {
        (self.p.clock)()
    }

    pub fn me(&self) -> Option<NodeId> {
        text_of(&self.p.link.status(), "me").and_then(NodeId::parse)
    }

    pub fn name_of(&self, id: &NodeId) -> String {
        let status = self.p.link.status();
        member_of(&status, id.as_str())
            .and_then(|m| text_of(m, "name"))
            .unwrap_or("that machine")
            .to_string()
    }

    pub fn facts(&self, to: &NodeId) -> MoveFacts {
        let status = self.p.link.status();
        let me = text_of(&status, "me").and_then(NodeId::parse);
        let home = status["cluster"]["home"].as_str().and_then(NodeId::parse);
        let term = status["cluster"]["term"]["n"].as_u64().unwrap_or(0);
        let member = member_of(&status, to.as_str()).cloned();
        let ready = (self.p.ready)(&status, to);
        let name = member
            .as_ref()
            .and_then(|m| text_of(m, "name"))
            .unwrap_or("that machine")
            .to_string();
        let to_facts = ToFacts {
            compat: promote_compat(&status, to, &name),
            name,
            member: member
                .as_ref()
                .is_some_and(|m| text_of(m, "state") == Some("active")),
            can_be_master: member
                .as_ref()
                .is_some_and(|m| m["canBeMaster"].as_bool() == Some(true)),
            online: member
                .as_ref()
                .is_some_and(|m| m["online"].as_bool() == Some(true)),
            ready,
        };
        let main: Vec<String> = status["main"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|id| id.as_str().map(str::to_string))
            .collect();
        let set = self.p.snaps.latest().map(|m| {
            m.sessions
                .into_iter()
                .map(|entry| (entry.id, entry.movable))
                .collect::<Vec<(String, Movable)>>()
        });
        MoveFacts {
            term,
            is_home: me.is_some() && me == home,
            to: to_facts,
            main,
            set,
        }
    }

    pub fn catch_up(&self, to: &NodeId) -> Result<(), String> {
        self.p.snaps.catch_up(to, &self.name_of(to))
    }

    pub fn freeze(&self, sessions: &[String], to_name: &str) {
        self.p.gate.freeze(sessions, to_name);
    }

    pub fn thaw(&self) {
        self.p.gate.thaw();
    }

    pub fn fence(&self, to_name: &str) {
        self.p.gate.fence(to_name);
        self.p.link.fence(Some(moving_text(to_name)));
    }

    pub fn unfence(&self) {
        self.p.gate.unfence();
        self.p.link.fence(None);
    }

    pub fn hold(&self, to: &NodeId, on: bool) {
        self.p.snaps.hold(to, on);
    }

    fn agents(&self) -> Vec<Agent> {
        self.p
            .rows
            .rows()
            .unwrap_or_default()
            .into_iter()
            .filter_map(|row| serde_json::from_value::<Agent>(row).ok())
            .collect()
    }

    pub fn working(&self, among: &[String]) -> Vec<String> {
        let mut busy: Vec<String> = self
            .agents()
            .into_iter()
            .filter(|agent| agent.turn_running() && among.contains(&agent.id))
            .map(|agent| agent.id)
            .collect();
        busy.sort();
        busy
    }

    pub fn live(&self, id: &str) -> bool {
        self.agents().into_iter().any(|agent| {
            agent.id == id
                && agent.node.is_none()
                && matches!(agent.target, AgentTarget::Job(_) | AgentTarget::Process(_))
        })
    }

    fn tool_of(&self, id: &str) -> Tool {
        self.p
            .snaps
            .latest()
            .and_then(|m| m.sessions.into_iter().find(|s| s.id == id))
            .map_or(Tool::Claude, |entry| entry.tool)
    }

    pub fn stop_one(&self, id: &str) -> Result<(), String> {
        match self.tool_of(id) {
            Tool::Claude => self.p.acts.stop_claude(id),
            _ => {
                self.p.acts.stop_codex(id);
                Ok(())
            }
        }
    }

    pub fn stop(&self, sessions: &[String]) -> StopResult {
        let mut result = StopResult::default();
        for id in sessions {
            match self.stop_one(id) {
                Ok(()) => result.done.push(id.clone()),
                Err(why) => {
                    result.failed = Some((id.clone(), why));
                    return result;
                }
            }
        }
        result
    }

    pub fn final_snapshot(&self, to: &NodeId, moving: &[String]) -> Result<FinalSet, String> {
        let at_ms = self.now();
        let manifest = self.p.snaps.final_set(to, &self.name_of(to))?;
        let entries: Vec<(String, Movable)> = manifest
            .sessions
            .into_iter()
            .map(|entry| (entry.id, entry.movable))
            .collect();
        Ok(FinalSet {
            set: manifest.set,
            left: left_behind(moving, &entries),
            at_ms,
        })
    }

    fn fence_moved(
        &self,
        me: Option<NodeId>,
        to: &NodeId,
        term: u64,
        keys: &[String],
    ) -> Result<(), String> {
        let me = me
            .or_else(|| self.me())
            .ok_or_else(|| "this machine's cluster node has no id".to_string())?;
        let at = Term {
            n: term,
            node: to.clone(),
        };
        let name = self.name_of(to);
        let status = self.p.link.status();
        let cluster = status["cluster"]["id"].as_str();
        let mut last = String::new();
        for attempt in 0..FENCE_TRIES {
            if attempt > 0 {
                std::thread::sleep(FENCE_PAUSE);
            }
            match mark_moved_in(&self.p.cluster, &me, at.clone(), keys, &name, cluster) {
                Ok(()) => return Ok(()),
                Err(why) => last = why,
            }
        }
        Err(last)
    }

    fn lift(&self, main: &BTreeMap<String, MainSession>, id: &str) {
        let keys = fence_keys(main, &[id.to_string()]);
        if let Err(why) = clear_moved(&self.p.cluster, &keys) {
            self.notice(
                &format!("cluster-move-lift-{id}"),
                Kind::ActionFailed,
                &format!("Couldn't lift the pause on {id} on this machine"),
                &why,
            );
        }
    }

    pub fn switch(
        &self,
        to: &NodeId,
        sessions: &[String],
        set: Option<&str>,
        term: u64,
    ) -> SwitchResult {
        let me = self.me();
        let keys = fence_keys(&self.mains(), sessions);
        let result = self.switch_link(to, sessions, set, term);
        if !matches!(result, SwitchResult::Done) {
            return result;
        }
        match self.fence_moved(me, to, term, &keys) {
            Ok(()) => SwitchResult::Done,
            Err(why) => {
                let (title, detail) = unfenced_notice(sessions, &why);
                self.notice("cluster-move-fence", Kind::ActionFailed, &title, &detail);
                SwitchResult::DoneUnfenced(why)
            }
        }
    }

    fn switch_link(
        &self,
        to: &NodeId,
        sessions: &[String],
        set: Option<&str>,
        term: u64,
    ) -> SwitchResult {
        let status = self.p.link.status();
        if status["cluster"].is_null() {
            return SwitchResult::Lost("this machine's cluster node isn't up yet".to_string());
        }
        if switched_to(&status, to, term) {
            return SwitchResult::Done;
        }
        match self.p.link.switch(to, sessions, set) {
            Ok(_) => SwitchResult::Done,
            Err(_) if switched_to(&self.p.link.status(), to, term) => SwitchResult::Done,
            Err(e) => match e.code.as_str() {
                "refused" => SwitchResult::Refused(e.message),
                "not_sent" => SwitchResult::NotSent(e.message),
                _ => SwitchResult::Lost(e.message),
            },
        }
    }

    pub fn resume(&self, to: &NodeId, sessions: &[String]) -> Result<Started, String> {
        let reached = self
            .p
            .link
            .call(
                to,
                MOVE_RESUME,
                json!({"sessions": sessions}),
                resume_wait(sessions.len()),
            )
            .map_err(|e| e.message)?;
        let found = reached["results"].as_array().cloned().unwrap_or_default();
        Ok(sessions
            .iter()
            .map(|id| {
                let row = found.iter().find(|r| text_of(r, "id") == Some(id.as_str()));
                let result = match row {
                    Some(r) if r["ok"].as_bool() == Some(true) => Ok(()),
                    Some(r) => Err(text_of(r, "error").unwrap_or("it didn't start").to_string()),
                    None => Err("that machine didn't answer for it".to_string()),
                };
                (id.clone(), result)
            })
            .collect())
    }

    fn mains(&self) -> BTreeMap<String, MainSession> {
        persist::load(&self.p.cluster)
            .ok()
            .flatten()
            .map(|s| s.main)
            .unwrap_or_default()
    }

    fn start_one(&self, main: &BTreeMap<String, MainSession>, id: &str) -> Result<(), String> {
        match main.get(id) {
            None => Err("this machine has no record of it".to_string()),
            Some(session) if session.tool != Tool::Claude => {
                self.lift(main, id);
                Ok(())
            }
            Some(_) if self.live(id) => {
                self.lift(main, id);
                Ok(())
            }
            Some(session) => {
                self.lift(main, id);
                self.p.acts.start_claude(session)
            }
        }
    }

    pub fn start_here(&self, sessions: &[String]) -> Started {
        let main = self.mains();
        sessions
            .iter()
            .map(|id| (id.clone(), self.start_one(&main, id)))
            .collect()
    }

    pub fn restart(&self, sessions: &[String], unless_live: &[String]) -> Started {
        let main = self.mains();
        let mut results: Started = sessions
            .iter()
            .map(|id| {
                let result = match main.get(id) {
                    None => Err("this machine has no record of it".to_string()),
                    Some(session) if session.tool != Tool::Claude => {
                        self.lift(&main, id);
                        Ok(())
                    }
                    Some(session) => {
                        self.lift(&main, id);
                        self.p.acts.start_claude(session)
                    }
                };
                (id.clone(), result)
            })
            .collect();
        for id in unless_live {
            if !sessions.contains(id) {
                results.push((id.clone(), self.start_one(&main, id)));
            }
        }
        results
    }

    pub fn stragglers(&self, since_ms: i64) -> Vec<String> {
        self.p
            .inbox
            .items()
            .into_iter()
            .filter(|item| {
                item.state == ItemState::Open
                    && item.created_ms >= since_ms
                    && !item.id.starts_with("cluster-move")
            })
            .map(|item| item.title)
            .collect()
    }

    pub fn notice(&self, id: &str, kind: Kind, title: &str, detail: &str) {
        raise_notice(&self.p.inbox, self.now(), id, (kind, title, detail));
    }
}

pub fn raise_notice(
    inbox: &InboxOps,
    now: i64,
    id: &str,
    (kind, title, detail): (Kind, &str, &str),
) {
    inbox.raise(
        Item {
            id: id.to_string(),
            kind,
            title: title.to_string(),
            detail: detail.to_string(),
            source: Source::default(),
            created_ms: now,
            expires_ms: None,
            actions: Vec::new(),
            state: ItemState::Open,
            answer: None,
            answered_by: None,
            seen: false,
            open: None,
            closed_ms: None,
        },
        None,
    );
}
