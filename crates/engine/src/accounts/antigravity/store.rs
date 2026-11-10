//! Keeps a copy of every Antigravity login it has seen and swaps them in on request
//! (`AntigravityStore`).
//!
//! Antigravity and Antigravity IDE share one login, in two places: its own go-keyring Keychain
//! item (service `gemini`, account `antigravity`, which it restores from at launch) and
//! `~/.gemini/jetski-standalone-oauth-token`, a mirror the standalone language server reads. A
//! switch must set both, and quit the apps first (they read the login at launch, and a running one
//! may renew it and write the old one back). The Keychain item already exists (Antigravity made
//! it), so it is updated in place, which keeps Antigravity's access to it. Google refresh tokens
//! don't rotate, so a saved copy keeps working after Antigravity has used it.
//!
//! Add Account deletes the item and the file so Antigravity starts at its sign-in screen; the next
//! login it saves is picked up. Our own copies are the Keychain items `Notchling Antigravity`,
//! one per account, holding the raw login JSON as the Swift app stored it (split over several items
//! when too long for one line, [`crate::accounts::chunked`]). A switch or an add changes
//! Antigravity's login only once the current one reads back from its saved copy exactly, and only
//! with a login short enough for Antigravity's own item, which is never split.

use super::login::{self, EMAIL_ID_PREFIX};
use super::quota;
use crate::accounts::book::{self, Saved};
use crate::accounts::chunked::{self, Plain};
use crate::accounts::generic_password::{plan_write, Keychain, KeychainError};
use crate::accounts::procs::{Apps, Procs};
use crate::docfile::DocFile;
use crate::providers::usage::{GroupedUsage, UsageReport};
use crate::providers::{
    AccountError, AccountProvider, AddProgress, AddStarted, Capabilities, Confirmation, HttpClient,
    ProviderAccount, ProviderId, ProviderState,
};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};

/// Our copies of each account's login.
pub const SERVICE: &str = "Notchling Antigravity";
/// Antigravity's own login, where it restores from at launch.
pub const LIVE_SERVICE: &str = "gemini";
pub const LIVE_ACCOUNT: &str = "antigravity";
/// Antigravity and Antigravity IDE.
pub const BUNDLE_IDS: [&str; 2] = ["com.google.antigravity", "com.google.antigravity-ide"];

pub type Clock = Arc<dyn Fn() -> f64 + Send + Sync>;

#[derive(Clone)]
pub struct AntigravityDeps {
    pub keychain: Arc<dyn Keychain>,
    pub http: Arc<dyn HttpClient>,
    pub procs: Arc<dyn Procs>,
    pub apps: Arc<dyn Apps>,
    pub clock: Clock,
}

#[derive(Clone, Debug)]
pub struct AntigravityPaths {
    /// `~/.gemini/jetski-standalone-oauth-token`.
    pub token_file: PathBuf,
    /// Where the Antigravity app would be installed (`/Applications`), for `is_available`.
    pub apps_dir: PathBuf,
    /// The daemon's own folder for this store: the file its list of saved accounts was before the
    /// database (`antigravity.json`, moved in once).
    pub state_dir: PathBuf,
    /// Notchling's data folder: the saved accounts are the `accounts/antigravity` document in the
    /// `notchling.db` there.
    pub data_dir: PathBuf,
}

impl AntigravityPaths {
    fn book(&self) -> DocFile {
        DocFile::new(
            &self.data_dir,
            book::ANTIGRAVITY_DOC,
            self.state_dir.join("antigravity.json"),
        )
    }
}

#[derive(Default)]
struct Inner {
    accounts: Vec<Saved>,
    active: Option<String>,
    usage: HashMap<String, GroupedUsage>,
    busy: bool,
    /// Antigravity was restarted at its sign-in screen; the next login it saves gets added.
    adding: bool,
    /// The account to put back if adding is cancelled.
    before_add: Option<String>,
    /// The login as last saved, so an unchanged one isn't written to the Keychain every minute.
    last_saved: Option<String>,
}

pub struct AntigravityStore {
    deps: AntigravityDeps,
    paths: AntigravityPaths,
    inner: Mutex<Inner>,
    /// One operation that reads or changes the login at a time.
    op: Mutex<()>,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

impl AntigravityStore {
    /// Loads the saved account names from the daemon's own list; touches nothing else.
    pub fn new(deps: AntigravityDeps, paths: AntigravityPaths) -> AntigravityStore {
        let accounts = book::read_accounts(&paths.book());
        AntigravityStore {
            deps,
            paths,
            inner: Mutex::new(Inner {
                accounts,
                ..Inner::default()
            }),
            op: Mutex::new(()),
        }
    }

    fn book(&self) -> DocFile {
        self.paths.book()
    }

    fn save_book(&self, accounts: &[Saved]) {
        book::write_accounts(&self.book(), accounts);
    }

    /// Antigravity is open, so a switch would restart it.
    pub fn is_running(&self) -> bool {
        BUNDLE_IDS.iter().any(|b| self.deps.apps.is_running(b))
    }

    // MARK: The login

    /// The login Antigravity would restore from: its Keychain item, else the file mirror.
    fn read_login(&self) -> Option<String> {
        if let Some(json) = self
            .deps
            .keychain
            .read(LIVE_SERVICE, LIVE_ACCOUNT)
            .and_then(|v| login::decode_keyring(&v))
        {
            return Some(json);
        }
        let data = std::fs::read(&self.paths.token_file).ok()?;
        if data.is_empty() {
            return None;
        }
        String::from_utf8(data).ok()
    }

    /// Sets both places Antigravity reads: its go-keyring Keychain item and the file the language
    /// server reads.
    fn write_login(&self, login: &str) -> Result<(), AccountError> {
        if let Err(e) =
            self.deps
                .keychain
                .write(LIVE_SERVICE, LIVE_ACCOUNT, &login::encode_keyring(login))
        {
            return Err(AccountError::new(&format!(
                "Couldn't update Antigravity's saved login: {e}."
            )));
        }
        // The file mirror is the older format (with `id_token`). A newer-format login doesn't fit
        // it, so drop the old account's file instead; Antigravity writes it again from the item.
        if !login::has_id_token(login) {
            let _ = std::fs::remove_file(&self.paths.token_file);
            return Ok(());
        }
        let failed = || AccountError::new("Couldn't update Antigravity's login file.");
        if let Some(dir) = self.paths.token_file.parent() {
            std::fs::create_dir_all(dir).map_err(|_| failed())?;
        }
        notchling_platform::files::write_private(&self.paths.token_file, login.as_bytes())
            .map_err(|_| failed())
    }

    /// Clears Antigravity's login so it starts at the sign-in screen.
    fn sign_out(&self) {
        self.deps.keychain.delete(LIVE_SERVICE, LIVE_ACCOUNT);
        let _ = std::fs::remove_file(&self.paths.token_file);
    }

    /// Quits whichever Antigravity apps are open and returns them, to open again afterwards.
    fn quit_apps(&self) -> Result<Vec<String>, AccountError> {
        let mut quit: Vec<String> = vec![];
        for bundle in BUNDLE_IDS {
            match self.deps.apps.quit(bundle) {
                Ok(true) => quit.push(bundle.to_string()),
                Ok(false) => {}
                Err(_) => {
                    // Whatever already quit is opened again, so a refusal leaves things as found.
                    self.open_apps(&quit);
                    return Err(AccountError::new(
                        "Antigravity didn't quit, so the account wasn't changed. Quit it and try again.",
                    ));
                }
            }
        }
        quit.sort();
        Ok(quit)
    }

    fn open_apps(&self, bundles: &[String]) {
        for bundle in bundles {
            self.deps.apps.open(bundle);
        }
    }

    /// Saves the login in place, if any, and says whose it is (`capture`). Holds the op lock.
    fn capture(&self) -> Option<String> {
        let Some(login) = self.read_login() else {
            lock(&self.inner).active = None;
            return None;
        };
        let mut account = login::account_in(&login)?;
        // The newer format names only the email: it's the saved account with that email, if any.
        if account.id.starts_with(EMAIL_ID_PREFIX) {
            if let Some(known) = lock(&self.inner)
                .accounts
                .iter()
                .find(|a| a.email.to_lowercase() == account.email.to_lowercase())
            {
                account = Saved {
                    id: known.id.clone(),
                    email: known.email.clone(),
                    plan: None,
                };
            }
        }
        let unchanged = lock(&self.inner).last_saved.as_deref() == Some(login.as_str());
        if !unchanged && self.save_copy(&account.id, &login).is_ok() {
            lock(&self.inner).last_saved = Some(login);
        }
        let mut inner = lock(&self.inner);
        let before = inner.accounts.clone();
        match inner.accounts.iter_mut().find(|a| a.id == account.id) {
            Some(a) => a.email = account.email.clone(),
            None => inner.accounts.push(account.clone()),
        }
        inner.active = Some(account.id.clone());
        if inner.accounts != before {
            self.save_book(&inner.accounts);
        }
        Some(account.id)
    }

    /// Our saved copy of an account's login, put back together if it was split.
    fn saved_copy(&self, id: &str) -> Result<Option<String>, KeychainError> {
        chunked::read(&Plain(self.deps.keychain.as_ref()), SERVICE, id)
    }

    fn save_copy(&self, id: &str, login: &str) -> Result<(), KeychainError> {
        chunked::write(&Plain(self.deps.keychain.as_ref()), SERVICE, id, login)
    }

    /// Ok when Antigravity has no login, or ours reads back from the account's saved copy exactly:
    /// only then may its login be replaced or signed out.
    fn live_is_saved(&self, active: Option<&str>, refusal: &str) -> Result<(), AccountError> {
        let Some(current) = self.read_login() else {
            return Ok(());
        };
        match active.map(|id| self.saved_copy(id)) {
            Some(Ok(Some(saved))) if saved == current => Ok(()),
            _ => Err(AccountError::new(refusal)),
        }
    }

    /// Antigravity's own item takes one `security -i` line (it is never split: Antigravity reads
    /// it), so a login too long for it is refused before anything quits or changes.
    fn fits_live_item(login: &str) -> Result<(), AccountError> {
        plan_write(LIVE_SERVICE, LIVE_ACCOUNT, &login::encode_keyring(login))
            .map(|_| ())
            .map_err(|e| {
                AccountError::new(&format!(
                    "This Antigravity login is too long for Notchling to put back in Antigravity's Keychain item, so nothing changed ({e})."
                ))
            })
    }

    fn switch_locked(&self, id: &str) -> Result<(), AccountError> {
        let active = self.capture();
        // Already in use: the live login may be newer than the saved copy (renewed), so keep it.
        if active.as_deref() == Some(id) {
            return Ok(());
        }
        self.live_is_saved(
            active.as_deref(),
            "Couldn't save the current Antigravity login, so the account wasn't changed.",
        )?;
        let login = self
            .saved_copy(id)
            .map_err(|e| {
                AccountError::new(&format!(
                    "Couldn't read the saved login for this account, so nothing changed: {e}."
                ))
            })?
            .ok_or_else(|| {
                AccountError::new("Notchling has no saved login for this account. Add it again.")
            })?;
        Self::fits_live_item(&login)?;
        let mut reopen: Vec<String> = vec![];
        let result = self.quit_apps().and_then(|quit| {
            reopen = quit;
            self.write_login(&login)
        });
        if result.is_ok() {
            let mut inner = lock(&self.inner);
            inner.last_saved = Some(login);
            inner.active = Some(id.to_string());
        }
        // Always end with Antigravity open on the switched-to account, even if it wasn't running.
        let active_now = lock(&self.inner).active.clone();
        if active_now.as_deref() != Some(id) || !reopen.is_empty() {
            let open = if reopen.is_empty() {
                vec![BUNDLE_IDS[0].to_string()]
            } else {
                reopen
            };
            self.open_apps(&open);
        }
        result
    }
}

impl AccountProvider for AntigravityStore {
    fn id(&self) -> ProviderId {
        ProviderId::new(ProviderId::ANTIGRAVITY)
    }

    fn title(&self) -> &'static str {
        "Antigravity"
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities::ADD_ACCOUNT
            .with(Capabilities::RESTARTS_APP)
            .with(Capabilities::USAGE)
    }

    /// Installed, or used before: otherwise the menu leaves Antigravity out.
    fn is_available(&self) -> bool {
        !lock(&self.inner).accounts.is_empty()
            || self.paths.token_file.exists()
            || self.paths.apps_dir.join("Antigravity.app").exists()
    }

    fn state(&self) -> ProviderState {
        let inner = lock(&self.inner);
        ProviderState {
            accounts: inner
                .accounts
                .iter()
                .map(|a| ProviderAccount::new(&a.id, &a.email, a.plan.as_deref()))
                .collect(),
            active_id: inner.active.clone(),
            busy: inner.busy,
            adding: inner.adding,
            sign_in_url: None,
            unlisted_active_title: None,
        }
    }

    /// Reads the current login and saves it, then reads the active account's limits if Antigravity
    /// is running.
    fn sync(&self, _force: bool) {
        let _op = lock(&self.op);
        self.capture();
        let (active, email) = {
            let inner = lock(&self.inner);
            let email = inner
                .active
                .as_ref()
                .and_then(|id| inner.accounts.iter().find(|a| &a.id == id))
                .map(|a| a.email.clone());
            (inner.active.clone(), email)
        };
        let Some(id) = active else {
            return;
        };
        let now = (self.deps.clock)();
        let Some(snapshot) = quota::fetch(
            self.deps.procs.as_ref(),
            self.deps.http.as_ref(),
            email.as_deref(),
            now,
        ) else {
            return;
        };
        let mut inner = lock(&self.inner);
        if let Some(usage) = snapshot.usage {
            inner.usage.insert(id.clone(), usage);
        }
        if let Some(plan) = snapshot.plan {
            if let Some(a) = inner.accounts.iter_mut().find(|a| a.id == id) {
                a.plan = Some(plan);
                let accounts = inner.accounts.clone();
                self.save_book(&accounts);
            }
        }
    }

    fn usage_report(&self, account: &str) -> Option<UsageReport> {
        lock(&self.inner).usage.get(account).map(UsageReport::from)
    }

    /// Quits Antigravity, puts the saved login in place and opens it again.
    fn switch_to(&self, id: &str) -> Result<(), AccountError> {
        let _op = lock(&self.op);
        lock(&self.inner).busy = true;
        let result = self.switch_locked(id);
        lock(&self.inner).busy = false;
        result
    }

    /// Saves the current login, then restarts Antigravity signed out so the user can sign in to
    /// another account. That login is saved as soon as it shows up.
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
        if self.read_login().is_none() {
            return AddProgress::Waiting { sign_in_url: None };
        }
        lock(&self.inner).adding = false;
        self.sync(false);
        AddProgress::Done
    }

    /// Stops waiting and, if nobody signed in yet, puts the previous login back.
    fn cancel_add(&self) {
        let previous = {
            let mut inner = lock(&self.inner);
            inner.adding = false;
            inner.before_add.take()
        };
        if let Some(previous) = previous {
            if self.read_login().is_none() {
                let _ = self.switch_to(&previous);
            }
        }
    }

    /// Stops waiting without putting the previous login back (that would write Antigravity's item).
    fn abandon_add(&self) -> Option<String> {
        let mut inner = lock(&self.inner);
        inner.adding = false;
        inner.before_add.take().map(|_| {
            "Antigravity was left at its sign-in screen. Sign in there, or turn accounts on and switch to an account to put its login back.".to_string()
        })
    }

    fn remove(&self, id: &str) {
        let _op = lock(&self.op);
        let _ = chunked::delete(&Plain(self.deps.keychain.as_ref()), SERVICE, id);
        let mut inner = lock(&self.inner);
        inner.accounts.retain(|a| a.id != id);
        let accounts = inner.accounts.clone();
        self.save_book(&accounts);
    }

    fn switch_confirmation(&self, _id: &str) -> Option<Confirmation> {
        self.is_running().then(|| Confirmation {
            title: "Restart Antigravity?".into(),
            message:
                "Antigravity quits and opens again on the other account. Agents running in it stop."
                    .into(),
            button: "Restart".into(),
        })
    }

    fn switch_failure_title(&self) -> &'static str {
        "Couldn't switch Antigravity accounts"
    }
}

impl AntigravityStore {
    fn begin_add_locked(&self) -> Result<(), AccountError> {
        let active = self.capture();
        // The item and the file go away next; make sure the copy really is in the Keychain first,
        // and that a cancel could put it back.
        self.live_is_saved(
            active.as_deref(),
            "Couldn't save the current Antigravity login, so it was left in place.",
        )?;
        if let Some(current) = self.read_login() {
            Self::fits_live_item(&current)?;
        }
        let reopen = self.quit_apps()?;
        self.sign_out();
        let open = if reopen.is_empty() {
            vec![BUNDLE_IDS[0].to_string()]
        } else {
            reopen
        };
        let mut inner = lock(&self.inner);
        inner.last_saved = None;
        inner.before_add = active;
        inner.active = None;
        inner.adding = true;
        drop(inner);
        self.open_apps(&open);
        Ok(())
    }
}
