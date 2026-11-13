pub mod http;
pub mod http_real;
pub mod usage;

#[cfg(test)]
mod tests;

pub use http::{FakeHttp, HttpClient, HttpRequest, HttpResponse, Trust};
pub use usage::{Bar, Group, GroupedGroup, GroupedUsage, Kind, UsageReport, UsageWindow};

use std::fmt;
use std::sync::Arc;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ProviderId(String);

impl ProviderId {
    pub const CLAUDE_CODE: &'static str = "claudeCode";
    pub const CLAUDE_DESKTOP: &'static str = "claudeDesktop";
    pub const CODEX: &'static str = "codex";
    pub const ANTIGRAVITY: &'static str = "antigravity";

    pub fn new(id: &str) -> ProviderId {
        ProviderId(id.to_string())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<&str> for ProviderId {
    fn from(id: &str) -> ProviderId {
        ProviderId::new(id)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Capabilities(u8);

impl Capabilities {
    pub const NONE: Capabilities = Capabilities(0);
    pub const ADD_ACCOUNT: Capabilities = Capabilities(1);
    pub const RESTARTS_APP: Capabilities = Capabilities(1 << 1);
    pub const USAGE: Capabilities = Capabilities(1 << 2);
    pub const AUTO_SWITCH: Capabilities = Capabilities(1 << 3);
    pub const COST: Capabilities = Capabilities(1 << 4);

    pub const fn with(self, other: Capabilities) -> Capabilities {
        Capabilities(self.0 | other.0)
    }

    pub fn contains(self, other: Capabilities) -> bool {
        self.0 & other.0 == other.0
    }

    pub fn names(self) -> Vec<&'static str> {
        [
            (Capabilities::ADD_ACCOUNT, "addAccount"),
            (Capabilities::RESTARTS_APP, "restartsApp"),
            (Capabilities::USAGE, "usage"),
            (Capabilities::AUTO_SWITCH, "autoSwitch"),
            (Capabilities::COST, "cost"),
        ]
        .into_iter()
        .filter(|(c, _)| self.contains(*c))
        .map(|(_, n)| n)
        .collect()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProviderAccount {
    pub id: String,
    pub email: String,
    pub plan: Option<String>,
    pub needs_login: bool,
    pub switchable: bool,
}

impl ProviderAccount {
    pub fn new(id: &str, email: &str, plan: Option<&str>) -> ProviderAccount {
        ProviderAccount {
            id: id.to_string(),
            email: email.to_string(),
            plan: plan.map(str::to_string),
            needs_login: false,
            switchable: true,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ProviderState {
    pub accounts: Vec<ProviderAccount>,
    pub active_id: Option<String>,
    pub busy: bool,
    pub adding: bool,
    pub sign_in_url: Option<String>,
    pub unlisted_active_title: Option<String>,
}

impl ProviderState {
    pub fn active(&self) -> Option<&ProviderAccount> {
        let id = self.active_id.as_deref()?;
        self.accounts.iter().find(|a| a.id == id)
    }

    pub fn active_name(&self) -> Option<String> {
        match self.active() {
            Some(a) => Some(a.email.clone()),
            None => self
                .active_id
                .as_ref()
                .and(self.unlisted_active_title.clone()),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Confirmation {
    pub title: String,
    pub message: String,
    pub button: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AccountError(String);

impl AccountError {
    pub fn new(message: &str) -> AccountError {
        AccountError(message.to_string())
    }

    pub fn message(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for AccountError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for AccountError {}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AddStarted {
    pub sign_in_url: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AddProgress {
    Idle,
    Waiting { sign_in_url: Option<String> },
    Done,
    Cancelled,
    Failed(AccountError),
}

pub trait AccountProvider: Send + Sync {
    fn id(&self) -> ProviderId;
    fn title(&self) -> &'static str;
    fn capabilities(&self) -> Capabilities;
    fn is_available(&self) -> bool {
        true
    }
    fn state(&self) -> ProviderState;
    fn sync(&self, force: bool);
    fn usage_report(&self, _account: &str) -> Option<UsageReport> {
        None
    }
    fn switch_to(&self, id: &str) -> Result<(), AccountError>;
    fn begin_add(&self) -> Result<AddStarted, AccountError>;
    fn poll_add(&self) -> AddProgress;
    fn cancel_add(&self);
    fn abandon_add(&self) -> Option<String> {
        None
    }
    fn remove(&self, id: &str);
    fn switch_confirmation(&self, _id: &str) -> Option<Confirmation> {
        None
    }
    fn add_confirmation(&self) -> Option<Confirmation> {
        None
    }
    fn switch_failure_title(&self) -> &'static str;
}

#[derive(Default)]
pub struct ProviderRegistry {
    providers: Vec<Arc<dyn AccountProvider>>,
}

impl ProviderRegistry {
    pub fn new() -> ProviderRegistry {
        ProviderRegistry::default()
    }

    pub fn register(&mut self, provider: Arc<dyn AccountProvider>) {
        if self.get(provider.id().as_str()).is_none() {
            self.providers.push(provider);
        }
    }

    pub fn providers(&self) -> &[Arc<dyn AccountProvider>] {
        &self.providers
    }

    pub fn get(&self, id: &str) -> Option<Arc<dyn AccountProvider>> {
        self.providers
            .iter()
            .find(|p| p.id().as_str() == id)
            .cloned()
    }

    pub fn sync_all(&self, force: bool) {
        for provider in &self.providers {
            provider.sync(force);
        }
    }
}
