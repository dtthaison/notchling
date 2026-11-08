//! The daemon's real providers for [`ProvidersOps`]: where each provider keeps its files, and which
//! effects are real. Under a fixture root (`NOTCHLING_DATA_ROOT`) every effect is a fake, so a dev
//! run never touches the real Keychain, apps or processes.
//!
//! [`ProviderStores`] keeps the concrete stores too: the limit watchers read Codex's and
//! Antigravity's ([`crate::limits_sources::ProviderLimits`]), and Codex's watcher asks whether
//! anything would be interrupted by a switch.

use crate::acting_ops::Room;
use crate::layout::Layout;
use crate::providers_ops::ProvidersOps;
use crate::switches::SwitchFlag;
use notchling_engine::accounts::antigravity::{
    AntigravityDeps, AntigravityPaths, AntigravityStore,
};
use notchling_engine::accounts::codex::{CodexDeps, CodexPaths, CodexStore};
use notchling_engine::accounts::desktop::{
    ClaudeAccounts as ClaudeNames, ClaudeName, DesktopDeps, DesktopPaths, DesktopStore,
};
use notchling_engine::accounts::generic_password::{FakeKeychain, Keychain, SecurityTool};
use notchling_engine::accounts::procs::{
    Apps, FakeApps, FakeProcs, Procs, SystemApps, SystemProcs,
};
use notchling_engine::providers::http_real::TlsHttp;
use notchling_engine::providers::{FakeHttp, HttpClient, ProviderRegistry};
use std::sync::Arc;

/// The four outside effects, real or fake.
type Effects = (
    Arc<dyn Keychain>,
    Arc<dyn HttpClient>,
    Arc<dyn Procs>,
    Arc<dyn Apps>,
);

/// The daemon's three provider stores.
pub struct ProviderStores {
    pub desktop: Arc<DesktopStore>,
    pub codex: Arc<CodexStore>,
    pub antigravity: Arc<AntigravityStore>,
}

impl ProviderStores {
    /// In the Accounts window's order: Claude Code (E8a, apart), Claude Desktop, Codex,
    /// Antigravity.
    pub fn registry(&self) -> ProviderRegistry {
        let mut registry = ProviderRegistry::new();
        registry.register(self.desktop.clone());
        registry.register(self.codex.clone());
        registry.register(self.antigravity.clone());
        registry
    }

    /// The daemon's stores. Under a fixture root (`NOTCHLING_DATA_ROOT`) every effect is a fake,
    /// so a dev run never touches the real Keychain, apps or processes; otherwise the real
    /// `/usr/bin/security`, apps, processes and HTTPS. The saved account names and Claude
    /// Desktop's copies are the daemon's own, in `<data dir>/daemon/accounts`. `claude_accounts`
    /// is E8a's account list, which Desktop's rows are matched against.
    pub fn for_daemon(layout: &Layout, claude_accounts: ClaudeNames) -> ProviderStores {
        let home = layout.home();
        let state_dir = layout.data_dir.join("daemon").join("accounts");
        let (keychain, http, procs, apps): Effects = if layout.fixture {
            (
                Arc::new(FakeKeychain::new()),
                Arc::new(FakeHttp::new()),
                Arc::new(FakeProcs::new()),
                Arc::new(FakeApps::new()),
            )
        } else {
            (
                Arc::new(SecurityTool),
                Arc::new(TlsHttp),
                Arc::new(SystemProcs { home: home.clone() }),
                Arc::new(SystemApps),
            )
        };
        let (apps_dir, claude_dir) = if layout.fixture {
            (home.join("Applications"), home.join("desktop-claude"))
        } else if cfg!(target_os = "macos") {
            (
                std::path::PathBuf::from("/Applications"),
                home.join("Library/Application Support/Claude"),
            )
        } else {
            (
                std::path::PathBuf::from("/Applications"),
                home.join(".config/Claude"),
            )
        };
        let clock: Arc<dyn Fn() -> f64 + Send + Sync> = Arc::new(|| {
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs_f64())
                .unwrap_or(0.0)
        });
        let desktop = Arc::new(DesktopStore::new(
            DesktopDeps {
                apps: apps.clone(),
                claude_accounts,
            },
            DesktopPaths {
                claude_dir,
                store_dir: state_dir.join("desktop"),
                apps_dir: apps_dir.clone(),
            },
        ));
        let codex = Arc::new(CodexStore::new(
            CodexDeps {
                keychain: keychain.clone(),
                http: http.clone(),
                procs: procs.clone(),
                apps: apps.clone(),
                clock: clock.clone(),
            },
            CodexPaths {
                home: layout.codex_dir(),
                state_dir: state_dir.clone(),
            },
        ));
        let gemini = layout
            .antigravity_dir()
            .parent()
            .map(std::path::Path::to_path_buf)
            .unwrap_or_else(|| home.join(".gemini"));
        let antigravity = Arc::new(AntigravityStore::new(
            AntigravityDeps {
                keychain,
                http,
                procs,
                apps,
                clock,
            },
            AntigravityPaths {
                token_file: gemini.join("jetski-standalone-oauth-token"),
                apps_dir,
                state_dir,
            },
        ));
        ProviderStores {
            desktop,
            codex,
            antigravity,
        }
    }
}

impl ProvidersOps {
    /// The daemon's provider ops over [`ProviderStores::for_daemon`], with no Claude Code accounts
    /// for Desktop to match (the daemon's wiring passes E8a's list).
    pub fn for_daemon(
        layout: &Layout,
        accounts: Arc<SwitchFlag>,
        room: Arc<dyn Room>,
    ) -> ProvidersOps {
        let stores = ProviderStores::for_daemon(layout, Arc::new(Vec::<ClaudeName>::new));
        ProvidersOps::new(accounts, stores.registry(), room)
    }
}
