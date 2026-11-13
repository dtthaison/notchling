use super::files;
use crate::accounts::procs::Apps;
use crate::providers::{
    AccountError, AccountProvider, AddProgress, AddStarted, Capabilities, Confirmation,
    ProviderAccount, ProviderId, ProviderState,
};
use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};

pub const BUNDLE_ID: &str = "com.anthropic.claudefordesktop";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClaudeName {
    pub id: String,
    pub email: String,
}

pub type ClaudeAccounts = Arc<dyn Fn() -> Vec<ClaudeName> + Send + Sync>;

#[derive(Clone)]
pub struct DesktopDeps {
    pub apps: Arc<dyn Apps>,
    pub claude_accounts: ClaudeAccounts,
}

#[derive(Clone, Debug)]
pub struct DesktopPaths {
    pub claude_dir: PathBuf,
    pub store_dir: PathBuf,
    pub apps_dir: PathBuf,
}

#[derive(Default)]
struct Inner {
    active: Option<String>,
    saved: BTreeSet<String>,
    busy: bool,
    adding: bool,
    before_add: Option<String>,
}

pub struct DesktopStore {
    deps: DesktopDeps,
    paths: DesktopPaths,
    inner: Mutex<Inner>,
    op: Mutex<()>,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

fn uuid_of(account_id: &str) -> &str {
    account_id.split(':').next().unwrap_or(account_id)
}

impl DesktopStore {
    pub fn new(deps: DesktopDeps, paths: DesktopPaths) -> DesktopStore {
        let saved = std::fs::read_dir(&paths.store_dir)
            .map(|dir| {
                dir.flatten()
                    .filter_map(|e| {
                        let name = e.file_name().to_string_lossy().into_owned();
                        let uuid = name.strip_suffix(".cookies")?;
                        files::valid_uuid(uuid).then(|| uuid.to_string())
                    })
                    .collect()
            })
            .unwrap_or_default();
        DesktopStore {
            deps,
            paths,
            inner: Mutex::new(Inner {
                saved,
                ..Inner::default()
            }),
            op: Mutex::new(()),
        }
    }

    fn mine(&self, uuid: &str) -> PathBuf {
        self.paths.store_dir.join(format!("{uuid}.cookies"))
    }

    pub fn is_running(&self) -> bool {
        self.deps.apps.is_running(BUNDLE_ID)
    }

    fn capture(&self, uuid: &str) -> bool {
        files::valid_uuid(uuid) && files::capture(&self.paths.claude_dir, &self.mine(uuid))
    }

    fn quit(&self) -> Result<bool, AccountError> {
        self.deps.apps.quit(BUNDLE_ID).map_err(|_| {
            AccountError::new(
                "Claude didn't quit, so the desktop account wasn't changed. Quit it and try again.",
            )
        })
    }

    fn require_closed(&self) -> Result<(), AccountError> {
        if self.is_running() {
            return Err(AccountError::new(
                "Claude is open, so the desktop account wasn't changed. Quit it and try again.",
            ));
        }
        Ok(())
    }

    fn switch_locked(&self, uuid: &str) -> Result<(), AccountError> {
        let saved = self.mine(uuid);
        if !files::valid_uuid(uuid) || !saved.exists() {
            return Err(AccountError::new(
                "Notchling has no saved desktop session for this account. Sign in to it in Claude once.",
            ));
        }
        let was_running = self.quit()?;
        let result = self.require_closed().and_then(|()| {
            files::restore(&self.paths.claude_dir, &saved, uuid)
                .map_err(|_| AccountError::new("Couldn't restore the saved desktop session."))
        });
        if result.is_ok() || was_running {
            self.deps.apps.open(BUNDLE_ID);
        }
        if result.is_ok() {
            lock(&self.inner).active = Some(uuid.to_string());
        }
        result
    }

    fn begin_add_locked(&self) -> Result<(), AccountError> {
        let current = files::active_account(&self.paths.claude_dir);
        if let Some(current) = &current {
            self.capture(current);
        }
        let was_running = self.quit()?;
        let result = self.require_closed().and_then(|()| {
            files::sign_out(&self.paths.claude_dir)
                .map_err(|_| AccountError::new("Couldn't sign Claude Desktop out."))
        });
        if result.is_ok() || was_running {
            self.deps.apps.open(BUNDLE_ID);
        }
        result?;
        let mut inner = lock(&self.inner);
        if let Some(current) = &current {
            if files::valid_uuid(current) && self.mine(current).exists() {
                inner.saved.insert(current.clone());
            }
        }
        inner.before_add = current;
        inner.active = None;
        inner.adding = true;
        Ok(())
    }
}

impl AccountProvider for DesktopStore {
    fn id(&self) -> ProviderId {
        ProviderId::new(ProviderId::CLAUDE_DESKTOP)
    }

    fn title(&self) -> &'static str {
        "Claude Desktop"
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities::ADD_ACCOUNT.with(Capabilities::RESTARTS_APP)
    }

    fn is_available(&self) -> bool {
        !lock(&self.inner).saved.is_empty() || self.paths.apps_dir.join("Claude.app").exists()
    }

    fn state(&self) -> ProviderState {
        let inner = lock(&self.inner);
        let accounts = (self.deps.claude_accounts)()
            .into_iter()
            .filter_map(|account| {
                let uuid = uuid_of(&account.id).to_string();
                let active = inner.active.as_deref() == Some(uuid.as_str());
                let saved = inner.saved.contains(&uuid);
                if !(active || saved) {
                    return None;
                }
                Some(ProviderAccount {
                    id: uuid,
                    email: account.email,
                    plan: None,
                    needs_login: false,
                    switchable: saved,
                })
            })
            .collect();
        ProviderState {
            accounts,
            active_id: inner.active.clone(),
            busy: inner.busy,
            adding: inner.adding,
            sign_in_url: None,
            unlisted_active_title: Some("Signed in".into()),
        }
    }

    fn sync(&self, _force: bool) {
        let _op = lock(&self.op);
        let Some(uuid) = files::active_account(&self.paths.claude_dir) else {
            lock(&self.inner).active = None;
            return;
        };
        let captured = self.capture(&uuid);
        let mut inner = lock(&self.inner);
        if captured {
            inner.saved.insert(uuid.clone());
        }
        inner.active = Some(uuid);
    }

    fn switch_to(&self, id: &str) -> Result<(), AccountError> {
        let _op = lock(&self.op);
        lock(&self.inner).busy = true;
        let result = self.switch_locked(id);
        lock(&self.inner).busy = false;
        result
    }

    fn begin_add(&self) -> Result<AddStarted, AccountError> {
        let _op = lock(&self.op);
        lock(&self.inner).busy = true;
        let result = self.begin_add_locked();
        lock(&self.inner).busy = false;
        result.map(|()| AddStarted::default())
    }

    fn poll_add(&self) -> AddProgress {
        if !lock(&self.inner).adding {
            return AddProgress::Idle;
        }
        let signed_in = files::active_account(&self.paths.claude_dir).is_some()
            && std::fs::read(files::cookies(&self.paths.claude_dir))
                .map(|d| !d.is_empty())
                .unwrap_or(false);
        if !signed_in {
            return AddProgress::Waiting { sign_in_url: None };
        }
        lock(&self.inner).adding = false;
        self.sync(false);
        AddProgress::Done
    }

    fn abandon_add(&self) -> Option<String> {
        let mut inner = lock(&self.inner);
        inner.adding = false;
        inner.before_add.take().map(|_| {
            "Claude Desktop was left signed out. Sign in there, or turn accounts on and switch to an account to put its login back.".to_string()
        })
    }

    fn cancel_add(&self) {
        let previous = {
            let mut inner = lock(&self.inner);
            inner.adding = false;
            inner.before_add.take()
        };
        if let Some(previous) = previous {
            if files::active_account(&self.paths.claude_dir).is_none() {
                let _ = self.switch_to(&previous);
            }
        }
    }

    fn remove(&self, id: &str) {
        let _op = lock(&self.op);
        if files::valid_uuid(id) {
            let _ = std::fs::remove_file(self.mine(id));
        }
        lock(&self.inner).saved.remove(id);
    }

    fn switch_confirmation(&self, _id: &str) -> Option<Confirmation> {
        self.is_running().then(|| Confirmation {
            title: "Restart Claude Desktop?".into(),
            message: "Claude Desktop quits and opens again on the other account.".into(),
            button: "Restart".into(),
        })
    }

    fn add_confirmation(&self) -> Option<Confirmation> {
        self.is_running().then(|| Confirmation {
            title: "Open Claude Desktop signed out?".into(),
            message: "Claude Desktop quits and reopens at the sign-in screen. Sign in to the account you want to add; Notchling saves it. Your current account is kept and you can switch back to it.".into(),
            button: "Continue".into(),
        })
    }

    fn switch_failure_title(&self) -> &'static str {
        "Couldn't switch Claude Desktop"
    }
}
