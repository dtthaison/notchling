use crate::account_asks::Book;
use crate::account_ids::AccountIds;
use crate::acting_ops::ActingOps;
use crate::providers_ops::ProvidersOps;
use crate::switches::SwitchFlag;
use notchling_engine::accounts::claude::{ClaudeAccounts, Snapshot};
use notchling_engine::organizations::{Kind, Organizations, Resolved};
use notchling_engine::providers::{ProviderId, ProviderState};
use notchling_engine::sessions::{Agent, Lane};
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub struct LiveBook {
    pub claude: Arc<ClaudeAccounts>,
    pub providers: Arc<ProvidersOps>,
    pub acting: Arc<ActingOps>,
    pub orgs: Organizations,
    pub ids: AccountIds,
    pub accounts: Arc<SwitchFlag>,
    pub home: PathBuf,
}

impl LiveBook {
    pub fn new(
        claude: Arc<ClaudeAccounts>,
        providers: Arc<ProvidersOps>,
        acting: Arc<ActingOps>,
        orgs: Organizations,
        daemon_dir: &Path,
        accounts: Arc<SwitchFlag>,
        home: &Path,
    ) -> LiveBook {
        LiveBook {
            claude,
            providers,
            acting,
            orgs,
            ids: AccountIds::in_dir(daemon_dir),
            accounts,
            home: home.to_path_buf(),
        }
    }
}

impl Book for LiveBook {
    fn claude(&self) -> Snapshot {
        self.claude.snapshot()
    }

    fn lanes(&self) -> Vec<Lane> {
        self.claude.env().lanes.all()
    }

    fn providers(&self) -> Vec<(String, ProviderState)> {
        self.providers.states()
    }

    fn agents(&self) -> Vec<Agent> {
        self.acting.agents_now()
    }

    fn closed(&self) -> bool {
        self.orgs.view().is_closed()
    }

    fn listing(&self, kind: Kind) -> Option<Vec<(String, Option<String>)>> {
        self.orgs.listing(kind)
    }

    fn resolve(&self, kind: Kind, key: &str, field: Option<&str>) -> Resolved {
        self.orgs.view().resolve(kind, key, field)
    }

    fn folder_org(&self, folder: &str) -> String {
        self.orgs
            .view()
            .folder_here(Path::new(folder), &self.home)
            .organization
    }

    fn main_dir(&self) -> String {
        self.acting.roots.main_dir.to_string_lossy().into_owned()
    }

    fn home(&self) -> String {
        self.home.to_string_lossy().into_owned()
    }

    fn accounts_on(&self) -> bool {
        self.accounts.is_on()
    }

    fn acting_on(&self) -> bool {
        self.acting.is_on()
    }

    fn can_add(&self, provider: &str) -> bool {
        provider != ProviderId::CLAUDE_CODE && self.providers.can_add(provider)
    }

    fn add_limit_ms(&self) -> i64 {
        i64::try_from(self.providers.add_limit().as_millis()).unwrap_or(i64::MAX)
    }

    fn added(&self, attempt: u64) -> Option<Vec<String>> {
        self.providers.added(attempt)
    }

    fn id(&self, provider: &str, raw: &str) -> Option<String> {
        self.ids.id(provider, raw)
    }

    fn lane_id(&self, name: &str) -> Option<String> {
        self.ids.lane(name)
    }

    fn place(&self, kind: Kind, raw: &str, organization: &str) -> bool {
        self.orgs.set_organization(kind, raw, organization).is_ok()
    }
}

#[cfg(test)]
#[path = "account_asks_live_tests.rs"]
mod tests;
