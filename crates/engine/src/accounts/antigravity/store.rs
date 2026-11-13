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

pub const SERVICE: &str = "Notchling Antigravity";
pub const LIVE_SERVICE: &str = "gemini";
pub const LIVE_ACCOUNT: &str = "antigravity";
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
    pub token_file: PathBuf,
    pub apps_dir: PathBuf,
    pub state_dir: PathBuf,
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

pub const USAGE_DOC: &str = "accounts/antigravity-usage";

#[derive(Default)]
struct Inner {
    accounts: Vec<Saved>,
    active: Option<String>,
    usage: HashMap<String, GroupedUsage>,
    busy: bool,
    adding: bool,
    before_add: Option<String>,
    last_saved: Option<String>,
}

pub struct AntigravityStore {
    deps: AntigravityDeps,
    paths: AntigravityPaths,
    inner: Mutex<Inner>,
    op: Mutex<()>,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

impl AntigravityStore {
    pub fn new(deps: AntigravityDeps, paths: AntigravityPaths) -> AntigravityStore {
        let accounts = book::read_accounts(&paths.book());
        let usage = crate::store::doc(&paths.data_dir, USAGE_DOC)
            .ok()
            .flatten()
            .map(|text| usage_from_json(&text))
            .unwrap_or_default();
        AntigravityStore {
            deps,
            paths,
            inner: Mutex::new(Inner {
                accounts,
                usage,
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

    pub fn is_running(&self) -> bool {
        BUNDLE_IDS.iter().any(|b| self.deps.apps.is_running(b))
    }

    pub fn idle_for_auto_switch(&self) -> bool {
        if self.is_running() {
            return false;
        }
        let Some(ps) = self.deps.procs.capture(
            "/bin/ps",
            &["-axo", "command="],
            std::time::Duration::from_secs(10),
        ) else {
            return false;
        };
        !agy_running(&ps)
    }

    pub fn switch_in_background(&self, id: &str) -> Result<(), AccountError> {
        let _op = lock(&self.op);
        lock(&self.inner).busy = true;
        let result = self.switch_locked(id, false);
        lock(&self.inner).busy = false;
        result
    }

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

    fn sign_out(&self) {
        self.deps.keychain.delete(LIVE_SERVICE, LIVE_ACCOUNT);
        let _ = std::fs::remove_file(&self.paths.token_file);
    }

    fn quit_apps(&self) -> Result<Vec<String>, AccountError> {
        let mut quit: Vec<String> = vec![];
        for bundle in BUNDLE_IDS {
            match self.deps.apps.quit(bundle) {
                Ok(true) => quit.push(bundle.to_string()),
                Ok(false) => {}
                Err(_) => {
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

    fn capture(&self) -> Option<String> {
        let Some(login) = self.read_login() else {
            lock(&self.inner).active = None;
            return None;
        };
        let mut account = login::account_in(&login)?;
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

    fn saved_copy(&self, id: &str) -> Result<Option<String>, KeychainError> {
        chunked::read(&Plain(self.deps.keychain.as_ref()), SERVICE, id)
    }

    fn save_copy(&self, id: &str, login: &str) -> Result<(), KeychainError> {
        chunked::write(&Plain(self.deps.keychain.as_ref()), SERVICE, id, login)
    }

    fn live_is_saved(&self, active: Option<&str>, refusal: &str) -> Result<(), AccountError> {
        let Some(current) = self.read_login() else {
            return Ok(());
        };
        match active.map(|id| self.saved_copy(id)) {
            Some(Ok(Some(saved))) if saved == current => Ok(()),
            _ => Err(AccountError::new(refusal)),
        }
    }

    fn fits_live_item(login: &str) -> Result<(), AccountError> {
        plan_write(LIVE_SERVICE, LIVE_ACCOUNT, &login::encode_keyring(login))
            .map(|_| ())
            .map_err(|e| {
                AccountError::new(&format!(
                    "This Antigravity login is too long for Notchling to put back in Antigravity's Keychain item, so nothing changed ({e})."
                ))
            })
    }

    fn switch_locked(&self, id: &str, open_closed: bool) -> Result<(), AccountError> {
        let active = self.capture();
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
        let active_now = lock(&self.inner).active.clone();
        if (open_closed && active_now.as_deref() != Some(id)) || !reopen.is_empty() {
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
            let text = usage_to_json(&inner.usage);
            if let Err(e) = crate::store::put_doc(&self.paths.data_dir, USAGE_DOC, &text) {
                eprintln!("antigravity: the limits read aren't kept ({e})");
            }
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

    fn switch_to(&self, id: &str) -> Result<(), AccountError> {
        let _op = lock(&self.op);
        lock(&self.inner).busy = true;
        let result = self.switch_locked(id, true);
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
        if self.read_login().is_none() {
            return AddProgress::Waiting { sign_in_url: None };
        }
        lock(&self.inner).adding = false;
        self.sync(false);
        AddProgress::Done
    }

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

pub fn agy_running(ps: &str) -> bool {
    ps.lines().any(|line| {
        line.split_whitespace()
            .next()
            .is_some_and(|program| program.rsplit('/').next() == Some("agy"))
    })
}

pub fn usage_to_json(usage: &HashMap<String, GroupedUsage>) -> String {
    let map: serde_json::Map<String, serde_json::Value> = usage
        .iter()
        .map(|(id, u)| {
            let groups: Vec<serde_json::Value> = u
                .groups
                .iter()
                .map(|g| {
                    let bars: Vec<serde_json::Value> = g
                        .bars
                        .iter()
                        .map(|(label, w)| {
                            serde_json::json!({
                                "label": label, "percent": w.percent, "resets_at": w.resets_at,
                            })
                        })
                        .collect();
                    serde_json::json!({"name": g.name, "bars": bars})
                })
                .collect();
            (
                id.clone(),
                serde_json::json!({"fetched": u.fetched, "groups": groups}),
            )
        })
        .collect();
    serde_json::Value::Object(map).to_string()
}

pub fn usage_from_json(text: &str) -> HashMap<String, GroupedUsage> {
    use crate::providers::usage::{GroupedGroup, UsageWindow};
    use serde_json::Value;
    let Ok(Value::Object(map)) = serde_json::from_str::<Value>(text) else {
        return HashMap::new();
    };
    map.into_iter()
        .filter_map(|(id, u)| {
            let groups = u
                .get("groups")?
                .as_array()?
                .iter()
                .filter_map(|g| {
                    let bars = g
                        .get("bars")?
                        .as_array()?
                        .iter()
                        .filter_map(|b| {
                            Some((
                                b.get("label")?.as_str()?.to_owned(),
                                UsageWindow {
                                    percent: b.get("percent")?.as_f64()?,
                                    resets_at: b.get("resets_at").and_then(Value::as_f64),
                                },
                            ))
                        })
                        .collect();
                    Some(GroupedGroup {
                        name: g.get("name")?.as_str()?.to_owned(),
                        bars,
                    })
                })
                .collect();
            let fetched = u.get("fetched")?.as_f64()?;
            Some((id, GroupedUsage { groups, fetched }))
        })
        .collect()
}
