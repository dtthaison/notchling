use crate::acting_ops::{bad, forbidden, now_ms, text_arg, ActingOps};
use crate::agy_ops::AgyOps;
use crate::late_emit::Events;
use crate::layout::Layout;
use crate::op_group::OpGroup;
use crate::outbox_runners::Delivery;
use crate::switches::SwitchFlag;
use crate::vault_caller::{Processes, SystemProcesses};
use notchling_engine::contract::channels::SIDECAR;
use notchling_engine::tools::agy_sidecar::bundle::is_antigravity_app_executable;
use notchling_engine::tools::agy_sidecar::config_file::{self, EnableError, Switch};
use notchling_engine::tools::agy_sidecar::deliver::{PICKUP_BUDGET, RUN_BUDGET};
use notchling_engine::tools::agy_sidecar::hook_io::{
    parse_conversation, parse_tool, prompt_event, stop_output, tool_output,
};
use notchling_engine::tools::agy_sidecar::hooks_file::{self, Found, Want};
use notchling_engine::tools::agy_sidecar::ids::is_uuid;
use notchling_engine::tools::agy_sidecar::jobs::{
    Cancelled, Job, Kind, Outcome, Patience, Queue, Stops,
};
use notchling_engine::tools::agy_sidecar::plugin::{self, Plugin};
use notchling_engine::tools::agy_sidecar::{
    Roots, ASK_WAIT_SECS, PRE_INVOCATION, PRE_TOOL, SIDECAR_ID,
};
use notchling_events::kinds::choices::DoneOrFailed;
use notchling_events::kinds::settings::{HookTarget, InstallAction, InstallHook};
use notchling_events::spec::Outcome as EventOutcome;
use notchling_events::{scope, Event};
use notchling_local_api::{ops, ApiError, Caller, ClientKind, ErrorCode, OpSpec};
use notchling_platform::paths::Os;
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub const OPS: [&str; 5] = [
    ops::AGY_SIDECAR_TAKE,
    ops::AGY_SIDECAR_REPORT,
    ops::AGY_SIDECAR_STATUS,
    ops::AGY_SIDECAR_SETUP,
    ops::AGY_HOOK,
];

pub const ALIVE_MS: i64 = 10_000;
pub const TAKE_WAIT: Duration = Duration::from_secs(25);
pub const PATIENCE: Patience = Patience {
    pickup: PICKUP_BUDGET,
    run: RUN_BUDGET,
};
pub const LINEAGE_DEPTH: usize = 10;

pub const NOT_RUNNING: &str =
    "The Notchling sidecar isn't running in Antigravity, so the message was kept here. Turn it on in Settings, then send it again.";
pub const NOT_PICKED_UP: &str =
    "The Notchling sidecar didn't pick the message up in time, so it was kept here. Send it again.";
pub const STILL_STARTING: &str =
    "The sidecar took the message but hasn't said it was delivered. Look in Antigravity before you send it again.";
pub const MAYBE_STARTED: &str =
    "The sidecar couldn't tell whether Antigravity started the conversation. Look in Antigravity before you send it again.";

pub struct SidecarLink {
    queue: Queue,
    stops: Stops,
    clock: Box<dyn Fn() -> i64 + Send + Sync>,
    patience: Patience,
    pid_alive: Box<dyn Fn(u32) -> bool + Send + Sync>,
}

impl SidecarLink {
    pub fn new() -> Arc<SidecarLink> {
        SidecarLink::with(
            Box::new(now_ms),
            PATIENCE,
            Box::new(|pid| SystemProcesses.exe(pid).is_some()),
        )
    }

    pub fn with(
        clock: Box<dyn Fn() -> i64 + Send + Sync>,
        patience: Patience,
        pid_alive: Box<dyn Fn(u32) -> bool + Send + Sync>,
    ) -> Arc<SidecarLink> {
        Arc::new(SidecarLink {
            queue: Queue::new(),
            stops: Stops::new(),
            clock,
            patience,
            pid_alive,
        })
    }

    pub fn saw(&self, pid: u32) {
        self.queue.seen_by((self.clock)(), pid);
    }

    pub fn now(&self) -> i64 {
        (self.clock)()
    }

    pub fn queue(&self) -> &Queue {
        &self.queue
    }

    pub fn stops(&self) -> &Stops {
        &self.stops
    }

    pub fn running(&self) -> bool {
        self.queue.alive((self.clock)(), ALIVE_MS)
            && self
                .queue
                .last_pid()
                .is_none_or(|pid| (self.pid_alive)(pid))
    }

    fn still_up(&self) -> bool {
        self.running()
    }

    fn unpicked(&self) -> String {
        if self.running() {
            NOT_PICKED_UP.into()
        } else {
            NOT_RUNNING.into()
        }
    }

    pub fn close(&self) {
        self.queue.close();
        self.stops.clear();
    }

    pub(crate) fn reply(&self, job: &str, session: &str, text: &str) -> Result<(), Delivery> {
        if !is_uuid(session) {
            return Err(Delivery::Failed(
                "That is not an Antigravity conversation id.".into(),
            ));
        }
        if !self.running() {
            return Err(Delivery::Failed(NOT_RUNNING.into()));
        }
        let job = Job {
            id: job.to_string(),
            kind: Kind::Reply {
                session: session.to_string(),
            },
            text: text.to_string(),
        };
        let id = job.id.clone();
        self.stops.forget(session);
        self.queue.submit(job).map_err(Delivery::Failed)?;
        match self
            .queue
            .wait_patient(&id, self.patience, &|| self.still_up())
        {
            Some(Outcome::Delivered { .. }) => Ok(()),
            Some(Outcome::Unconfirmed { .. }) => Err(Delivery::Cut),
            Some(Outcome::Failed(why)) => Err(Delivery::Failed(why)),
            None => match self.queue.cancel(&id) {
                Cancelled::Withdrawn => Err(Delivery::Failed(self.unpicked())),
                Cancelled::Taken | Cancelled::Gone => Err(Delivery::Cut),
            },
        }
    }

    pub fn start(
        &self,
        job: &str,
        text: &str,
        model: Option<&str>,
        title: Option<&str>,
    ) -> Result<String, ApiError> {
        let failed = |m: &str| ApiError::new(ErrorCode::Failed, m);
        if !self.running() {
            return Err(failed(NOT_RUNNING));
        }
        let job = Job {
            id: job.to_string(),
            kind: Kind::Start {
                model: model.map(String::from),
                title: title.map(String::from),
            },
            text: text.to_string(),
        };
        let id = job.id.clone();
        self.queue.submit(job).map_err(|why| failed(&why))?;
        match self
            .queue
            .wait_patient(&id, self.patience, &|| self.still_up())
        {
            Some(Outcome::Delivered {
                session: Some(session),
            })
            | Some(Outcome::Unconfirmed {
                session: Some(session),
            }) => {
                self.stops.forget(&session);
                Ok(session)
            }
            Some(Outcome::Delivered { session: None })
            | Some(Outcome::Unconfirmed { session: None }) => Err(failed(MAYBE_STARTED)),
            Some(Outcome::Failed(why)) => Err(failed(&why)),
            None => match self.queue.cancel(&id) {
                Cancelled::Withdrawn => Err(failed(&self.unpicked())),
                Cancelled::Taken | Cancelled::Gone => Err(failed(STILL_STARTING)),
            },
        }
    }

    pub fn stop(&self, conversation: &str) -> Result<(), ApiError> {
        if !is_uuid(conversation) {
            return Err(bad("That is not an Antigravity conversation id."));
        }
        self.stops.ask(conversation);
        Ok(())
    }
}

pub struct AgySidecarOps {
    link: Arc<SidecarLink>,
    agy: Arc<AgyOps>,
    acting: Arc<ActingOps>,
    install: Arc<SwitchFlag>,
    roots: Roots,
    binary: Option<PathBuf>,
    processes: Arc<dyn Processes>,
    clock: Box<dyn Fn() -> u64 + Send + Sync>,
    events: Option<Events>,
    writing: Mutex<()>,
}

fn system_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn failed(message: impl Into<String>) -> ApiError {
    ApiError::new(ErrorCode::Failed, message)
}

pub fn from_antigravity(processes: &dyn Processes, pid: u32) -> bool {
    let mut next = processes.parent(pid);
    for _ in 0..LINEAGE_DEPTH {
        let Some(p) = next.filter(|p| *p > 1) else {
            return false;
        };
        if processes
            .exe(p)
            .is_some_and(|path| is_antigravity_app_executable(&path))
        {
            return true;
        }
        next = processes.parent(p);
    }
    false
}

impl AgySidecarOps {
    pub fn new(
        link: Arc<SidecarLink>,
        agy: Arc<AgyOps>,
        acting: Arc<ActingOps>,
        install: Arc<SwitchFlag>,
        roots: Roots,
        binary: Option<PathBuf>,
        processes: Arc<dyn Processes>,
    ) -> AgySidecarOps {
        AgySidecarOps {
            link,
            agy,
            acting,
            install,
            roots,
            binary,
            processes,
            clock: Box::new(system_ms),
            events: None,
            writing: Mutex::new(()),
        }
    }

    pub fn for_daemon(
        layout: &Layout,
        agy: &Arc<AgyOps>,
        acting: &Arc<ActingOps>,
        install: Arc<SwitchFlag>,
        binary: Option<PathBuf>,
        events: Events,
    ) -> Arc<AgySidecarOps> {
        let link = SidecarLink::new();
        agy.attach_sidecar(link.clone());
        Arc::new(
            AgySidecarOps::new(
                link,
                agy.clone(),
                acting.clone(),
                install,
                Roots::under(&layout.home()),
                binary,
                Arc::new(SystemProcesses),
            )
            .with_events(events),
        )
    }

    pub fn with_events(mut self, events: Events) -> AgySidecarOps {
        self.events = Some(events);
        self
    }

    pub fn with_clock(mut self, clock: Box<dyn Fn() -> u64 + Send + Sync>) -> AgySidecarOps {
        self.clock = clock;
        self
    }

    pub fn spec(&self, op: &str) -> Option<OpSpec> {
        match op {
            ops::AGY_SIDECAR_STATUS => Some(OpSpec::APP),
            ops::AGY_SIDECAR_SETUP => Some(OpSpec::APP),
            ops::AGY_SIDECAR_TAKE | ops::AGY_SIDECAR_REPORT | ops::AGY_HOOK => Some(OpSpec::AGENT_HOOK),
            _ => None,
        }
    }

    pub fn call(&self, caller: &Caller, op: &str, args: &Value) -> Result<Value, ApiError> {
        match op {
            ops::AGY_SIDECAR_TAKE => self.take(caller, args),
            ops::AGY_SIDECAR_REPORT => self.report(caller, args),
            ops::AGY_HOOK => self.hook(caller, args),
            ops::AGY_SIDECAR_STATUS => Ok(self.status()),
            ops::AGY_SIDECAR_SETUP => self.setup(args),
            _ => Err(ApiError::internal(format!("{op} has no handler"))),
        }
    }

    fn sidecar_caller(&self, caller: &Caller, what: &str) -> Result<u32, ApiError> {
        if caller.client != ClientKind::Hook {
            return Err(forbidden(&format!(
                "{what} is only for `notchling agy-sidecar` and `notchling agy-hook`"
            )));
        }
        caller
            .pid
            .filter(|pid| from_antigravity(&*self.processes, *pid))
            .ok_or_else(|| {
                forbidden(&format!(
                    "{what} comes only from a process Antigravity started"
                ))
            })
    }

    fn take(&self, caller: &Caller, args: &Value) -> Result<Value, ApiError> {
        let pid = self.sidecar_caller(caller, "agy.sidecar.take")?;
        let wait = args
            .get("waitMs")
            .and_then(Value::as_u64)
            .map(Duration::from_millis)
            .map_or(TAKE_WAIT, |w| w.min(TAKE_WAIT));
        let max = args
            .get("max")
            .and_then(Value::as_u64)
            .map_or(usize::MAX, |m| {
                usize::try_from(m).unwrap_or(usize::MAX).max(1)
            });
        let processes = self.processes.clone();
        let up = move || processes.exe(pid).is_some();
        let now = || self.link.now();
        let jobs: Vec<Value> = self
            .link
            .queue()
            .take_noting(wait, max, &up, Some((&now, pid)))
            .iter()
            .map(Job::to_json)
            .collect();
        Ok(json!({"jobs": jobs}))
    }

    fn report(&self, caller: &Caller, args: &Value) -> Result<Value, ApiError> {
        let pid = self.sidecar_caller(caller, "agy.sidecar.report")?;
        self.link.saw(pid);
        let id = text_arg(args, "id")?;
        let outcome = Outcome::from_json(args)
            .ok_or_else(|| bad("result is delivered, unconfirmed or failed"))?;
        if let Outcome::Delivered {
            session: Some(session),
        }
        | Outcome::Unconfirmed {
            session: Some(session),
        } = &outcome
        {
            if !is_uuid(session) {
                return Err(bad("session is an Antigravity conversation id"));
            }
        }
        Ok(json!({"accepted": self.link.queue().report(id, outcome)}))
    }

    fn hook(&self, caller: &Caller, args: &Value) -> Result<Value, ApiError> {
        let pid = self.sidecar_caller(caller, "agy.hook")?;
        let nothing = Ok(json!({"output": null}));
        if !self.chosen() {
            return nothing;
        }
        let which = text_arg(args, "event")?;
        let input = text_arg(args, "input")?;
        match which {
            PRE_INVOCATION => {
                let Some(conversation) = parse_conversation(input.as_bytes()) else {
                    return nothing;
                };
                if self.link.stops().take(&conversation) {
                    return Ok(json!({"output": stop_output()}));
                }
                nothing
            }
            PRE_TOOL => {
                let Some(ask) = parse_tool(input.as_bytes()) else {
                    return nothing;
                };
                let wait = args
                    .get("waitMs")
                    .and_then(Value::as_u64)
                    .map(Duration::from_millis)
                    .map_or(Duration::from_secs(ASK_WAIT_SECS), |w| {
                        w.min(Duration::from_secs(ASK_WAIT_SECS))
                    });
                let row = format!("agy:{}", ask.conversation);
                let decision = self.acting.ask_user(prompt_event(&ask, &row), pid, wait);
                let answer = match decision.get("action").and_then(Value::as_str) {
                    Some("allow") => "allow",
                    Some("deny") => "deny",
                    _ => "",
                };
                Ok(json!({"output": tool_output(answer)}))
            }
            _ => Err(bad("event is pre-tool or pre-invocation")),
        }
    }

    fn chosen(&self) -> bool {
        self.agy.channel().id() == SIDECAR
    }

    pub fn status(&self) -> Value {
        let config = config_file::switch(&self.roots.config_json(), SIDECAR_ID);
        let plugin_state = plugin::state(&self.roots);
        let current = self
            .binary
            .as_deref()
            .is_some_and(|b| plugin::installed_for(&self.roots, b));
        let hooks = hooks_file::found(&self.roots.hooks_json());
        let app = self.roots.installed();
        let enabled = matches!(config, Ok(Switch::On));
        let any = plugin_state == Plugin::Ours || hooks == Found::Installed || enabled;
        let state = if !app {
            "no_app"
        } else if plugin_state == Plugin::Foreign || config.is_err() || hooks == Found::Unreadable {
            "blocked"
        } else if enabled && current && hooks == Found::Installed {
            "on"
        } else if any {
            "partial"
        } else {
            "off"
        };
        json!({
            "state": state,
            "app": app,
            "chosen": self.chosen(),
            "enabled": match config {
                Ok(Switch::On) => "on",
                Ok(Switch::Off) => "off",
                Ok(Switch::Unset) => "unset",
                Err(_) => "unreadable",
            },
            "plugin": match plugin_state {
                Plugin::Absent => "absent",
                Plugin::Ours => "ours",
                Plugin::Foreign => "foreign",
            },
            "pluginCurrent": current,
            "hooks": match hooks {
                Found::Missing => "missing",
                Found::Installed => "installed",
                Found::Absent => "absent",
                Found::Unreadable => "unreadable",
            },
            "running": self.link.running(),
            "waiting": self.link.queue().waiting(),
        })
    }

    fn note(&self, action: InstallAction, result: DoneOrFailed) {
        let Some(events) = &self.events else {
            return;
        };
        events.push(Event::new(
            &InstallHook {
                target: HookTarget::Agy,
                action,
                result,
            },
            match result {
                DoneOrFailed::Done => EventOutcome::Ok,
                _ => EventOutcome::Failed,
            },
            now_ms(),
        ));
        scope::mark_recorded();
    }

    fn preflight(&self, binary: &std::path::Path) -> Result<(), String> {
        plugin::preflight(&self.roots, binary)?;
        config_file::check_enable(&self.roots.config_json(), SIDECAR_ID)
            .map_err(|e| e.to_string())?;
        hooks_file::check(
            &self.roots.hooks_json(),
            binary,
            Os::current(),
            Want::Install,
        )
        .map_err(|e| e.to_string())
    }

    fn setup(&self, args: &Value) -> Result<Value, ApiError> {
        let action = text_arg(args, "action")?;
        if !matches!(action, "turn_on" | "turn_off") {
            return Err(bad("action is turn_on or turn_off"));
        }
        let confirmed = args.get("confirm").and_then(Value::as_bool) == Some(true);
        if action == "turn_on" && !confirmed {
            return Err(bad(
                "turning the sidecar on writes Antigravity's config.json; pass confirm: true after asking the user",
            ));
        }
        self.install.check()?;
        let _one_at_a_time = self.writing.lock().unwrap_or_else(|e| e.into_inner());
        let stamp = (self.clock)();
        let binary = self
            .binary
            .as_deref()
            .ok_or_else(|| failed("can't find the path of this program, so no sidecar command"))?;
        let mut backups: Vec<String> = Vec::new();
        let mut keep = |b: Option<PathBuf>| {
            if let Some(b) = b {
                backups.push(b.to_string_lossy().into_owned());
            }
        };
        if action == "turn_on" {
            if !self.roots.installed() {
                return Err(failed("The Antigravity app is not installed."));
            }
            self.preflight(binary).map_err(|e| {
                self.note(InstallAction::Installed, DoneOrFailed::Failed);
                failed(e)
            })?;
            let blocked = |e: String| {
                self.note(InstallAction::Installed, DoneOrFailed::Failed);
                failed(e)
            };
            plugin::install(&self.roots, binary, stamp).map_err(blocked)?;
            let hooks = hooks_file::apply(
                &self.roots.hooks_json(),
                binary,
                Os::current(),
                Want::Install,
                stamp,
            )
            .map_err(|e| blocked(e.to_string()))?;
            keep(hooks.backup);
            let config = config_file::enable(&self.roots.config_json(), SIDECAR_ID, true, stamp)
                .map_err(|e| match e {
                    EnableError::NotConfirmed => bad(e.to_string()),
                    other => blocked(other.to_string()),
                })?;
            keep(config.backup);
            self.note(InstallAction::Installed, DoneOrFailed::Done);
        } else {
            let hooks = hooks_file::apply(
                &self.roots.hooks_json(),
                binary,
                Os::current(),
                Want::Remove,
                stamp,
            )
            .map_err(|e| {
                self.note(InstallAction::Removed, DoneOrFailed::Failed);
                failed(e.to_string())
            })?;
            keep(hooks.backup);
            let config = config_file::disable(&self.roots.config_json(), SIDECAR_ID, stamp)
                .map_err(|e| {
                    self.note(InstallAction::Removed, DoneOrFailed::Failed);
                    failed(e.to_string())
                })?;
            keep(config.backup);
            plugin::remove(&self.roots).map_err(|e| {
                self.note(InstallAction::Removed, DoneOrFailed::Failed);
                failed(e)
            })?;
            self.note(InstallAction::Removed, DoneOrFailed::Done);
        }
        let mut out = self.status();
        out["backups"] = json!(backups);
        Ok(out)
    }
}

impl OpGroup for AgySidecarOps {
    fn spec(&self, op: &str) -> Option<OpSpec> {
        AgySidecarOps::spec(self, op)
    }

    fn call(&self, caller: &Caller, op: &str, args: &Value) -> Result<Value, ApiError> {
        AgySidecarOps::call(self, caller, op, args)
    }
}

#[cfg(test)]
#[path = "agy_sidecar_ops_tests.rs"]
mod tests;
