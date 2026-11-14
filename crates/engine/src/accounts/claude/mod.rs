mod config;
mod events;
mod lanes_side;
pub mod profile;
pub mod signin;
mod tokens;

pub use events::SwitchWhy;
pub use lanes_side::MoveAllAccounts;
pub use tokens::Renewal;

use crate::accounts::http::{HttpClient, HttpRequest, PROFILE_URL};
use crate::accounts::json_splice::set_top_level;
use crate::accounts::keychain::Keys;
use crate::accounts::{Login, Secret};
use crate::cost::account_log::AccountSwitches;
use crate::docfile::DocFile;
use crate::lanes::LaneStore;
use notchling_events::kinds::accounts::LoginAction;
use notchling_events::kinds::choices::DoneOrFailed;
use notchling_events::Emit;
use profile::{
    access_token, account_id_from_config, account_id_from_profile, is_expired,
    oauth_account_from_profile, plan_name, valid_id,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};

pub const LIVE_SERVICE: &str = "Claude Code-credentials";
pub const SAVED_SERVICE: &str = "Notchling Accounts";
pub const LIST_DOC: &str = "daemon/accounts";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClaudeAccount {
    pub id: String,
    pub email: String,
    pub plan: String,
    #[serde(rename = "needsLogin", default)]
    pub needs_login: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct Snapshot {
    pub accounts: Vec<ClaudeAccount>,
    pub active: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AccountError(pub String);

impl std::fmt::Display for AccountError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

pub(crate) fn err(message: impl Into<String>) -> AccountError {
    AccountError(message.into())
}

impl From<crate::accounts::keychain::KeychainError> for AccountError {
    fn from(e: crate::accounts::keychain::KeychainError) -> Self {
        AccountError(e.to_string())
    }
}

impl From<crate::lanes::LaneError> for AccountError {
    fn from(e: crate::lanes::LaneError) -> Self {
        AccountError(e.to_string())
    }
}

pub struct ClaudeEnv {
    pub keys: Keys,
    pub undo_keys: Keys,
    pub http: Arc<dyn HttpClient>,
    pub now_ms: Arc<dyn Fn() -> i64 + Send + Sync>,
    pub config_file: PathBuf,
    pub data_dir: PathBuf,
    pub list_file: PathBuf,
    pub lanes: LaneStore,
    pub events: Arc<dyn Emit>,
    pub history: Arc<dyn AccountSwitches>,
}

#[derive(Default)]
struct State {
    active: Option<String>,
    logged: Option<String>,
    known: HashMap<[u8; 32], String>,
}

pub struct ClaudeAccounts {
    env: ClaudeEnv,
    state: Mutex<State>,
    seen: Mutex<Option<String>>,
}

struct Op<'a> {
    state: MutexGuard<'a, State>,
    seen: &'a Mutex<Option<String>>,
}

impl std::ops::Deref for Op<'_> {
    type Target = State;
    fn deref(&self) -> &State {
        &self.state
    }
}

impl std::ops::DerefMut for Op<'_> {
    fn deref_mut(&mut self) -> &mut State {
        &mut self.state
    }
}

impl Drop for Op<'_> {
    fn drop(&mut self) {
        *self.seen.lock().unwrap_or_else(|e| e.into_inner()) = self.state.active.clone();
    }
}

impl ClaudeAccounts {
    pub fn new(env: ClaudeEnv) -> ClaudeAccounts {
        ClaudeAccounts {
            env,
            state: Mutex::new(State::default()),
            seen: Mutex::new(None),
        }
    }

    pub fn env(&self) -> &ClaudeEnv {
        &self.env
    }

    fn lock(&self) -> Op<'_> {
        Op {
            state: self.state.lock().unwrap_or_else(|e| e.into_inner()),
            seen: &self.seen,
        }
    }

    fn now(&self) -> i64 {
        (self.env.now_ms)()
    }

    fn note_active(&self, state: &mut State, why: &SwitchWhy) {
        let Some(active) = state.active.clone() else {
            return;
        };
        if state.logged.is_none() {
            state.logged = self.env.history.switches().pop().map(|entry| entry.id);
        }
        if state.logged.as_deref() == Some(active.as_str()) {
            return;
        }
        self.push_switch(state.logged.as_deref(), &active, why);
        state.logged = Some(active);
    }

    fn list_doc(&self) -> DocFile {
        DocFile::new(&self.env.data_dir, LIST_DOC, self.env.list_file.clone())
    }

    pub fn saved_list(&self) -> Vec<ClaudeAccount> {
        self.list_doc()
            .read()
            .ok()
            .flatten()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default()
    }

    fn save_list(&self, list: &[ClaudeAccount]) -> Result<(), AccountError> {
        let text = serde_json::to_string(list).map_err(|e| err(e.to_string()))?;
        self.list_doc()
            .put(
                |old| serde_json::from_str::<Vec<ClaudeAccount>>(old).is_ok(),
                &text,
            )
            .map_err(|e| err(format!("Couldn't save the accounts list: {e}")))
    }

    pub fn snapshot(&self) -> Snapshot {
        Snapshot {
            accounts: self.saved_list(),
            active: self.seen.lock().unwrap_or_else(|e| e.into_inner()).clone(),
        }
    }

    pub fn sync(&self) -> Result<Snapshot, AccountError> {
        let mut state = self.lock();
        let mut list = self.saved_list();
        self.capture(&mut state, &mut list)?;
        self.save_list(&list)?;
        Ok(Snapshot {
            accounts: list,
            active: state.active.clone(),
        })
    }

    pub fn switch_to(&self, id: &str) -> Result<Snapshot, AccountError> {
        self.switch_for(id, &SwitchWhy::USER)
    }

    pub fn switch_for(&self, id: &str, why: &SwitchWhy) -> Result<Snapshot, AccountError> {
        if !valid_id(id) {
            return Err(err("That isn't an account id."));
        }
        let mut state = self.lock();
        let mut list = self.saved_list();
        self.capture(&mut state, &mut list)?;
        if state.active.as_deref() != Some(id) {
            if let Some(lane) = self.env.lanes.lane_for_account(id) {
                return Err(err(format!(
                    "{} is in use in the {} lane. Switch or close that lane first.",
                    lane.email, lane.name
                )));
            }
            self.live_is_saved(&state)?;
            self.activate(id)?;
            state.active = Some(id.to_string());
            self.note_active(&mut state, why);
        }
        if let Some(entry) = list.iter_mut().find(|a| a.id == id) {
            entry.needs_login = false;
        }
        self.save_list(&list)?;
        Ok(Snapshot {
            accounts: list,
            active: state.active.clone(),
        })
    }

    pub fn add(&self, oauth: &Login) -> Result<ClaudeAccount, AccountError> {
        let unreadable = || err("Signed in, but couldn't read the account's profile. Try again.");
        let access = access_token(oauth).ok_or_else(unreadable)?;
        let profile = self.profile(&access).ok_or_else(unreadable)?;
        let id = account_id_from_profile(&profile)
            .filter(|id| valid_id(id))
            .ok_or_else(unreadable)?;
        let mut login = oauth.clone();
        let org = profile.object("organization").unwrap_or_default();
        if let Some(kind) = org.str("organization_type") {
            login.0.insert(
                "subscriptionType".into(),
                json!(kind.replace("claude_", "")),
            );
        }
        if let Some(tier) = org.str("rate_limit_tier") {
            login.0.insert("rateLimitTier".into(), json!(tier));
        }
        let info = oauth_account_from_profile(&profile);
        let _state = self.lock();
        let mut list = self.saved_list();
        let mut slot = Login::default();
        slot.0
            .insert("claudeAiOauth".into(), login.clone().into_value());
        slot.0.insert("oauthAccount".into(), info.clone());
        if let Err(e) = self.write_saved(&id, &slot) {
            self.push_login(&id, LoginAction::Added, DoneOrFailed::Failed);
            return Err(e);
        }
        self.push_login(&id, LoginAction::Added, DoneOrFailed::Done);
        let info = Login(info.as_object().cloned().unwrap_or_default());
        let account = ClaudeAccount {
            id: id.clone(),
            email: info.str("emailAddress").unwrap_or("Unknown").to_string(),
            plan: plan_name(&info, &login),
            needs_login: false,
        };
        upsert(&mut list, account.clone());
        self.save_list(&list)?;
        Ok(account)
    }

    pub fn remove(&self, id: &str) -> Result<Snapshot, AccountError> {
        if !valid_id(id) {
            return Err(err("That isn't an account id."));
        }
        let state = self.lock();
        if let Some(lane) = self.env.lanes.lane_for_account(id) {
            return Err(err(format!(
                "{} is in use in the {} lane. Close that lane first.",
                lane.email, lane.name
            )));
        }
        if let Err(e) = self.env.keys.delete(SAVED_SERVICE, id) {
            self.push_login(id, LoginAction::Removed, DoneOrFailed::Failed);
            return Err(e.into());
        }
        self.push_login(id, LoginAction::Removed, DoneOrFailed::Done);
        let mut list = self.saved_list();
        list.retain(|a| a.id != id);
        self.save_list(&list)?;
        Ok(Snapshot {
            accounts: list,
            active: state.active.clone(),
        })
    }

    fn capture(
        &self,
        state: &mut State,
        list: &mut Vec<ClaudeAccount>,
    ) -> Result<(), AccountError> {
        let live = self.env.keys.read_json(LIVE_SERVICE, &self.env.keys.user)?;
        let Some(oauth) = live.and_then(|l| l.object("claudeAiOauth")) else {
            state.active = None;
            return Ok(());
        };
        let Some(access) = access_token(&oauth) else {
            state.active = None;
            return Ok(());
        };
        let config = config::oauth_account(&self.env.config_file);
        let key: [u8; 32] = Sha256::digest(access.expose().as_bytes()).into();
        let mut id = state.known.get(&key).cloned();
        let mut profile = None;
        if id.is_none() && !is_expired(&oauth, self.now()) {
            profile = self.profile(&access);
            id = profile.as_ref().and_then(account_id_from_profile);
        }
        if id.is_none() {
            id = config.as_ref().and_then(account_id_from_config);
        }
        let Some(id) = id.filter(|id| valid_id(id)) else {
            return Ok(());
        };
        state.known.insert(key, id.clone());
        state.active = Some(id.clone());
        self.note_active(state, &SwitchWhy::OTHER);
        let account = match profile {
            Some(p) => Some(oauth_account_from_profile(&p)),
            None => config
                .filter(|c| account_id_from_config(c).as_deref() == Some(id.as_str()))
                .map(Login::into_value),
        };
        let existing = self.saved_login(&id)?;
        let mut saved = existing.clone().unwrap_or_default();
        saved
            .0
            .insert("claudeAiOauth".into(), oauth.clone().into_value());
        if let Some(account) = account {
            saved.0.insert("oauthAccount".into(), account);
        }
        let Some(info) = saved.object("oauthAccount") else {
            return Ok(());
        };
        if existing.as_ref() != Some(&saved) {
            self.write_saved(&id, &saved)?;
        }
        upsert(
            list,
            ClaudeAccount {
                id,
                email: info.str("emailAddress").unwrap_or("Unknown").to_string(),
                plan: plan_name(&info, &oauth),
                needs_login: false,
            },
        );
        Ok(())
    }

    fn live_is_saved(&self, state: &State) -> Result<(), AccountError> {
        let live = self.env.keys.read_json(LIVE_SERVICE, &self.env.keys.user)?;
        let Some(oauth) = live.and_then(|l| l.object("claudeAiOauth")) else {
            return Ok(());
        };
        let saved = match &state.active {
            Some(active) => self
                .saved_login(active)?
                .and_then(|s| s.object("claudeAiOauth")),
            None => None,
        };
        if saved.as_ref() == Some(&oauth) {
            return Ok(());
        }
        Err(err(
            "Claude Code's current login couldn't be saved (Notchling can't tell which account it is), so the account wasn't changed. Run claude once so it names the account, then try again.",
        ))
    }

    fn activate(&self, id: &str) -> Result<(), AccountError> {
        let saved = self.saved_login(id)?;
        let (Some(oauth), Some(account)) = (
            saved.as_ref().and_then(|s| s.object("claudeAiOauth")),
            saved.as_ref().and_then(|s| s.get("oauthAccount").cloned()),
        ) else {
            return Err(err(
                "No saved login for this account. Sign in to it once with claude auth login.",
            ));
        };
        config::spliced(&self.env.config_file, &account).map_err(err)?;
        let user = self.env.keys.user.clone();
        let old = self.env.keys.read_text(LIVE_SERVICE, &user)?;
        let value = oauth.into_value();
        let new = old
            .as_ref()
            .and_then(|t| set_top_level(t.expose(), "claudeAiOauth", &value).ok())
            .or_else(|| set_top_level("{}", "claudeAiOauth", &value).ok())
            .ok_or_else(|| err("Couldn't update Claude Code's login."))?;
        if let Err(e) = self.env.keys.write(LIVE_SERVICE, &user, &new) {
            self.undo_live(&user, old.as_ref());
            return Err(err(format!(
                "Couldn't update Claude Code's login in the Keychain: {e}"
            )));
        }
        if let Err(why) = config::set_account(&self.env.config_file, &account) {
            self.undo_live(&user, old.as_ref());
            return Err(err(why));
        }
        Ok(())
    }

    fn undo_live(&self, user: &str, old: Option<&Secret>) {
        let _ = match old {
            Some(old) => self.env.undo_keys.write(LIVE_SERVICE, user, old.expose()),
            None => self.env.undo_keys.delete(LIVE_SERVICE, user).map(|_| ()),
        };
    }

    fn saved_login(&self, id: &str) -> Result<Option<Login>, AccountError> {
        Ok(self.env.keys.read_json(SAVED_SERVICE, id)?)
    }

    fn write_saved(&self, id: &str, value: &Login) -> Result<(), AccountError> {
        let text = serde_json::to_string(&value.0).map_err(|e| err(e.to_string()))?;
        self.env
            .keys
            .write(SAVED_SERVICE, id, &text)
            .map_err(|e| err(format!("Couldn't save the login in the Keychain: {e}")))
    }

    fn profile(&self, token: &Secret) -> Option<Login> {
        self.env
            .http
            .send(&HttpRequest::get(PROFILE_URL, token))
            .json_ok()
            .map(Login)
    }

    fn config_account(&self) -> Option<String> {
        config::oauth_account(&self.env.config_file)
            .as_ref()
            .and_then(account_id_from_config)
    }
}

pub fn account_value(account: &ClaudeAccount) -> serde_json::Value {
    serde_json::to_value(account).unwrap_or_default()
}

fn upsert(list: &mut Vec<ClaudeAccount>, account: ClaudeAccount) {
    match list.iter_mut().find(|a| a.id == account.id) {
        Some(slot) => *slot = account,
        None => list.push(account),
    }
}

#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_lanes;
#[cfg(test)]
mod tests_log;
#[cfg(test)]
mod tests_safe;
#[cfg(test)]
mod tests_support;
