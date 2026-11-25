use super::files::{self, Kept, Refused, Session};
use crate::accounts::procs::Apps;
use crate::providers::{
    AccountError, AccountProvider, AddProgress, AddStarted, Capabilities, Confirmation,
    ProviderAccount, ProviderId, ProviderState,
};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
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
    before_add: Option<Kept>,
    known_before_add: BTreeSet<String>,
    added: Option<String>,
    left: Option<Kept>,
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

fn plain(account: Option<&str>) -> Option<&str> {
    account.filter(|id| files::valid_uuid(id))
}

fn holds_login(live: &Option<Session>) -> bool {
    live.as_ref().is_some_and(|s| s.account.is_some())
}

fn listed(store_dir: &Path) -> BTreeSet<String> {
    std::fs::read_dir(store_dir)
        .map(|dir| {
            dir.flatten()
                .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
                .filter_map(|e| {
                    let name = e.file_name().to_string_lossy().into_owned();
                    let uuid = name.strip_suffix(".cookies")?;
                    files::valid_uuid(uuid).then(|| uuid.to_string())
                })
                .collect()
        })
        .unwrap_or_default()
}

fn left_note(left: &Kept) -> String {
    let named = plain(left.account.as_deref()).is_some();
    if named {
        "Claude Desktop was left signed out. Sign in there, or turn accounts on and switch to an account to put its login back."
    } else {
        "Claude Desktop was left signed out. Sign in there again."
    }
    .to_string()
}

impl DesktopStore {
    pub fn new(deps: DesktopDeps, paths: DesktopPaths) -> DesktopStore {
        let saved = listed(&paths.store_dir);
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
        files::saved(&self.paths.store_dir, uuid)
    }

    pub fn is_running(&self) -> bool {
        self.deps.apps.is_running(BUNDLE_ID)
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

    fn change(
        &self,
        to: Option<(&Path, Option<&str>)>,
        failed: &'static str,
    ) -> Result<Option<Kept>, AccountError> {
        let dirs = files::Dirs {
            claude: &self.paths.claude_dir,
            store: &self.paths.store_dir,
        };
        let was_running = self.quit()?;
        let result = self.require_closed().and_then(|()| {
            let changed = match to {
                Some((saved, account)) => files::restore(&dirs, saved, account),
                None => files::sign_out(&dirs),
            };
            changed.map_err(|refused| {
                AccountError::new(match refused {
                    Refused::Unsettled => "Claude Desktop hasn't finished saving its login, so it was left in place. Open Claude, quit it and try again.",
                    Refused::Unkept => "Couldn't save the Claude Desktop login in use, so it was left in place.",
                    Refused::Failed => failed,
                })
            })
        });
        if result.is_ok() || was_running {
            self.deps.apps.open(BUNDLE_ID);
        }
        let kept = result.inspect_err(|_| {
            let found = listed(&self.paths.store_dir);
            lock(&self.inner).saved.extend(found);
        })?;
        let mut inner = lock(&self.inner);
        let left = kept.as_ref().and_then(|kept| kept.account.as_deref());
        if let Some(id) = plain(left) {
            inner.saved.insert(id.to_string());
        }
        inner.active = plain(to.and_then(|(_, account)| account)).map(str::to_string);
        if to.is_some() {
            inner.left = None;
        }
        Ok(kept)
    }

    fn switch_locked(&self, uuid: &str) -> Result<(), AccountError> {
        let saved = self.mine(uuid);
        if !files::valid_uuid(uuid) || !files::is_copy(&saved) {
            return Err(AccountError::new(
                "Notchling has no saved desktop session for this account. Sign in to it in Claude once.",
            ));
        }
        self.change(
            Some((&saved, Some(uuid))),
            "Couldn't restore the saved desktop session.",
        )
        .map(|_| ())
    }

    fn end_add(&self) -> Option<Option<Kept>> {
        let mut inner = lock(&self.inner);
        if !inner.adding {
            return None;
        }
        inner.adding = false;
        Some(inner.before_add.take())
    }

    fn begin_add_locked(&self) -> Result<(), AccountError> {
        let kept = self.change(None, "Couldn't sign Claude Desktop out.")?;
        let mut inner = lock(&self.inner);
        inner.known_before_add = inner.saved.clone();
        inner.before_add = kept;
        inner.added = None;
        inner.left = None;
        inner.adding = true;
        Ok(())
    }

    fn show(&self, session: Option<Session>) {
        let named = session.and_then(|Session { account, cookies }| {
            Some((account.filter(|id| files::valid_uuid(id))?, cookies))
        });
        let Some((uuid, cookies)) = named else {
            lock(&self.inner).active = None;
            return;
        };
        let saved = files::save(&self.mine(&uuid), &cookies);
        let mut inner = lock(&self.inner);
        if saved {
            inner.saved.insert(uuid.clone());
        }
        inner.active = Some(uuid);
        inner.left = None;
    }

    fn signed_out(&self) -> bool {
        files::session(&self.paths.claude_dir).is_ok_and(|live| !holds_login(&live))
    }

    fn put_back(&self, before: &Kept) -> bool {
        files::is_copy(&before.copy)
            && self
                .change(
                    Some((&before.copy, before.account.as_deref())),
                    "Couldn't restore the saved desktop session.",
                )
                .is_ok()
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
        if lock(&self.inner).adding {
            return;
        }
        if let Ok(session) = files::session(&self.paths.claude_dir) {
            self.show(session);
        }
    }

    fn switch_to(&self, id: &str) -> Result<(), AccountError> {
        let _op = lock(&self.op);
        if lock(&self.inner).adding {
            return Err(AccountError::new(
                "Claude Desktop is waiting for a sign-in. Finish or cancel it first.",
            ));
        }
        lock(&self.inner).busy = true;
        let result = self.switch_locked(id);
        lock(&self.inner).busy = false;
        result
    }

    fn begin_add(&self) -> Result<AddStarted, AccountError> {
        let _op = lock(&self.op);
        if lock(&self.inner).adding {
            return Ok(AddStarted::default());
        }
        lock(&self.inner).busy = true;
        let result = self.begin_add_locked();
        lock(&self.inner).busy = false;
        result.map(|()| AddStarted::default())
    }

    fn poll_add(&self) -> AddProgress {
        let _op = lock(&self.op);
        if !lock(&self.inner).adding {
            return AddProgress::Idle;
        }
        let Some((uuid, cookies)) = files::live(&self.paths.claude_dir) else {
            return AddProgress::Waiting { sign_in_url: None };
        };
        let saved = files::valid_uuid(&uuid) && files::save(&self.mine(&uuid), &cookies);
        let mut inner = lock(&self.inner);
        if !inner.adding {
            return AddProgress::Cancelled;
        }
        inner.adding = false;
        inner.before_add = None;
        if !saved {
            return AddProgress::Failed(AccountError::new(
                "Couldn't save the new Claude Desktop login, so it wasn't added.",
            ));
        }
        inner.saved.insert(uuid.clone());
        inner.added = (!inner.known_before_add.contains(&uuid)).then(|| uuid.clone());
        inner.active = Some(uuid);
        AddProgress::Done
    }

    fn abandon_add(&self) -> Option<String> {
        let _op = lock(&self.op);
        let before = self.end_add().flatten();
        let left = before.or_else(|| lock(&self.inner).left.take())?;
        self.signed_out().then(|| left_note(&left))
    }

    fn added_account(&self) -> Option<Option<String>> {
        Some(lock(&self.inner).added.clone())
    }

    fn cancel_add(&self) {
        let _op = lock(&self.op);
        let Some(before) = self.end_add() else {
            return;
        };
        let live = files::session(&self.paths.claude_dir).ok().flatten();
        if holds_login(&live) {
            return self.show(live);
        }
        let Some(before) = before else {
            return;
        };
        lock(&self.inner).busy = true;
        let back = self.put_back(&before);
        lock(&self.inner).busy = false;
        if back {
            return;
        }
        match files::session(&self.paths.claude_dir) {
            Ok(live) if holds_login(&live) => self.show(live),
            Ok(_) => lock(&self.inner).left = Some(before),
            Err(_) => {}
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
