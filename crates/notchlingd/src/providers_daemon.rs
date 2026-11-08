//! The daemon's real providers for [`ProvidersOps`]: where each provider keeps its files, and which
//! effects are real. Under a fixture root (`NOTCHLING_DATA_ROOT`) every effect is a fake, so a dev
//! run never touches the real Keychain, apps or processes.

use crate::acting_ops::Room;
use crate::layout::Layout;
use crate::providers_ops::{ProvidersOps, SwitchFlag};
use notchling_engine::accounts::antigravity::{
    AntigravityDeps, AntigravityPaths, AntigravityStore,
};
use notchling_engine::accounts::codex::{CodexDeps, CodexPaths, CodexStore};
use notchling_engine::accounts::desktop::{ClaudeName, DesktopDeps, DesktopPaths, DesktopStore};
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

impl ProvidersOps {
    /// The daemon's providers. Under a fixture root (`NOTCHLING_DATA_ROOT`) every effect is a fake,
    /// so a dev run never touches the real Keychain, apps or processes; otherwise the real
    /// `/usr/bin/security`, apps, processes and HTTPS. The saved account names and Claude Desktop's
    /// copies are the daemon's own, in `<data dir>/daemon/accounts`.
    pub fn for_daemon(
        layout: &Layout,
        accounts: Arc<SwitchFlag>,
        room: Arc<dyn Room>,
    ) -> ProvidersOps {
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
        let mut registry = ProviderRegistry::new();
        // The Accounts window's order: Claude Code (E8a), Claude Desktop, Codex, Antigravity.
        registry.register(Arc::new(DesktopStore::new(
            DesktopDeps {
                apps: apps.clone(),
                // E8a's account list, once it lands: Desktop lists the Claude Code accounts it has
                // a session for.
                claude_accounts: Arc::new(Vec::<ClaudeName>::new),
            },
            DesktopPaths {
                claude_dir,
                store_dir: state_dir.join("desktop"),
                apps_dir: apps_dir.clone(),
            },
        )));
        registry.register(Arc::new(CodexStore::new(
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
        )));
        let gemini = layout
            .antigravity_dir()
            .parent()
            .map(std::path::Path::to_path_buf)
            .unwrap_or_else(|| home.join(".gemini"));
        registry.register(Arc::new(AntigravityStore::new(
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
        )));
        ProvidersOps::new(accounts, registry, room)
    }
}
