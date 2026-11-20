use crate::account_history::DbSwitches;
use crate::accounts_ops::AccountsOps;
use crate::acting::{Acting, ACTING_OFF};
use crate::acting_ops::{ActingOps, Room};
use crate::activity_ops::ActivityOps;
use crate::agy_ops::AgyOps;
use crate::agy_sidecar_ops::AgySidecarOps;
use crate::ask_ops::AskOps;
use crate::attach::{ClaudeAttacher, LiveLauncher};
use crate::awake_loop::{power_for, AwakeLoop};
use crate::awake_ops::AwakeOps;
use crate::bridge_backend::APPROVAL_WAIT;
use crate::bridge_ops::{BridgeOps, BridgeSetup};
use crate::catalog_ops::CatalogService;
use crate::cluster_forward::{acting_gated, Forward};
use crate::cluster_prompts::RemotePrompts;
use crate::cluster_relay::HeldRelay;
use crate::cluster_relay_wiring::{devices_replica, parts as relay_parts, RelayEnv};
use crate::cluster_sessions::RemoteSessions;
use crate::cluster_wiring::ClusterParts;
use crate::codex_children::CodexChildren;
use crate::codex_ops::CodexOps;
use crate::daemon::{FirstScan, FIRST_LIST_WAIT};
use crate::every_session::{EverySession, CLAUDE};
use crate::git_ops::{listed_folders, live_folders, GitOps};
use crate::inbox_ops::InboxOps;
use crate::install_ops::{InstallOps, InstallSetup};
use crate::late_emit::{Events, LateEmit, LateSalt};
use crate::layout::Layout;
use crate::limits_ops::LimitsOps;
use crate::limits_sources::{ClaudeLimits, ProviderLimits, SessionJobs};
use crate::log_ops::LogOps;
use crate::models_ops::ModelsOps;
use crate::op_group::OpGroup;
use crate::organizations_ops::OrganizationsOps;
use crate::outbox_inbox::OutboxNotice;
use crate::outbox_ops::OutboxOps;
use crate::outbox_postman::Postmen;
use crate::pages_ops::{PagesOps, PagesSetup};
use crate::postmen_wiring::postmen_for;
use crate::providers_daemon::ProviderStores;
use crate::providers_ops::ProvidersOps;
use crate::settings_ops::{SettingsOps, SwitchStates};
use crate::shell_jobs::ShellJobs;
use crate::switches::{SwitchFlag, SwitchOps, Switches};
use crate::tailscale_ops::TailscaleOps;
use crate::vault_ops::VaultOps;
use crate::versions_ops::{AuditRecorder, VersionsOps};
use crate::wake_loop::{wakes_for, WakeLoop, Wakes};
use crate::{op_kinds, quiet_ops};
use notchling_engine::accounts::codex::login::idle_for_auto_switch;
use notchling_engine::accounts::desktop::ClaudeName;
use notchling_engine::activity::EventsDb;
use notchling_engine::awake::Power;
use notchling_engine::cluster::versions::BridgeFacts;
use notchling_engine::control::launcher::claude_path;
use notchling_engine::control::session::home_of;
use notchling_engine::control::RealLauncher;
use notchling_engine::git::SandboxedLauncher;
use notchling_engine::limits::renew::ClaudeRenewer;
use notchling_engine::outbox::OutboxStore;
use notchling_engine::versions::mac::Roots as VersionRoots;
use notchling_engine::versions::Version;
use notchling_events::{Emit, Salt};
use notchling_local_api::{
    ops, topics, ApiError, Caller, ClientKind, ErrorCode, Handler, OpHome, OpSpec, Tier,
};
use serde_json::{Map, Value};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

pub struct Ops {
    sessions: Arc<Mutex<Value>>,
    acting: Option<Arc<ActingOps>>,
    bridge: Option<Arc<BridgeOps>>,
    codex: Option<Arc<CodexOps>>,
    agy: Option<Arc<AgyOps>>,
    pages: Option<Arc<PagesOps>>,
    every: Option<Arc<EverySession>>,
    shells: Option<Arc<ShellJobs>>,
    groups: Vec<Arc<dyn OpGroup>>,
    forward: Option<Arc<dyn Forward>>,
    remote: Option<Arc<RemoteSessions>>,
    gate: Option<Arc<crate::cluster_move::MoveGate>>,
    prompts: Option<Arc<RemotePrompts>>,
    topic_list: Vec<&'static str>,
    first: Arc<FirstScan>,
    first_wait: Duration,
}

impl Ops {
    pub fn new(sessions: Arc<Mutex<Value>>) -> Ops {
        Ops {
            sessions,
            acting: None,
            bridge: None,
            codex: None,
            agy: None,
            pages: None,
            every: None,
            shells: None,
            groups: Vec::new(),
            forward: None,
            remote: None,
            gate: None,
            prompts: None,
            topic_list: vec![topics::SESSIONS],
            first: FirstScan::open(),
            first_wait: FIRST_LIST_WAIT,
        }
    }

    pub fn with_acting(sessions: Arc<Mutex<Value>>, acting: Arc<ActingOps>) -> Ops {
        Ops {
            acting: Some(acting),
            ..Ops::new(sessions)
        }
        .with_topics()
    }

    pub fn with_bridge(mut self, bridge: Arc<BridgeOps>) -> Ops {
        self.bridge = Some(bridge);
        self.with_topics()
    }

    pub fn with_codex(mut self, codex: Arc<CodexOps>) -> Ops {
        self.codex = Some(codex);
        self.with_topics()
    }

    fn with_topics(mut self) -> Ops {
        let mut list = vec![topics::SESSIONS];
        if self.acting.is_some() {
            list.extend([topics::PROMPTS, topics::SETTINGS]);
        }
        if self.bridge.is_some() {
            list.push(topics::BRIDGE);
        }
        if self.codex.is_some() || self.agy.is_some() {
            list.push(topics::TOOLS);
        }
        for topic in self.groups.iter().flat_map(|g| g.topics().iter().copied()) {
            if !list.contains(&topic) {
                list.push(topic);
            }
        }
        self.topic_list = list;
        self
    }

    pub fn with_group(mut self, group: Arc<dyn OpGroup>) -> Ops {
        self.groups.push(group);
        self.with_topics()
    }

    fn group(&self, op: &str) -> Option<&Arc<dyn OpGroup>> {
        self.groups.iter().find(|g| g.spec(op).is_some())
    }

    pub fn with_agy(mut self, agy: Arc<AgyOps>) -> Ops {
        self.agy = Some(agy);
        self.with_topics()
    }

    pub fn with_pages(mut self, pages: Arc<PagesOps>) -> Ops {
        self.pages = Some(pages);
        self
    }

    pub fn with_every(mut self, every: Arc<EverySession>) -> Ops {
        self.every = Some(every);
        self
    }

    pub fn with_shells(mut self, shells: Arc<ShellJobs>) -> Ops {
        self.shells = Some(shells);
        self
    }

    pub fn with_cluster(
        mut self,
        forward: Option<Arc<dyn Forward>>,
        remote: Arc<RemoteSessions>,
    ) -> Ops {
        self.forward = forward;
        self.remote = Some(remote);
        self
    }

    pub fn with_move_gate(mut self, gate: Arc<crate::cluster_move::MoveGate>) -> Ops {
        self.gate = Some(gate);
        self
    }

    pub fn with_remote_prompts(mut self, prompts: Arc<RemotePrompts>) -> Ops {
        self.prompts = Some(prompts);
        self
    }

    pub fn with_first_scan(mut self, first: Arc<FirstScan>, wait: Duration) -> Ops {
        self.first = first;
        self.first_wait = wait;
        self
    }
}

fn every_tool(args: &Value) -> Result<bool, ApiError> {
    match args.get("tools") {
        None | Some(Value::Null) => Ok(false),
        Some(Value::String(tools)) if tools == "claude" => Ok(false),
        Some(Value::String(tools)) if tools == "all" => Ok(true),
        Some(_) => Err(ApiError::bad_request(
            "sessions.list takes {\"tools\": \"all\" | \"claude\"}",
        )),
    }
}

fn with_remote_kinds(args: &Value) -> Result<(bool, bool), ApiError> {
    let usage = "sessions.list takes {\"remote\": \"shell\" | \"codex\" | [\"shell\", \"codex\"]}";
    let kinds: Vec<&str> = match args.get("remote") {
        None | Some(Value::Null) => return Ok((false, false)),
        Some(Value::String(kind)) => vec![kind.as_str()],
        Some(Value::Array(list)) => list
            .iter()
            .map(|k| k.as_str().ok_or_else(|| ApiError::bad_request(usage)))
            .collect::<Result<_, _>>()?,
        Some(_) => return Err(ApiError::bad_request(usage)),
    };
    if kinds.iter().any(|k| *k != "shell" && *k != "codex") {
        return Err(ApiError::bad_request(usage));
    }
    Ok((kinds.contains(&"shell"), kinds.contains(&"codex")))
}

fn tagged(list: Value, tool: &str) -> Vec<Value> {
    let Value::Array(rows) = list else {
        return Vec::new();
    };
    rows.into_iter()
        .map(|mut row| {
            row["tool"] = Value::String(tool.to_string());
            row
        })
        .collect()
}

impl Ops {
    fn sessions_list(&self, caller: &Caller, args: &Value) -> Result<(Value, bool), ApiError> {
        let every = every_tool(args)?;
        let (shell, codex) = with_remote_kinds(args)?;
        let landed = self.first.wait(self.first_wait);
        let list = self
            .sessions
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        let list = if every {
            let mut rows = tagged(list, CLAUDE);
            if let Some(other) = &self.every {
                rows.extend(other.values(now_ms()));
            }
            if let Some(shells) = &self.shells {
                rows.extend(shells.rows());
            }
            Value::Array(rows)
        } else {
            list
        };
        match &self.remote {
            Some(remote) if caller.client != ClientKind::Node && every => {
                Ok((remote.merge_all(&list), landed))
            }
            Some(remote) if caller.client != ClientKind::Node && (shell || codex) => {
                Ok((remote.merge_kinds(&list, shell, codex), landed))
            }
            Some(remote) if caller.client != ClientKind::Node => Ok((remote.merge(&list), landed)),
            _ => Ok((list, landed)),
        }
    }
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

impl Handler for Ops {
    fn call_with_extras(
        &self,
        caller: &Caller,
        op: &str,
        args: &Value,
    ) -> (Result<Value, ApiError>, Map<String, Value>) {
        if op != ops::SESSIONS_LIST {
            return (self.call(caller, op, args), Map::new());
        }
        let (list, landed) = match self.sessions_list(caller, args) {
            Ok(answer) => answer,
            Err(e) => return (Err(e), Map::new()),
        };
        let mut extras = Map::new();
        if !landed {
            extras.insert("partial".into(), Value::Bool(true));
        }
        (Ok(list), extras)
    }

    fn loud(&self, op: &str, args: &Value) -> bool {
        quiet_ops::loud(op, args)
    }

    fn session_of(&self, pid: u32) -> Option<String> {
        self.acting.as_ref().and_then(|a| a.session_of(pid))
    }

    fn home(&self, op: &str) -> OpHome {
        op_kinds::home(op)
    }

    fn known_ops(&self) -> &[&'static str] {
        op_kinds::known_ops()
    }

    fn spec(&self, op: &str) -> Option<OpSpec> {
        let spec = match op {
            ops::SESSIONS_LIST => Some(OpSpec::AGENT),
            op => self
                .pages
                .as_ref()
                .and_then(|p| p.spec(op))
                .or_else(|| self.acting.as_ref().and_then(|a| a.spec(op)))
                .or_else(|| self.bridge.as_ref().and_then(|b| b.spec(op)))
                .or_else(|| self.codex.as_ref().and_then(|c| c.spec(op)))
                .or_else(|| self.agy.as_ref().and_then(|a| a.spec(op)))
                .or_else(|| self.group(op).and_then(|g| g.spec(op))),
        };
        quiet_ops::quiet(spec, op)
    }

    fn call(&self, caller: &Caller, op: &str, args: &Value) -> Result<Value, ApiError> {
        if self.acting.as_ref().is_some_and(|a| !a.is_on()) && acting_gated(caller, op, args) {
            return Err(ApiError::new(ErrorCode::Forbidden, ACTING_OFF));
        }
        if let Some(gate) = &self.gate {
            let acting = self.acting.as_ref();
            gate.check(op, args, &|id| acting.and_then(|a| a.prompt_session(id)))?;
        }
        if let Some(routed) = self
            .forward
            .as_ref()
            .and_then(|forward| forward.forward(caller, op, args))
        {
            return routed;
        }
        if let Some(bridge) = self.bridge.as_ref().filter(|b| b.spec(op).is_some()) {
            return bridge.call(op, args);
        }
        if let Some(codex) = self.codex.as_ref().filter(|c| c.spec(op).is_some()) {
            if op == ops::CODEX_SEND
                && args.get("job").is_some_and(|j| !j.is_null())
                && caller.client != ClientKind::Node
            {
                return Err(ApiError::bad_request(
                    "job is set by the cluster, not by a caller",
                ));
            }
            return codex.call(op, args);
        }
        if let Some(agy) = self.agy.as_ref().filter(|a| a.spec(op).is_some()) {
            return agy.call(op, args);
        }
        if let Some(pages) = self.pages.as_ref().filter(|p| p.spec(op).is_some()) {
            return pages.call(op, args);
        }
        if let Some(group) = self.group(op) {
            return group.call(caller, op, args);
        }
        match (op, &self.acting) {
            (ops::SESSIONS_LIST, _) => self.sessions_list(caller, args).map(|(list, _)| list),
            (ops::PROMPTS_LIST, Some(acting)) => {
                let local = acting.call(caller, op, args)?;
                Ok(match &self.prompts {
                    Some(prompts) => prompts.merge(local, caller),
                    None => local,
                })
            }
            (op, Some(acting)) => acting.call(caller, op, args),
            (op, None) => Err(ApiError::internal(format!("{op} has no handler"))),
        }
    }

    fn topics(&self) -> &[&str] {
        &self.topic_list
    }

    fn topic_tier(&self, topic: &str) -> Tier {
        match topic {
            topics::PROMPTS | topics::BRIDGE => Tier::App,
            _ => self
                .groups
                .iter()
                .find_map(|g| g.topic_tier(topic))
                .unwrap_or(Tier::Agent),
        }
    }

    fn acting(&self) -> Option<bool> {
        self.acting.as_ref().map(|a| a.is_on())
    }
}

fn bridge_setup(layout: &Layout, ports: (u16, u16)) -> BridgeSetup {
    let beside = |dir: &std::path::Path| dir.parent().unwrap_or(dir).join("AgentConsole");
    let data_dir = beside(&layout.data_dir);
    let images_dir = match (layout.fixture, notchling_platform::paths::cache_dir()) {
        (false, Some(cache)) => beside(&cache).join("phone-images"),
        _ => data_dir.join("phone-images"),
    };
    BridgeSetup {
        data_dir,
        home: PathBuf::from(home_of(&layout.roots())),
        images_dir,
        projects_dir: layout.claude_dir.join("projects"),
        lan_port: ports.0,
        tunnel_port: ports.1,
        loopback_only: false,
        approval_wait: APPROVAL_WAIT,
    }
}

fn outbox_ops(
    layout: &Layout,
    sessions: &Arc<Mutex<Value>>,
    acting: &Arc<ActingOps>,
    room: &Arc<dyn Room>,
    events: &Events,
) -> OutboxOps {
    let (store, saving) = match OutboxStore::open(&layout.data_dir) {
        Ok(store) => (store, true),
        Err(_) => (OutboxStore::in_memory_for_tests(), false),
    };
    let ops = OutboxOps::new(
        Arc::new(store),
        sessions.clone(),
        layout.roots(),
        acting.acting_switch(),
        room.clone(),
        Some(events.clone()),
    );
    if saving {
        ops
    } else {
        ops.without_store()
    }
}

pub struct Wired {
    pub acting: Arc<ActingOps>,
    pub codex: Arc<CodexOps>,
    pub bridge: Arc<BridgeOps>,
    pub agy: Arc<AgyOps>,
    pub every: Arc<EverySession>,
    pub switches: Arc<Switches>,
    pub accounts: Arc<AccountsOps>,
    pub providers: Arc<ProvidersOps>,
    pub limits: Arc<LimitsOps>,
    pub catalog: Arc<CatalogService>,
    pub inbox: Arc<InboxOps>,
    pub outbox: Arc<OutboxOps>,
    pub ask: Arc<AskOps>,
    pub postmen: Postmen,
    pub outbox_notice: Arc<OutboxNotice>,
    pub vault: Arc<VaultOps>,
    pub install: Arc<InstallOps>,
    pub models: Arc<ModelsOps>,
    pub sidecar: Arc<AgySidecarOps>,
    pub settings: Arc<SettingsOps>,
    pub versions: Arc<VersionsOps>,
    pub awake: Arc<AwakeLoop>,
    pub events: Arc<LateEmit>,
    pub salt: LateSalt,
    history: Arc<DbSwitches>,
    log: OnceLock<(Arc<Mutex<EventsDb>>, Arc<LogOps>)>,
    pub wake: Arc<WakeLoop>,
    pub codex_children: Arc<CodexChildren>,
    pub cluster: ClusterParts,
}

impl Wired {
    pub fn build(
        layout: &Layout,
        sessions: Arc<Mutex<Value>>,
        room: Arc<dyn Room>,
        bridge_ports: (u16, u16),
    ) -> Wired {
        Wired::build_with_power(layout, sessions, room, bridge_ports, power_for(layout))
    }

    pub fn build_with_power(
        layout: &Layout,
        sessions: Arc<Mutex<Value>>,
        room: Arc<dyn Room>,
        bridge_ports: (u16, u16),
        power: Box<dyn Power>,
    ) -> Wired {
        let wakes = wakes_for(layout);
        Wired::build_with_hardware(layout, sessions, room, bridge_ports, power, wakes)
    }

    pub fn build_with_hardware(
        layout: &Layout,
        sessions: Arc<Mutex<Value>>,
        room: Arc<dyn Room>,
        bridge_ports: (u16, u16),
        power: Box<dyn Power>,
        wakes: Box<dyn Wakes>,
    ) -> Wired {
        let late = LateEmit::new();
        let salt: LateSalt = Arc::new(OnceLock::new());
        let events = Events::new(late.clone(), salt.clone());
        let acting_switch = Arc::new(Acting::off());
        let claude = claude_path(Path::new(&home_of(&layout.roots())));
        let acting = Arc::new(
            ActingOps::new(
                acting_switch.clone(),
                sessions.clone(),
                layout.roots(),
                room.clone(),
                Box::new(ClaudeAttacher::new(&claude)),
            )
            .with_launcher(
                Arc::new(LiveLauncher::new(RealLauncher::with_claude(claude.clone()))),
                Box::new(|ms| std::thread::sleep(Duration::from_millis(ms))),
            )
            .with_screen_file(layout.state_dir().join("permission-screen.txt"))
            .with_reply_screen_file(layout.state_dir().join("reply-screen.txt"))
            .with_events(events.clone()),
        );
        let codex = Arc::new(CodexOps::for_daemon(
            layout,
            acting_switch,
            room.clone(),
            &claude,
        ));
        acting.attach_codex(codex.clone());
        let inbox = InboxOps::new(layout.data_dir.clone(), room.clone())
            .with_events(events.clone())
            .with_cluster_dir(layout.cluster_dir());
        let outbox = Arc::new(outbox_ops(layout, &sessions, &acting, &room, &events));
        acting.attach_outbox(outbox.clone());
        let chrome = crate::chrome_ops::ChromeOps::new(crate::chrome_ops::LiveHosts::new(&acting));
        let noting = Arc::downgrade(&acting);
        chrome.set_audit(Arc::new(move |agent, word, result| {
            if let Some(acting) = noting.upgrade() {
                acting.note_chrome(agent, word, result);
            }
        }));
        acting.attach_chrome(chrome);
        crate::chrome_ops::reconcile_when_acting_turns_on(&acting);
        let node_slot = crate::ask_wiring::Slot::default();
        let worker = crate::ask_wiring::worker_in(node_slot.clone());
        let ask =
            crate::ask_wiring::build(layout, &sessions, &acting, &room, &events, &inbox, worker);
        let agy = Arc::new(AgyOps::for_daemon(
            layout,
            acting.acting_switch(),
            room.clone(),
        ));
        acting.attach_agy(agy.clone());
        let postmen = postmen_for(&outbox, &acting, layout, &codex, &agy);
        let outbox_notice = OutboxNotice::new(outbox.clone(), inbox.clone());
        let every = Arc::new(EverySession::new(layout));
        let bridge = Arc::new(
            BridgeOps::new(
                bridge_setup(layout, bridge_ports),
                sessions.clone(),
                acting.clone(),
                room.clone(),
                inbox.clone(),
                every.clone(),
            )
            .with_events(events.clone())
            .with_node_identity(
                layout
                    .cluster_dir()
                    .join(crate::cluster_wiring::IDENTITY_DIR),
            ),
        );
        let awake = Arc::new(AwakeLoop::for_daemon(
            layout,
            sessions.clone(),
            codex.clone(),
            agy.clone(),
            room.clone(),
            power,
        ));
        every.attach_agy(agy.clone());
        let wake = Arc::new(WakeLoop::for_daemon(
            layout,
            sessions.clone(),
            acting.clone(),
            room.clone(),
            wakes,
        ));

        let switches = Arc::new(Switches::off());
        let on = switches.accounts.clone();
        let history = DbSwitches::new();
        let accounts = Arc::new(AccountsOps::recording(
            layout,
            on.clone(),
            room.clone(),
            events.emit(),
            history.clone(),
        ));
        accounts.attach_acting(acting.clone());
        let claude_store = accounts.store();
        let names = claude_store.clone();
        let stores = ProviderStores::for_daemon(
            layout,
            &on,
            Arc::new(move || {
                names
                    .saved_list()
                    .into_iter()
                    .map(|a| ClaudeName {
                        id: a.id,
                        email: a.email,
                    })
                    .collect()
            }),
        );
        let providers = Arc::new(ProvidersOps::new(
            on.clone(),
            stores.registry(),
            room.clone(),
        ));
        let catalog = Arc::new(CatalogService::for_daemon(
            layout,
            (accounts.store(), on.clone()),
            room.clone(),
        ));
        agy.attach_catalog(catalog.clone());
        acting.attach_catalog(catalog.clone());
        let limits = Arc::new(LimitsOps::for_daemon(
            layout,
            on.clone(),
            room.clone(),
            catalog.shared_catalog(),
            inbox.clone(),
            history.clone(),
        ));
        limits.set_claude(
            Arc::new(
                ClaudeLimits::new(on.clone(), claude_store).with_renewer(Arc::new(
                    ClaudeRenewer::new(
                        Arc::new(RealLauncher::with_claude(claude.clone())),
                        layout.roots(),
                    ),
                )),
            ),
            Arc::new(SessionJobs::new(acting.clone())),
        );
        let codex_store = stores.codex.clone();
        let idle_on = on.clone();
        limits.set_codex(
            Arc::new(ProviderLimits::new(on.clone(), stores.codex.clone())),
            Box::new(move || idle_on.is_on() && idle_for_auto_switch(&codex_store.running_work())),
        );
        let (agy_switch, agy_idle) = (stores.antigravity.clone(), stores.antigravity.clone());
        let agy_on = on.clone();
        limits.set_antigravity(
            Arc::new(
                ProviderLimits::new(on, stores.antigravity.clone()).with_switch(Box::new(
                    move |id| {
                        agy_switch
                            .switch_in_background(id)
                            .map_err(|e| e.message().to_string())
                    },
                )),
            ),
            Box::new(move || agy_on.is_on() && agy_idle.idle_for_auto_switch()),
        );

        let vault = Arc::new(VaultOps::for_daemon(
            layout,
            switches.vault.clone(),
            room.clone(),
            inbox.clone(),
            events.clone(),
        ));
        let binary = std::env::current_exe().and_then(std::fs::canonicalize).ok();
        let sidecar = AgySidecarOps::for_daemon(
            layout,
            &agy,
            &acting,
            switches.install.clone(),
            binary.clone(),
            events.clone(),
        );
        let mut install = InstallOps::new(
            InstallSetup::from_layout(layout, binary.clone()),
            switches.install.clone(),
        )
        .with_events(events.clone());
        if let Some(binary) = binary {
            install = install.with_skill(crate::vault_skill_wiring::spec(&binary, vault.clone()));
        }
        let states: SwitchStates = {
            let (acting, bridge, switches) = (acting.clone(), bridge.clone(), switches.clone());
            Arc::new(move |name: &str| match name {
                "acting" => Some(acting.is_on()),
                "bridge" => Some(bridge.is_on()),
                other => switches.get(other).map(|s| s.is_on()),
            })
        };
        let settings = Arc::new(
            SettingsOps::new(layout.data_dir.clone(), room.clone(), states)
                .with_events(events.clone())
                .with_limits(limits.config().clone())
                .with_inbox(inbox.clone()),
        );
        let versions = VersionsOps::new(
            if layout.fixture {
                VersionRoots::inside(&layout.home())
            } else {
                VersionRoots::for_home(&layout.home())
            },
            layout.data_dir.clone(),
            room.clone(),
            Arc::new(RealLauncher::with_claude(claude.clone())),
            Arc::new(AuditRecorder::new(events.clone())),
            Arc::new(|| {
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_millis() as i64)
                    .unwrap_or(0)
            }),
            Version::parse(env!("CARGO_PKG_VERSION")).unwrap_or_else(Version::zero),
        );
        versions.checks().set_enabled(!layout.fixture);
        versions.checks().set_inbox(inbox.clone());
        versions.drifts().set_inbox(inbox.clone());
        versions.set_install_switch(switches.install.clone());
        let cluster = ClusterParts::build(
            layout,
            room.clone(),
            switches.cluster.clone(),
            (versions.clone(), {
                let bridge = bridge.clone();
                Arc::new(move || {
                    let served = bridge.served();
                    Some(BridgeFacts {
                        port: served.port,
                        fingerprint: served.fingerprint,
                    })
                })
            }),
            (inbox.clone(), vault.clone()),
            events.clone(),
            (sessions.clone(), acting.acting_switch()),
        );
        if let Ok(node) = &cluster.node {
            let _ = node_slot.set(node.clone());
        }
        cluster.fence_keeper().set_codex(codex.clone());
        codex.set_job_link(cluster.reporter.clone());
        bridge.cluster().attach(
            cluster.handler(),
            cluster.remote.clone(),
            cluster.prompts.clone(),
        );
        let relay = Arc::new(HeldRelay::new(relay_parts(RelayEnv {
            cluster: layout.cluster_dir(),
            link: cluster.link(),
            phone: bridge.cluster(),
            bridge: bridge.clone(),
            acting: acting.clone(),
            sessions: sessions.clone(),
            clock: Arc::new(crate::acting_ops::now_ms),
        })));
        bridge.set_replica(devices_replica(
            layout.cluster_dir(),
            layout.data_dir.clone(),
            cluster.link(),
        ));
        relay.start();
        let _ = cluster.held().set(relay.clone());
        bridge.cluster().attach_relay(relay);
        if let Some(sink) = cluster.failover_sink() {
            settings.bind_failover(sink);
        }
        if let Some(tell) = cluster.peer_teller() {
            outbox.set_tell(tell);
        }
        outbox.set_remote(cluster.remote_outbox());
        acting.attach_login(cluster.login.clone());
        let install = Arc::new(install);
        let models = Arc::new(ModelsOps::for_daemon(
            layout,
            catalog.clone(),
            install.clone(),
            crate::models_ops::codex_session_live(every.clone()),
        ));
        Wired {
            acting,
            codex,
            bridge,
            agy,
            every,
            switches,
            accounts,
            providers,
            limits,
            catalog,
            inbox,
            outbox,
            ask,
            postmen,
            outbox_notice,
            vault,
            install,
            models,
            sidecar,
            settings,
            versions,
            awake,
            events: late,
            salt,
            history,
            log: OnceLock::new(),
            wake,
            codex_children: Arc::new(CodexChildren::new(
                layout.codex_dir().join("sessions"),
                layout.codex_children_db(),
            )),
            cluster,
        }
    }

    pub fn set_events(&self, bus: Arc<dyn Emit>, salt: Salt) {
        self.events.set(bus);
        let _ = self.salt.set(salt);
    }

    pub fn set_log(&self, db: Arc<Mutex<EventsDb>>, ops: Arc<LogOps>) {
        self.history.set(db.clone());
        let _ = self.log.set((db, ops));
    }

    pub fn ops(
        &self,
        layout: &Layout,
        sessions: Arc<Mutex<Value>>,
        first_scan: Arc<FirstScan>,
        first_wait: Duration,
    ) -> Ops {
        let folders = listed_folders(
            live_folders(
                sessions.clone(),
                self.agy.titles(),
                layout.codex_dir().join("sessions"),
                layout.fixture,
            ),
            self.every.clone(),
        );
        let claude = claude_path(Path::new(&home_of(&layout.roots())));
        let git = GitOps::new(
            Arc::new(SandboxedLauncher::new(Arc::new(RealLauncher::with_claude(
                claude,
            )))),
            folders.clone(),
        );
        let listed = sessions.clone();
        self.versions.checks().set_self_test(Arc::new(move || {
            listed
                .lock()
                .map(|v| v.is_array() || v.is_object() || v.is_null())
                .unwrap_or(false)
        }));
        let ops = Ops::with_acting(sessions, self.acting.clone())
            .with_bridge(self.bridge.clone())
            .with_codex(self.codex.clone())
            .with_agy(self.agy.clone())
            .with_every(self.every.clone())
            .with_pages(Arc::new(
                PagesOps::new(PagesSetup::from_layout(layout)).with_titles(self.agy.titles()),
            ))
            .with_group(Arc::new(
                SwitchOps::new(self.switches.clone(), self.acting.room.clone())
                    .with_events(Events::new(self.events.clone(), self.salt.clone())),
            ))
            .with_group(self.accounts.clone())
            .with_group(self.install.clone())
            .with_group(self.models.clone())
            .with_group(crate::cost_profile_ops::wired(
                layout,
                &self.catalog,
                &self.switches,
            ))
            .with_group(self.sidecar.clone())
            .with_group(self.providers.clone())
            .with_group(self.limits.clone())
            .with_group(crate::cost_report_ops::shared(self.limits.cost()))
            .with_group(self.catalog.clone())
            .with_group(self.inbox.clone())
            .with_group(self.outbox.clone())
            .with_group(self.ask.clone())
            .with_group(self.vault.clone())
            .with_group(self.settings.clone())
            .with_group(self.versions.clone())
            .with_group(self.cluster.ops.clone())
            .with_group(self.cluster.follow.clone())
            .with_group(self.cluster.dispatch.clone())
            .with_group(self.cluster.returns.clone())
            .with_shells(self.cluster.shells.clone())
            .with_cluster(self.cluster.forward(), self.cluster.remote.clone())
            .with_move_gate(self.cluster.gate.clone())
            .with_remote_prompts(self.cluster.prompts.clone())
            .with_group(Arc::new(git))
            .with_group(Arc::new(AwakeOps::new(self.awake.clone())))
            .with_group(Arc::new(OrganizationsOps::for_daemon(layout, folders)))
            .with_group(Arc::new(TailscaleOps::system()))
            .with_group(self.codex_children.clone())
            .with_first_scan(first_scan, first_wait);
        match self.log.get() {
            Some((db, log)) => ops
                .with_group(Arc::new(ActivityOps::new(
                    db.clone(),
                    Arc::new(|_, _| None),
                )))
                .with_group(log.clone()),
            None => ops.with_group(Arc::new(ActivityOps::for_daemon(layout))),
        }
    }

    pub fn start(&self) {
        self.limits.spawn_loop();
        self.inbox.spawn_loop();
        self.versions.start();
        self.awake.spawn_loop();
        self.wake.spawn_loop();
        self.codex_children.start();
        self.postmen.begin();
        self.outbox_notice.spawn_loop();
        self.ask.spawn_loop();
    }

    pub fn load(&self) {
        self.inbox.load();
        self.ask.load(crate::acting_ops::now_ms());
        self.outbox.load(crate::acting_ops::now_ms());
    }

    pub fn close(&self) {
        self.ask.close();
        self.outbox_notice.close();
        self.postmen.stop();
        self.wake.close();
        self.codex_children.close();
        self.awake.close();
        self.acting.close();
        self.codex.close();
        self.agy.close();
        self.providers.close();
        self.vault.close();
        self.bridge.close();
        self.limits.close();
        self.settings.close();
        self.cluster.close();
        self.inbox.close();
        self.versions.close();
    }

    pub fn accounts_switch(&self) -> &Arc<SwitchFlag> {
        &self.switches.accounts
    }
}

#[cfg(test)]
#[path = "daemon_wiring_tests.rs"]
mod tests;
