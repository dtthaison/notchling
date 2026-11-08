//! Keeps a copy of every Codex CLI login it has seen and swaps them in on request (`CodexStore`).
//!
//! The Codex CLI, its background app server and the ChatGPT app's Codex all share one login,
//! `<CODEX_HOME>/auth.json` (ChatGPT OAuth tokens plus an id token naming the account). A switch
//! saves the live file back to its account's copy first, since Codex renews it in place, then
//! writes the other account's copy and restarts the app server so it reads it. Adding runs
//! `codex login` in a scratch `CODEX_HOME`, so the account in use stays signed in.
//!
//! Never renews a token. Saved copies are base64 in the Keychain item `Notchling Codex`, because
//! `security -w` prints a value containing newlines as hex; a real login is too long for one
//! `security -i` line, so it is split over several items ([`crate::accounts::chunked`]). A switch
//! writes `auth.json` only once the login it replaces reads back from its saved copy byte for byte.

use super::live::{auth_path, write_live, LOCK_WAIT};
use super::login::{self, Identity, RunningWork};
use crate::accounts::book::{self, Saved};
use crate::accounts::chunked::{self, Plain};
use crate::accounts::generic_password::{Keychain, KeychainError};
use crate::accounts::procs::{Apps, LoginChild, LoginSpawn, Procs};
use crate::providers::usage::{GroupedUsage, UsageReport};
use crate::providers::{
    AccountError, AccountProvider, AddProgress, AddStarted, Capabilities, Confirmation, HttpClient,
    HttpRequest, ProviderAccount, ProviderId, ProviderState, Trust,
};
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

/// The Keychain service the saved copies are under.
pub const SERVICE: &str = "Notchling Codex";
/// The ChatGPT app, which shares Codex's login.
pub const APP_BUNDLE_ID: &str = "com.openai.codex";
const USAGE_URL: &str = "https://chatgpt.com/backend-api/wham/usage";
/// A usage read for one account is repeated no sooner than this, unless forced.
const MIN_REFRESH_SECS: f64 = 120.0;
const SIGN_IN_PREFIX: &str = "https://auth.openai.com/";

pub type Clock = Arc<dyn Fn() -> f64 + Send + Sync>;

/// Everything the store reaches outside itself through.
#[derive(Clone)]
pub struct CodexDeps {
    pub keychain: Arc<dyn Keychain>,
    pub http: Arc<dyn HttpClient>,
    pub procs: Arc<dyn Procs>,
    pub apps: Arc<dyn Apps>,
    /// Seconds since 1970.
    pub clock: Clock,
}

/// Where things are.
#[derive(Clone, Debug)]
pub struct CodexPaths {
    /// `CODEX_HOME`, else `~/.codex`.
    pub home: PathBuf,
    /// The daemon's own folder for this store: the list of saved accounts and the scratch homes.
    pub state_dir: PathBuf,
}

impl CodexPaths {
    fn book(&self) -> PathBuf {
        self.state_dir.join("codex.json")
    }

    fn scratch_root(&self) -> PathBuf {
        self.state_dir.join("codex-login")
    }
}

struct AddFlow {
    child: Box<dyn LoginChild>,
    scratch: PathBuf,
    cancelled: bool,
}

#[derive(Default)]
struct Inner {
    accounts: Vec<Saved>,
    active: Option<String>,
    usage: HashMap<String, GroupedUsage>,
    fetched: HashMap<String, f64>,
    busy: bool,
    sign_in_url: Option<String>,
    /// The login as last saved, so an unchanged one isn't written to the Keychain every minute.
    last_saved: Option<Vec<u8>>,
    add: Option<AddFlow>,
}

pub struct CodexStore {
    deps: CodexDeps,
    paths: CodexPaths,
    inner: Mutex<Inner>,
    /// One operation that reads or changes the login at a time.
    op: Mutex<()>,
    binary: Mutex<Option<Option<PathBuf>>>,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

impl CodexStore {
    /// Loads the saved account names from the daemon's own file; touches nothing else.
    pub fn new(deps: CodexDeps, paths: CodexPaths) -> CodexStore {
        let accounts = book::read_accounts(&paths.book());
        CodexStore {
            deps,
            paths,
            inner: Mutex::new(Inner {
                accounts,
                ..Inner::default()
            }),
            op: Mutex::new(()),
            binary: Mutex::new(None),
        }
    }

    fn now(&self) -> f64 {
        (self.deps.clock)()
    }

    /// The codex command, looked up once (the menu asks every time it opens).
    fn codex_binary(&self) -> Option<PathBuf> {
        lock(&self.binary)
            .get_or_insert_with(|| self.deps.procs.find_binary("codex"))
            .clone()
    }

    /// What a switch would interrupt right now.
    pub fn running_work(&self) -> RunningWork {
        let ps = self
            .deps
            .procs
            .capture("/bin/ps", &["-axo", "command="], Duration::from_secs(10))
            .unwrap_or_default();
        login::running_work(&ps, self.deps.apps.is_running(APP_BUNDLE_ID))
    }

    // MARK: The login

    fn saved(&self, id: &str) -> Option<Vec<u8>> {
        self.saved_checked(id).ok().flatten()
    }

    /// The saved copy, put back together if it was split; a damaged one is an error.
    fn saved_checked(&self, id: &str) -> Result<Option<Vec<u8>>, KeychainError> {
        let Some(text) = chunked::read(&Plain(self.deps.keychain.as_ref()), SERVICE, id)? else {
            return Ok(None);
        };
        STANDARD
            .decode(text.trim())
            .map(Some)
            .map_err(|_| KeychainError::Damaged)
    }

    fn store(&self, login: &[u8], id: &str) -> Result<(), KeychainError> {
        if self.saved(id).as_deref() == Some(login) {
            return Ok(());
        }
        let keychain = Plain(self.deps.keychain.as_ref());
        chunked::write(&keychain, SERVICE, id, &STANDARD.encode(login))
    }

    /// Ok when the live login is safely saved (read back byte for byte) or there is none, so a
    /// switch may overwrite `auth.json`. Codex renews the login in place, so the live file can be
    /// the only copy of a working refresh token.
    fn live_is_saved(&self) -> Result<(), AccountError> {
        let refuse = |why: &str| {
            Err(AccountError::new(&format!(
                "Codex's current login couldn't be saved in the Keychain, so the account wasn't changed ({why})."
            )))
        };
        let live = match std::fs::read(auth_path(&self.paths.home)) {
            Ok(live) => live,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(_) => return refuse("auth.json couldn't be read"),
        };
        let Some(who) = login::identity(&live) else {
            return refuse("it names no ChatGPT account");
        };
        match self.saved_checked(&who.id) {
            Ok(Some(saved)) if saved == live => Ok(()),
            Ok(_) => refuse("the saved copy doesn't match it"),
            Err(e) => refuse(&e.to_string()),
        }
    }

    fn save_book(&self, accounts: &[Saved]) {
        book::write_accounts(&self.paths.book(), accounts);
    }

    /// Saves the live login under its account and says whose it is (`capture`). Holds the op lock.
    fn capture(&self) -> Option<Identity> {
        let live = std::fs::read(auth_path(&self.paths.home)).ok();
        let who = live.as_deref().and_then(login::identity);
        let (Some(live), Some(who)) = (live, who) else {
            lock(&self.inner).active = None;
            return None;
        };
        let unchanged = lock(&self.inner).last_saved.as_deref() == Some(live.as_slice());
        if !unchanged && self.store(&live, &who.id).is_ok() {
            lock(&self.inner).last_saved = Some(live);
        }
        let mut inner = lock(&self.inner);
        let before = inner.accounts.clone();
        match inner.accounts.iter_mut().find(|a| a.id == who.id) {
            Some(a) => {
                a.email = who.email.clone();
                if a.plan.is_none() {
                    a.plan = who.plan.clone();
                }
            }
            None => inner.accounts.push(Saved {
                id: who.id.clone(),
                email: who.email.clone(),
                plan: who.plan.clone(),
            }),
        }
        inner.active = Some(who.id.clone());
        if inner.accounts != before {
            self.save_book(&inner.accounts);
        }
        Some(who)
    }

    // MARK: Limits

    fn fetch_usage(&self, token: &login::Access) -> Option<Vec<u8>> {
        let request = HttpRequest {
            method: "GET",
            url: USAGE_URL.to_string(),
            headers: vec![
                ("Authorization".into(), format!("Bearer {}", token.token)),
                ("ChatGPT-Account-Id".into(), token.workspace.clone()),
                ("User-Agent".into(), "Notchling".into()),
            ],
            body: None,
            timeout: Duration::from_secs(10),
            trust: Trust::System,
        };
        let reply = self.deps.http.send(&request).ok()?;
        (reply.status == 200).then_some(reply.body)
    }

    fn read_limits(&self, force: bool) {
        let (accounts, active) = {
            let inner = lock(&self.inner);
            (inner.accounts.clone(), inner.active.clone())
        };
        for account in accounts {
            let now = self.now();
            if !force {
                if let Some(last) = lock(&self.inner).fetched.get(&account.id) {
                    if now - last < MIN_REFRESH_SECS {
                        continue;
                    }
                }
            }
            let login = if active.as_deref() == Some(account.id.as_str()) {
                std::fs::read(auth_path(&self.paths.home)).ok()
            } else {
                self.saved(&account.id)
            };
            let Some(access) = login.and_then(|l| login::access(&l, now)) else {
                continue;
            };
            let Some(data) = self.fetch_usage(&access) else {
                continue;
            };
            let (plan, usage) = login::usage(&data, now);
            let mut inner = lock(&self.inner);
            inner.fetched.insert(account.id.clone(), now);
            if let Some(u) = usage {
                inner.usage.insert(account.id.clone(), u);
            }
            if let Some(plan) = plan {
                if let Some(a) = inner.accounts.iter_mut().find(|a| a.id == account.id) {
                    if a.plan.as_ref() != Some(&plan) {
                        a.plan = Some(plan);
                        let accounts = inner.accounts.clone();
                        self.save_book(&accounts);
                    }
                }
            }
        }
    }

    // MARK: Adding

    fn finish_add(&self, flow: AddFlow, code: i32) -> AddProgress {
        let AddFlow {
            scratch, cancelled, ..
        } = flow;
        {
            let mut inner = lock(&self.inner);
            inner.sign_in_url = None;
        }
        let result = if cancelled {
            AddProgress::Cancelled
        } else {
            self.save_new_login(&scratch, code)
        };
        // The scratch home holds the new login; it is gone either way.
        let _ = std::fs::remove_dir_all(&scratch);
        if result == AddProgress::Done {
            self.sync(true);
        }
        result
    }

    fn save_new_login(&self, scratch: &std::path::Path, code: i32) -> AddProgress {
        let not_finished =
            || AddProgress::Failed(AccountError::new("Codex didn't finish signing in."));
        let data = match std::fs::read(auth_path(scratch)) {
            Ok(d) if code == 0 => d,
            _ => return not_finished(),
        };
        let Some(who) = login::identity(&data) else {
            return not_finished();
        };
        if let Err(e) = self.store(&data, &who.id) {
            return AddProgress::Failed(AccountError::new(&format!(
                "Couldn't save the new Codex login in the Keychain: {e}."
            )));
        }
        let mut inner = lock(&self.inner);
        let account = Saved {
            id: who.id,
            email: who.email,
            plan: who.plan,
        };
        match inner.accounts.iter_mut().find(|a| a.id == account.id) {
            Some(existing) => *existing = account,
            None => inner.accounts.push(account),
        }
        let accounts = inner.accounts.clone();
        self.save_book(&accounts);
        AddProgress::Done
    }

    fn scratch_dir(&self) -> PathBuf {
        static N: AtomicU32 = AtomicU32::new(0);
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        self.paths.scratch_root().join(format!(
            "{}-{}-{nanos}",
            std::process::id(),
            N.fetch_add(1, Ordering::SeqCst)
        ))
    }
}

/// The sign-in page `codex login` printed: the first `https://auth.openai.com/…`, up to the next
/// white space or control character (so a colour code after it isn't part of it).
pub fn sign_in_url(output: &str) -> Option<String> {
    let start = output.find(SIGN_IN_PREFIX)?;
    let url: String = output[start..]
        .chars()
        .take_while(|c| !c.is_whitespace() && !c.is_control())
        .collect();
    (url.len() > SIGN_IN_PREFIX.len()).then_some(url)
}

impl AccountProvider for CodexStore {
    fn id(&self) -> ProviderId {
        ProviderId::new(ProviderId::CODEX)
    }

    fn title(&self) -> &'static str {
        "Codex"
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities::ADD_ACCOUNT
            .with(Capabilities::USAGE)
            .with(Capabilities::AUTO_SWITCH)
    }

    fn is_available(&self) -> bool {
        !lock(&self.inner).accounts.is_empty()
            || auth_path(&self.paths.home).exists()
            || self.codex_binary().is_some()
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
            adding: inner.add.is_some(),
            sign_in_url: inner.sign_in_url.clone(),
            unlisted_active_title: None,
        }
    }

    /// Saves the live login, then reads limits for every account whose token is still good (at
    /// most every two minutes each, unless `force`).
    fn sync(&self, force: bool) {
        let _op = lock(&self.op);
        self.capture();
        self.read_limits(force);
    }

    fn usage_report(&self, account: &str) -> Option<UsageReport> {
        lock(&self.inner).usage.get(account).map(UsageReport::from)
    }

    fn switch_to(&self, id: &str) -> Result<(), AccountError> {
        let _op = lock(&self.op);
        lock(&self.inner).busy = true;
        let result = self.switch_locked(id);
        let mut inner = lock(&self.inner);
        inner.busy = false;
        inner.fetched.remove(id);
        drop(inner);
        if result.is_ok() {
            self.read_limits(false);
        }
        result
    }

    /// Runs `codex login` in a scratch home: the browser opens, the new login lands there and is
    /// saved as another account. The live login isn't touched.
    fn begin_add(&self) -> Result<AddStarted, AccountError> {
        let _op = lock(&self.op);
        {
            let inner = lock(&self.inner);
            if inner.add.is_some() {
                return Ok(AddStarted {
                    sign_in_url: inner.sign_in_url.clone(),
                });
            }
        }
        let codex = self.codex_binary().ok_or_else(|| {
            AccountError::new("Couldn't find the codex command. Install the Codex CLI first.")
        })?;
        let scratch = self.scratch_dir();
        notchling_platform::files::ensure_private_dir(&scratch)
            .map_err(|_| AccountError::new("Couldn't make a folder for the sign-in."))?;
        let spawn = LoginSpawn {
            program: codex,
            args: vec![
                "login".into(),
                "-c".into(),
                "cli_auth_credentials_store=\"file\"".into(),
            ],
            env: vec![("CODEX_HOME".into(), scratch.to_string_lossy().into_owned())],
            log: scratch.join("login.log"),
        };
        let child = match self.deps.procs.spawn_login(&spawn) {
            Ok(child) => child,
            Err(_) => {
                let _ = std::fs::remove_dir_all(&scratch);
                return Err(AccountError::new("Couldn't start codex login."));
            }
        };
        let mut inner = lock(&self.inner);
        inner.add = Some(AddFlow {
            child,
            scratch,
            cancelled: false,
        });
        inner.sign_in_url = None;
        Ok(AddStarted::default())
    }

    fn poll_add(&self) -> AddProgress {
        let mut inner = lock(&self.inner);
        let (output, code) = match inner.add.as_mut() {
            Some(flow) => (flow.child.output(), flow.child.exit_code()),
            None => return AddProgress::Idle,
        };
        if inner.sign_in_url.is_none() {
            inner.sign_in_url = sign_in_url(&output);
        }
        let Some(code) = code else {
            return AddProgress::Waiting {
                sign_in_url: inner.sign_in_url.clone(),
            };
        };
        let Some(flow) = inner.add.take() else {
            return AddProgress::Idle;
        };
        drop(inner);
        self.finish_add(flow, code)
    }

    fn cancel_add(&self) {
        if let Some(flow) = lock(&self.inner).add.as_mut() {
            flow.cancelled = true;
            flow.child.kill();
        }
    }

    /// Stops `codex login` and removes its scratch home, which holds the new login if one landed;
    /// the live login and the Keychain are not touched.
    fn abandon_add(&self) -> Option<String> {
        let flow = {
            let mut inner = lock(&self.inner);
            inner.sign_in_url = None;
            inner.add.take()
        };
        if let Some(mut flow) = flow {
            flow.child.kill();
            let _ = std::fs::remove_dir_all(&flow.scratch);
        }
        None
    }

    fn remove(&self, id: &str) {
        let _op = lock(&self.op);
        let _ = chunked::delete(&Plain(self.deps.keychain.as_ref()), SERVICE, id);
        let mut inner = lock(&self.inner);
        inner.accounts.retain(|a| a.id != id);
        inner.usage.remove(id);
        let accounts = inner.accounts.clone();
        self.save_book(&accounts);
    }

    fn switch_confirmation(&self, _id: &str) -> Option<Confirmation> {
        let warning = login::switch_warning(&self.running_work())?;
        Some(Confirmation {
            title: "Switch Codex Account?".into(),
            message: warning,
            button: "Switch".into(),
        })
    }

    fn switch_failure_title(&self) -> &'static str {
        "Couldn't switch Codex accounts"
    }
}

impl CodexStore {
    fn switch_locked(&self, id: &str) -> Result<(), AccountError> {
        self.capture();
        self.live_is_saved()?;
        let login = self
            .saved_checked(id)
            .map_err(|e| {
                AccountError::new(&format!(
                    "Couldn't read the saved login for this account, so nothing changed: {e}."
                ))
            })?
            .ok_or_else(|| {
                AccountError::new("Notchling has no saved login for this account. Add it again.")
            })?;
        // The ChatGPT app quits first and opens again, so it never reads a half-changed login.
        let reopen = self.deps.apps.quit(APP_BUNDLE_ID).map_err(|_| {
            AccountError::new(
                "The ChatGPT app didn't quit, so the Codex account wasn't changed. Quit it and try again.",
            )
        })?;
        let result = self.write_and_restart(&login, id);
        if reopen {
            self.deps.apps.open(APP_BUNDLE_ID);
        }
        result
    }

    fn write_and_restart(&self, login: &[u8], id: &str) -> Result<(), AccountError> {
        write_live(&self.paths.home, login, LOCK_WAIT)?;
        {
            let mut inner = lock(&self.inner);
            inner.last_saved = Some(login.to_vec());
            inner.active = Some(id.to_string());
        }
        if self.running_work().server {
            if let Some(codex) = self.codex_binary() {
                // Output to /dev/null: a daemon that keeps a pipe open would hang a read.
                let _ = self.deps.procs.run_null(
                    &codex,
                    &["app-server", "daemon", "restart"],
                    Duration::from_secs(30),
                );
            }
        }
        Ok(())
    }
}
