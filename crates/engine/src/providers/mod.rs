//! The account providers (E8b, `Providers/*.swift`): one source of accounts the user can switch
//! between. Codex, Antigravity and Claude Desktop live in [`crate::accounts`]; Claude Code's
//! adapter is E8a's. The registry lists them in the order the Accounts window shows them.
//!
//! Built and switched off: nothing here runs until the daemon's `accounts` switch is on. The
//! providers do not decide that; the daemon's ops check it before they call in.

pub mod http;
pub mod http_real;
pub mod usage;

#[cfg(test)]
mod tests;

pub use http::{FakeHttp, HttpClient, HttpRequest, HttpResponse, Trust};
pub use usage::{Bar, Group, GroupedGroup, GroupedUsage, Kind, UsageReport, UsageWindow};

use std::fmt;
use std::sync::Arc;

/// A provider's stable name: the Accounts window's tab and what ops ask for.
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

/// What a provider can do (`ProviderCapabilities`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Capabilities(u8);

impl Capabilities {
    pub const NONE: Capabilities = Capabilities(0);
    /// Accounts can be added from Notchling.
    pub const ADD_ACCOUNT: Capabilities = Capabilities(1);
    /// A switch quits and reopens an app.
    pub const RESTARTS_APP: Capabilities = Capabilities(1 << 1);
    /// Reports limits ([`AccountProvider::usage_report`]).
    pub const USAGE: Capabilities = Capabilities(1 << 2);
    /// The limit watcher may switch it when a limit is hit.
    pub const AUTO_SWITCH: Capabilities = Capabilities(1 << 3);
    /// Token cost is tracked for it.
    pub const COST: Capabilities = Capabilities(1 << 4);

    pub const fn with(self, other: Capabilities) -> Capabilities {
        Capabilities(self.0 | other.0)
    }

    pub fn contains(self, other: Capabilities) -> bool {
        self.0 & other.0 == other.0
    }

    /// The names, for the wire.
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
    /// "Pro", "Max 5x"…; `None` when the provider has none to show.
    pub plan: Option<String>,
    /// The saved login was refused; the user has to sign in again.
    pub needs_login: bool,
    /// There is something saved to switch to (Desktop lists accounts it can't switch to yet).
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
    /// Switching or adding is under way.
    pub busy: bool,
    /// An add flow is waiting for the user (a browser sign-in, an app at its sign-in screen).
    pub adding: bool,
    /// The sign-in page `codex login` printed.
    pub sign_in_url: Option<String>,
    /// Names the active account when it isn't among `accounts` (Desktop on an account Notchling
    /// has no Claude Code login for).
    pub unlisted_active_title: Option<String>,
}

impl ProviderState {
    pub fn active(&self) -> Option<&ProviderAccount> {
        let id = self.active_id.as_deref()?;
        self.accounts.iter().find(|a| a.id == id)
    }

    /// The active account's name for a menu row; `None` when signed out.
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

/// A question to answer before something that interrupts work.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Confirmation {
    pub title: String,
    pub message: String,
    pub button: String,
}

/// A failure the user can read. Always built from fixed text: a login, a token or a cookie is
/// never put in one.
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

/// What starting an add flow tells the caller.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AddStarted {
    /// Codex prints its sign-in page a moment after it starts; it is in the state once known.
    pub sign_in_url: Option<String>,
}

/// Where an add flow is, as [`AccountProvider::poll_add`] finds it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AddProgress {
    /// No flow is under way.
    Idle,
    /// Waiting for the user.
    Waiting { sign_in_url: Option<String> },
    /// The new account was saved.
    Done,
    /// The user cancelled.
    Cancelled,
    Failed(AccountError),
}

/// One source of accounts (`AccountProvider`). The implementations wrap the stores and add no
/// logic of their own: the stores keep the account-switching code, which touches the user's real
/// logins.
///
/// Calls block; the daemon runs the slow ones off its connection thread.
pub trait AccountProvider: Send + Sync {
    fn id(&self) -> ProviderId;
    /// The tab and menu header.
    fn title(&self) -> &'static str;
    fn capabilities(&self) -> Capabilities;
    /// Installed or used before; otherwise the menu leaves it out.
    fn is_available(&self) -> bool {
        true
    }
    /// What is known now: the saved names and the last read of the live login. Reads no login.
    fn state(&self) -> ProviderState;
    /// Reads the live login and refreshes limits. Providers with no cheaper read ignore `force`.
    fn sync(&self, force: bool);
    fn usage_report(&self, _account: &str) -> Option<UsageReport> {
        None
    }
    fn switch_to(&self, id: &str) -> Result<(), AccountError>;
    /// Starts the add flow; progress is read with [`AccountProvider::poll_add`].
    fn begin_add(&self) -> Result<AddStarted, AccountError>;
    fn poll_add(&self) -> AddProgress;
    fn cancel_add(&self);
    /// Forgets a saved login. The provider stays signed in if it is the one in use.
    fn remove(&self, id: &str);
    /// Asked before `switch_to`: `None` when nothing would be interrupted.
    fn switch_confirmation(&self, _id: &str) -> Option<Confirmation> {
        None
    }
    /// Asked before `begin_add`.
    fn add_confirmation(&self) -> Option<Confirmation> {
        None
    }
    fn switch_failure_title(&self) -> &'static str;
}

/// The providers, in the order the Accounts window and the menu show them.
#[derive(Default)]
pub struct ProviderRegistry {
    providers: Vec<Arc<dyn AccountProvider>>,
}

impl ProviderRegistry {
    pub fn new() -> ProviderRegistry {
        ProviderRegistry::default()
    }

    /// A second provider with the same id is ignored.
    pub fn register(&mut self, provider: Arc<dyn AccountProvider>) {
        if self.get(provider.id().as_str()).is_none() {
            self.providers.push(provider);
        }
    }

    pub fn providers(&self) -> &[Arc<dyn AccountProvider>] {
        &self.providers
    }

    pub fn get(&self, id: &str) -> Option<Arc<dyn AccountProvider>> {
        self.providers.iter().find(|p| p.id().as_str() == id).cloned()
    }

    pub fn sync_all(&self, force: bool) {
        for provider in &self.providers {
            provider.sync(force);
        }
    }
}
