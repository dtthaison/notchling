use crate::cluster_node::ClusterNode;
use crate::cluster_sessions::LocalRows;
use crate::inbox_ops::InboxOps;
use notchling_engine::cluster::core::Takeover;
use notchling_engine::cluster::main_sessions::MainSession;
use notchling_engine::cluster::move_home::{
    left_behind, MoveFacts, StopResult, SwitchResult, ToFacts,
};
use notchling_engine::cluster::move_home::{
    MOVE_FREEZE, MOVE_RESUME, MOVE_STOP, MOVE_SWITCH, MOVE_UNDO,
};
use notchling_engine::cluster::persist;
use notchling_engine::cluster::snapshot::{Manifest, Movable};
use notchling_engine::cluster::versions::Compat;
use notchling_engine::cluster::wire::{Origin, OriginTier, WireError};
use notchling_engine::cluster::{LocalCmd, NodeId, Tool};
use notchling_engine::control::session::{conversation_of, resume_plan_step};
use notchling_engine::control::{execute, Outcome as PlanOutcome, Plan, Step};
use notchling_engine::inbox::{Item, Kind, Source, State as ItemState};
use notchling_engine::sessions::agent::Agent;
use notchling_engine::sessions::lanes::Roots;
use notchling_local_api::{ApiError, ErrorCode};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

pub const MOVE_OPS: [&str; 5] = [MOVE_FREEZE, MOVE_STOP, MOVE_SWITCH, MOVE_RESUME, MOVE_UNDO];
pub const SNAP_WAIT: Duration = Duration::from_secs(5 * 60);
pub const SWITCH_WAIT: Duration = Duration::from_secs(5 * 60 + 5);
pub const RESUME_WAIT: Duration = Duration::from_secs(2 * 60);

pub(crate) fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

pub fn failed(why: impl Into<String>) -> ApiError {
    ApiError::new(ErrorCode::Failed, why)
}

#[derive(Default)]
pub struct MoveGate {
    held: Mutex<Option<(BTreeSet<String>, String)>>,
}

impl MoveGate {
    pub fn freeze(&self, sessions: &[String], to_name: &str) {
        *lock(&self.held) = Some((sessions.iter().cloned().collect(), to_name.to_string()));
    }

    pub fn thaw(&self) {
        *lock(&self.held) = None;
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
    fn switch(&self, to: &NodeId, sessions: &[String]) -> Result<Value, WireError>;
    fn call(
        &self,
        to: &NodeId,
        op: &str,
        args: Value,
        within: Duration,
    ) -> Result<Value, WireError>;
    fn take_primary(&self, takeover: Takeover) -> Result<Value, WireError>;
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

    fn switch(&self, to: &NodeId, sessions: &[String]) -> Result<Value, WireError> {
        self.node()?.local_wire(
            LocalCmd::MoveSwitch {
                to: to.clone(),
                sessions: sessions.to_vec(),
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
                args,
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
}

pub trait Acts: Send + Sync {
    fn stop_claude(&self, id: &str) -> Result<(), String>;
    fn stop_codex(&self, id: &str);
    fn start_claude(&self, session: &MainSession) -> Result<(), String>;

    fn primary_moved(&self, _to: &NodeId) {}
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
}

pub trait Restorer: Send + Sync {
    fn restore(
        &self,
        primary: &NodeId,
        node: &str,
        sessions: &BTreeSet<String>,
        hooks: &crate::snap_restore::DbHooks,
    ) -> Result<(), String>;
}

pub struct RealRestorer {
    pub cluster: PathBuf,
    pub home: PathBuf,
    pub git: Arc<dyn notchling_engine::control::Launcher>,
    pub read: Arc<dyn notchling_engine::control::Launcher>,
}

impl Restorer for RealRestorer {
    fn restore(
        &self,
        primary: &NodeId,
        node: &str,
        sessions: &BTreeSet<String>,
        hooks: &crate::snap_restore::DbHooks,
    ) -> Result<(), String> {
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
            db: hooks,
        })
        .map(|_| ())
    }
}

pub struct Mover {
    pub(crate) p: MoveParts,
}

fn text_of<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value.get(key).and_then(Value::as_str)
}

impl Mover {
    pub fn new(parts: MoveParts) -> Arc<Mover> {
        Arc::new(Mover { p: parts })
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
        status["members"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|m| text_of(m, "id") == Some(id.as_str()))
            .and_then(|m| text_of(m, "name"))
            .unwrap_or("that machine")
            .to_string()
    }

    pub fn facts(&self, to: &NodeId) -> MoveFacts {
        let status = self.p.link.status();
        let me = text_of(&status, "me").and_then(NodeId::parse);
        let home = status["cluster"]["home"].as_str().and_then(NodeId::parse);
        let term = status["cluster"]["term"]["n"].as_u64().unwrap_or(0);
        let member = status["members"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|m| text_of(m, "id") == Some(to.as_str()))
            .cloned();
        let ready = (self.p.ready)(&status, to);
        let to_facts = ToFacts {
            name: member
                .as_ref()
                .and_then(|m| text_of(m, "name"))
                .unwrap_or("that machine")
                .to_string(),
            member: member
                .as_ref()
                .is_some_and(|m| text_of(m, "state") == Some("active")),
            can_be_master: member
                .as_ref()
                .is_some_and(|m| m["canBeMaster"].as_bool() == Some(true)),
            online: member
                .as_ref()
                .is_some_and(|m| m["online"].as_bool() == Some(true)),
            compat: Compat::Ok,
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

    pub fn working(&self, among: &[String]) -> Vec<String> {
        let rows = self.p.rows.rows().unwrap_or_default();
        let mut busy: Vec<String> = rows
            .into_iter()
            .filter_map(|row| serde_json::from_value::<Agent>(row).ok())
            .filter(|agent| agent.turn_running() && among.contains(&agent.id))
            .map(|agent| agent.id)
            .collect();
        busy.sort();
        busy
    }

    fn tool_of(&self, id: &str) -> Tool {
        self.p
            .snaps
            .latest()
            .and_then(|m| m.sessions.into_iter().find(|s| s.id == id))
            .map_or(Tool::Claude, |entry| entry.tool)
    }

    pub fn stop(&self, sessions: &[String]) -> StopResult {
        let mut result = StopResult::default();
        for id in sessions {
            match self.tool_of(id) {
                Tool::Claude => match self.p.acts.stop_claude(id) {
                    Ok(()) => result.done.push(id.clone()),
                    Err(why) => {
                        result.failed = Some((id.clone(), why));
                        return result;
                    }
                },
                _ => {
                    self.p.acts.stop_codex(id);
                    result.done.push(id.clone());
                }
            }
        }
        result
    }

    pub fn final_snapshot(
        &self,
        to: &NodeId,
        moving: &[String],
    ) -> Result<Vec<(String, String)>, String> {
        let manifest = self.p.snaps.final_set(to, &self.name_of(to))?;
        let entries: Vec<(String, Movable)> = manifest
            .sessions
            .into_iter()
            .map(|entry| (entry.id, entry.movable))
            .collect();
        Ok(left_behind(moving, &entries))
    }

    pub fn switch(&self, to: &NodeId, sessions: &[String]) -> SwitchResult {
        let status = self.p.link.status();
        if status["cluster"]["home"].as_str() == Some(to.as_str()) {
            return SwitchResult::Done;
        }
        match self.p.link.switch(to, sessions) {
            Ok(_) => SwitchResult::Done,
            Err(e) if matches!(e.code.as_str(), "not_sent" | "refused" | "stale_term") => {
                SwitchResult::Refused(e.message)
            }
            Err(e) => SwitchResult::Lost(e.message),
        }
    }

    pub fn resume(&self, to: &NodeId, sessions: &[String]) -> Vec<(String, Result<(), String>)> {
        let answer = self
            .p
            .link
            .call(to, MOVE_RESUME, json!({"sessions": sessions}), RESUME_WAIT);
        let reached = match answer {
            Ok(value) => value,
            Err(e) => {
                return sessions
                    .iter()
                    .map(|id| (id.clone(), Err(e.message.clone())))
                    .collect()
            }
        };
        let found = reached["results"].as_array().cloned().unwrap_or_default();
        sessions
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
            .collect()
    }

    pub fn start_here(&self, sessions: &[String]) -> Vec<(String, Result<(), String>)> {
        let state = persist::load(&self.p.cluster).ok().flatten();
        let main: BTreeMap<String, MainSession> = state.map(|s| s.main).unwrap_or_default();
        sessions
            .iter()
            .map(|id| {
                let result = match main.get(id) {
                    None => Err("this machine has no record of it".to_string()),
                    Some(session) if session.tool != Tool::Claude => Ok(()),
                    Some(session) => self.p.acts.start_claude(session),
                };
                (id.clone(), result)
            })
            .collect()
    }

    pub fn restart(&self, sessions: &[String]) -> Vec<(String, Result<(), String>)> {
        self.start_here(sessions)
    }

    pub fn notice(&self, id: &str, kind: Kind, title: &str, detail: &str) {
        self.p.inbox.raise(
            Item {
                id: id.to_string(),
                kind,
                title: title.to_string(),
                detail: detail.to_string(),
                source: Source::default(),
                created_ms: self.now(),
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
}
