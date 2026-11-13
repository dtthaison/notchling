//! What the daemon serves: [`Ops`], the [`Handler`] over every area's ops, and [`Wired`], which
//! builds each area for one run of [`crate::daemon::run`] and closes them when it stops.
//!
//! Every switch starts off, whatever a file says: `acting` (M3, and M5's Codex and Antigravity
//! behind it), the bridge (M4), and Milestone 6's `accounts` (Claude Code's accounts and lanes,
//! the other providers, the limit watchers), `vault` and `install` ([`crate::switches`]). Under a
//! fixture root every login effect is a fake or refused: no Keychain, no network, no real folder.

use crate::accounts_ops::AccountsOps;
use crate::acting::Acting;
use crate::acting_ops::{ActingOps, Room};
use crate::agy_ops::AgyOps;
use crate::attach::{ClaudeAttacher, LiveLauncher};
use crate::bridge_backend::APPROVAL_WAIT;
use crate::bridge_ops::{BridgeOps, BridgeSetup};
use crate::catalog_ops::CatalogService;
use crate::codex_ops::CodexOps;
use crate::daemon::{FirstScan, FIRST_LIST_WAIT};
use crate::inbox_ops::InboxOps;
use crate::install_ops::{InstallOps, InstallSetup};
use crate::layout::Layout;
use crate::limits_ops::LimitsOps;
use crate::limits_sources::{ClaudeLimits, ProviderLimits, SessionJobs};
use crate::op_group::OpGroup;
use crate::pages_ops::{PagesOps, PagesSetup};
use crate::providers_daemon::ProviderStores;
use crate::providers_ops::ProvidersOps;
use crate::switches::{SwitchFlag, SwitchOps, Switches};
use crate::vault_ops::VaultOps;
use notchling_engine::accounts::codex::login::idle_for_auto_switch;
use notchling_engine::accounts::desktop::ClaudeName;
use notchling_engine::control::launcher::claude_path;
use notchling_engine::control::session::home_of;
use notchling_engine::control::RealLauncher;
use notchling_engine::delegate::skill::SkillTool;
use notchling_engine::install::skills::SkillSpec;
use notchling_engine::vault::skill as vault_skill;
use notchling_local_api::{ops, topics, ApiError, AuditLog, Caller, Handler, OpSpec, Tier};
use serde_json::{Map, Value};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// The ops: M2's session list (agent tier), with [`Ops::with_acting`] M3's ([`ActingOps`]), and
/// with [`Ops::with_bridge`] M4's phone bridge ([`BridgeOps`], app tier).
pub struct Ops {
    sessions: Arc<Mutex<Value>>,
    acting: Option<Arc<ActingOps>>,
    bridge: Option<Arc<BridgeOps>>,
    codex: Option<Arc<CodexOps>>,
    agy: Option<Arc<AgyOps>>,
    pages: Option<Arc<PagesOps>>,
    /// Milestone 6's areas ([`Ops::with_group`]).
    groups: Vec<Arc<dyn OpGroup>>,
    /// The topics a client may follow: those of the ops this daemon has.
    topic_list: Vec<&'static str>,
    /// What `sessions.list` waits for, and for how long.
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

    /// M5's Codex ops ([`CodexOps`]), and the `tools` topic they publish on.
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

    /// A group of ops served beside these (Milestone 6's switches, accounts, vault, install), and
    /// the topics it publishes on.
    pub fn with_group(mut self, group: Arc<dyn OpGroup>) -> Ops {
        self.groups.push(group);
        self.with_topics()
    }

    fn group(&self, op: &str) -> Option<&Arc<dyn OpGroup>> {
        self.groups.iter().find(|g| g.spec(op).is_some())
    }

    /// M5's Antigravity ops ([`AgyOps`]), publishing on the `tools` topic too.
    pub fn with_agy(mut self, agy: Arc<AgyOps>) -> Ops {
        self.agy = Some(agy);
        self.with_topics()
    }

    /// M5's pages library and session index ([`PagesOps`], agent tier, reads only).
    pub fn with_pages(mut self, pages: Arc<PagesOps>) -> Ops {
        self.pages = Some(pages);
        self
    }

    /// `sessions.list` waits up to `wait` for `first`, then answers with what the list holds.
    pub fn with_first_scan(mut self, first: Arc<FirstScan>, wait: Duration) -> Ops {
        self.first = first;
        self.first_wait = wait;
        self
    }
}

impl Ops {
    /// The list, and whether the first read was in. Only this client's thread waits. Past the wait
    /// the list is what there is, and the first read follows as `sessions.changed` to a client that
    /// subscribed.
    fn sessions_list(&self) -> (Value, bool) {
        let landed = self.first.wait(self.first_wait);
        let list = self
            .sessions
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        (list, landed)
    }
}

impl Handler for Ops {
    /// `sessions.list` answered before the first read adds `"partial": true` beside `ok`; the
    /// result itself stays a bare list, which every client reads as one.
    fn call_with_extras(
        &self,
        caller: &Caller,
        op: &str,
        args: &Value,
    ) -> (Result<Value, ApiError>, Map<String, Value>) {
        if op != ops::SESSIONS_LIST {
            return (self.call(caller, op, args), Map::new());
        }
        let (list, landed) = self.sessions_list();
        let mut extras = Map::new();
        if !landed {
            extras.insert("partial".into(), Value::Bool(true));
        }
        (Ok(list), extras)
    }

    fn spec(&self, op: &str) -> Option<OpSpec> {
        match op {
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
        }
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
            (ops::SESSIONS_LIST, _) => Ok(self.sessions_list().0),
            (op, Some(acting)) => acting.call(caller, op, args),
            (op, None) => Err(ApiError::internal(format!("{op} has no handler"))),
        }
    }

    fn topics(&self) -> &[&str] {
        &self.topic_list
    }

    /// The held prompts are the app's: their ids are what answers them. So are the bridge's
    /// pairing requests. A group says for its own topics (the vault's approvals and forms).
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

/// Where the bridge keeps its files: `AgentConsole` beside Notchling's data folder (the Swift
/// app's, so paired phones carry over; `<root>/AgentConsole` under a fixture root), and photos in
/// `AgentConsole/phone-images` beside the cache folder.
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

/// Every area's ops for one daemon run.
pub struct Wired {
    pub acting: Arc<ActingOps>,
    pub codex: Arc<CodexOps>,
    pub bridge: Arc<BridgeOps>,
    pub agy: Arc<AgyOps>,
    pub switches: Arc<Switches>,
    pub accounts: Arc<AccountsOps>,
    pub providers: Arc<ProvidersOps>,
    pub limits: Arc<LimitsOps>,
    /// The one catalog: `catalog.get`, and the prices cost reads.
    pub catalog: Arc<CatalogService>,
    pub inbox: Arc<InboxOps>,
    pub vault: Arc<VaultOps>,
    pub install: Arc<InstallOps>,
}

impl Wired {
    /// Builds every area, all off. `room` is the server's, set once it is bound.
    pub fn build(
        layout: &Layout,
        sessions: Arc<Mutex<Value>>,
        room: Arc<dyn Room>,
        bridge_ports: (u16, u16),
    ) -> Wired {
        // Off at every start, whatever a file says: only the app's `settings.set_acting` during
        // this run turns it on.
        let acting_switch = Arc::new(Acting::off());
        // The real terminal and launcher, used only while acting is on.
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
        // Codex turns (M5), behind the same switch: nothing starts until acting is on.
        let codex = Arc::new(CodexOps::for_daemon(
            layout,
            acting_switch,
            room.clone(),
            &claude,
        ));
        acting.attach_codex(codex.clone());
        let inbox = InboxOps::new(layout.data_dir.clone(), room.clone());
        // No port is bound and nothing is written under `AgentConsole` until the app's
        // `bridge.set` in this run.
        let bridge = Arc::new(BridgeOps::new(
            bridge_setup(layout, bridge_ports),
            sessions,
            acting.clone(),
            room.clone(),
            inbox.clone(),
        ));
        // `agy` starts only on an acting op, and the copy of its list is made only when the list
        // is read.
        let agy = Arc::new(AgyOps::for_daemon(
            layout,
            acting.acting_switch(),
            room.clone(),
        ));
        acting.attach_agy(agy.clone());

        // Milestone 6: no login, vault value or install target is touched until the app turns
        // that switch on in this run.
        let switches = Arc::new(Switches::off());
        let on = switches.accounts.clone();
        let accounts = Arc::new(AccountsOps::for_daemon(layout, on.clone(), room.clone()));
        accounts.attach_acting(acting.clone());
        // Desktop's rows match Claude Code's saved accounts: the daemon's own list file, no login.
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
        // The one catalog: the data file with `catalog.json` over it, Claude's list fetched with
        // the main login's token only while `accounts` is on, Codex's from its cache file and
        // Antigravity's from the options an open `agy acp` reports.
        let catalog = Arc::new(CatalogService::for_daemon(
            layout,
            (accounts.store(), on.clone()),
            room.clone(),
        ));
        agy.attach_catalog(catalog.clone());
        acting.attach_catalog(catalog.clone());
        // The limit watchers, over the same stores. The loop calls them only while `accounts` is
        // on; `cost.summary` only reads transcripts and needs no switch.
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
        Wired {
            acting,
            codex,
            bridge,
            agy,
            switches,
            accounts,
            providers,
            limits,
            catalog,
            inbox,
            vault,
            install: Arc::new(install),
        }
    }

    /// What the server serves.
    pub fn ops(
        &self,
        layout: &Layout,
        sessions: Arc<Mutex<Value>>,
        first_scan: Arc<FirstScan>,
        first_wait: Duration,
    ) -> Ops {
        Ops::with_acting(sessions, self.acting.clone())
            .with_bridge(self.bridge.clone())
            .with_codex(self.codex.clone())
            .with_agy(self.agy.clone())
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
            .with_first_scan(first_scan, first_wait)
    }

    /// The server answers: the limits loop starts (a flag check per tick while `accounts` is off).
    pub fn start(&self) {
        self.limits.spawn_loop();
        self.inbox.spawn_loop();
    }

    /// The daemon stops: held hooks pass, turns and waits end, the bridge's ports close.
    pub fn close(&self) {
        self.acting.close();
        self.codex.close();
        self.agy.close();
        self.providers.close();
        self.vault.close();
        self.bridge.close();
        self.limits.close();
        self.inbox.close();
    }

    /// The accounts switch (tests).
    pub fn accounts_switch(&self) -> &Arc<SwitchFlag> {
        &self.switches.accounts
    }
}

/// The `notchling-vault` skill for the install ops (Swift `VaultSkill`), in all three tools. Its
/// description lists the saved names, read only while the vault is on.
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
