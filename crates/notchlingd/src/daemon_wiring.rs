use crate::accounts_ops::AccountsOps;
use crate::acting::Acting;
use crate::acting_ops::{ActingOps, Room};
use crate::activity_ops::ActivityOps;
use crate::agy_ops::AgyOps;
use crate::attach::{ClaudeAttacher, LiveLauncher};
use crate::awake_loop::{power_for, AwakeLoop};
use crate::bridge_backend::APPROVAL_WAIT;
use crate::bridge_ops::{BridgeOps, BridgeSetup};
use crate::catalog_ops::CatalogService;
use crate::codex_ops::CodexOps;
use crate::daemon::{FirstScan, FIRST_LIST_WAIT};
use crate::every_session::{EverySession, CLAUDE};
use crate::git_ops::{live_folders, GitOps};
use crate::inbox_ops::InboxOps;
use crate::install_ops::{InstallOps, InstallSetup};
use crate::layout::Layout;
use crate::limits_ops::LimitsOps;
use crate::limits_sources::{ClaudeLimits, ProviderLimits, SessionJobs};
use crate::op_group::OpGroup;
use crate::pages_ops::{PagesOps, PagesSetup};
use crate::providers_daemon::ProviderStores;
use crate::providers_ops::ProvidersOps;
use crate::quiet_ops;
use crate::settings_ops::{SettingsOps, SwitchStates};
use crate::switches::{SwitchFlag, SwitchOps, Switches};
use crate::vault_ops::VaultOps;
use crate::versions_ops::{AuditRecorder, VersionsOps};
use notchling_engine::accounts::codex::login::idle_for_auto_switch;
use notchling_engine::accounts::desktop::ClaudeName;
use notchling_engine::awake::Power;
use notchling_engine::control::launcher::claude_path;
use notchling_engine::control::session::home_of;
use notchling_engine::control::RealLauncher;
use notchling_engine::delegate::skill::SkillTool;
use notchling_engine::git::SandboxedLauncher;
use notchling_engine::install::skills::SkillSpec;
use notchling_engine::vault::skill as vault_skill;
use notchling_engine::versions::mac::Roots as VersionRoots;
use notchling_engine::versions::Version;
use notchling_local_api::{ops, topics, ApiError, AuditLog, Caller, Handler, OpSpec, Tier};
use serde_json::{Map, Value};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub struct Ops {
    sessions: Arc<Mutex<Value>>,
    acting: Option<Arc<ActingOps>>,
    bridge: Option<Arc<BridgeOps>>,
    codex: Option<Arc<CodexOps>>,
    agy: Option<Arc<AgyOps>>,
    pages: Option<Arc<PagesOps>>,
    every: Option<Arc<EverySession>>,
    groups: Vec<Arc<dyn OpGroup>>,
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
            groups: Vec::new(),
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
    fn sessions_list(&self, args: &Value) -> Result<(Value, bool), ApiError> {
        let every = every_tool(args)?;
        let landed = self.first.wait(self.first_wait);
        let list = self
            .sessions
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        if !every {
            return Ok((list, landed));
        }
        let mut rows = tagged(list, CLAUDE);
        if let Some(other) = &self.every {
            rows.extend(other.values(now_ms()));
        }
        Ok((Value::Array(rows), landed))
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
        let (list, landed) = match self.sessions_list(args) {
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
        if let Some(bridge) = self.bridge.as_ref().filter(|b| b.spec(op).is_some()) {
            return bridge.call(op, args);
        }
        if let Some(codex) = self.codex.as_ref().filter(|c| c.spec(op).is_some()) {
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
            (ops::SESSIONS_LIST, _) => self.sessions_list(args).map(|(list, _)| list),
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
    pub vault: Arc<VaultOps>,
    pub install: Arc<InstallOps>,
    pub settings: Arc<SettingsOps>,
    pub versions: Arc<VersionsOps>,
    pub awake: Arc<AwakeLoop>,
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
            .with_screen_file(layout.state_dir().join("permission-screen.txt")),
        );
        let codex = Arc::new(CodexOps::for_daemon(
            layout,
            acting_switch,
            room.clone(),
            &claude,
        ));
        acting.attach_codex(codex.clone());
        let inbox = InboxOps::new(layout.data_dir.clone(), room.clone());
        let every = Arc::new(EverySession::new(layout));
        let bridge = Arc::new(BridgeOps::new(
            bridge_setup(layout, bridge_ports),
            sessions.clone(),
            acting.clone(),
            room.clone(),
            inbox.clone(),
            every.clone(),
        ));
        let agy = Arc::new(AgyOps::for_daemon(
            layout,
            acting.acting_switch(),
            room.clone(),
        ));
        acting.attach_agy(agy.clone());
        let awake = Arc::new(AwakeLoop::for_daemon(
            layout,
            sessions,
            codex.clone(),
            agy.clone(),
            power,
        ));
        every.attach_agy(agy.clone());

        let switches = Arc::new(Switches::off());
        let on = switches.accounts.clone();
        let accounts = Arc::new(AccountsOps::for_daemon(layout, on.clone(), room.clone()));
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
        ));
        limits.set_claude(
            Arc::new(ClaudeLimits::new(on.clone(), claude_store)),
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
        ));
        let binary = std::env::current_exe().and_then(std::fs::canonicalize).ok();
        let mut install = InstallOps::new(
            InstallSetup::from_layout(layout, binary.clone()),
            switches.install.clone(),
        );
        if let Some(binary) = binary {
            install = install.with_skill(vault_skill_spec(&binary, vault.clone()));
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
                .with_audit(AuditLog::new(layout.audit_path()))
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
            Arc::new(AuditRecorder::new(AuditLog::new(layout.audit_path()))),
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
            vault,
            install: Arc::new(install),
            settings,
            versions,
            awake,
        }
    }

    pub fn ops(
        &self,
        layout: &Layout,
        sessions: Arc<Mutex<Value>>,
        first_scan: Arc<FirstScan>,
        first_wait: Duration,
    ) -> Ops {
        let folders = live_folders(
            sessions.clone(),
            self.agy.titles(),
            layout.codex_dir().join("sessions"),
            layout.fixture,
        );
        let claude = claude_path(Path::new(&home_of(&layout.roots())));
        let git = GitOps::new(
            Arc::new(SandboxedLauncher::new(Arc::new(RealLauncher::with_claude(
                claude,
            )))),
            folders,
        );
        let listed = sessions.clone();
        self.versions.checks().set_self_test(Arc::new(move || {
            listed
                .lock()
                .map(|v| v.is_array() || v.is_object() || v.is_null())
                .unwrap_or(false)
        }));
        Ops::with_acting(sessions, self.acting.clone())
            .with_bridge(self.bridge.clone())
            .with_codex(self.codex.clone())
            .with_agy(self.agy.clone())
            .with_every(self.every.clone())
            .with_pages(Arc::new(
                PagesOps::new(PagesSetup::from_layout(layout)).with_titles(self.agy.titles()),
            ))
            .with_group(Arc::new(
                SwitchOps::new(self.switches.clone(), self.acting.room.clone())
                    .with_audit(AuditLog::new(layout.audit_path())),
            ))
            .with_group(self.accounts.clone())
            .with_group(self.install.clone())
            .with_group(self.providers.clone())
            .with_group(self.limits.clone())
            .with_group(self.catalog.clone())
            .with_group(self.inbox.clone())
            .with_group(self.vault.clone())
            .with_group(self.settings.clone())
            .with_group(self.versions.clone())
            .with_group(Arc::new(git))
            .with_group(Arc::new(ActivityOps::for_daemon(layout)))
            .with_first_scan(first_scan, first_wait)
    }

    pub fn start(&self) {
        self.limits.spawn_loop();
        self.inbox.spawn_loop();
        self.acting.spawn_held_loop();
        self.versions.start();
        self.awake.spawn_loop();
    }

    pub fn load(&self) {
        self.inbox.load();
    }

    pub fn close(&self) {
        self.awake.close();
        self.acting.stop_held_loop();
        self.acting.close();
        self.codex.close();
        self.agy.close();
        self.providers.close();
        self.vault.close();
        self.bridge.close();
        self.limits.close();
        self.settings.close();
        self.inbox.close();
        self.versions.close();
    }

    pub fn accounts_switch(&self) -> &Arc<SwitchFlag> {
        &self.switches.accounts
    }
}

fn vault_skill_spec(binary: &Path, vault: Arc<VaultOps>) -> SkillSpec {
    let binary = binary.to_string_lossy().into_owned();
    SkillSpec {
        name: vault_skill::NAME.to_string(),
        tools: vec![SkillTool::Claude, SkillTool::Codex, SkillTool::Antigravity],
        text: Box::new(move |tool| {
            let tool = match tool {
                SkillTool::Claude => vault_skill::SkillTool::Claude,
                SkillTool::Codex => vault_skill::SkillTool::Codex,
                SkillTool::Antigravity => vault_skill::SkillTool::Antigravity,
            };
            vault_skill::content(&binary, &vault.skill_names(), tool)
        }),
    }
}

#[cfg(test)]
#[path = "daemon_wiring_tests.rs"]
mod tests;
