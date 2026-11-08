//! Claude Code's accounts (`AccountStore` in `Accounts/Accounts.swift`): a copy of every login
//! seen, swapped in on request so switching needs no new sign-in.
//!
//! Claude Code keeps its login in the Keychain item `Claude Code-credentials` (JSON: `claudeAiOauth`
//! beside MCP logins) and the account's profile in `oauthAccount` of `~/.claude.json`. A switch
//! replaces only those two pieces. Saved logins are Keychain items of their own (service
//! `Notchling Accounts`, account `<id>`, the Swift app's, holding `{claudeAiOauth, oauthAccount}`),
//! never on disk; the list of saved accounts (id, email, plan, needs-login: no token) is a file the
//! caller names, the daemon's own until handover.
//!
//! One lock serializes every operation, so a removal can't be undone by a sync that read the list
//! before it (the Swift app's issue #7, which its `removedSince` stamps worked around).
//!
//! The rules: the main login's token and a lane's are never renewed here (refresh tokens rotate,
//! and the copy in use would be signed out); only a saved login live nowhere is renewed, and that
//! is checked again just before. An account is live in one place: the main switch refuses an
//! account in a lane, and a lane refuses the main login's account or another lane's.

mod config;
mod lanes_side;
pub mod profile;
pub mod signin;
mod tokens;

pub use lanes_side::MoveAllAccounts;
pub use tokens::Renewal;

use crate::accounts::http::{HttpClient, HttpRequest, PROFILE_URL};
use crate::accounts::json_splice::set_top_level;
use crate::accounts::keychain::Keys;
use crate::accounts::{Login, Secret};
use crate::lanes::LaneStore;
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

/// Claude Code's own login item.
pub const LIVE_SERVICE: &str = "Claude Code-credentials";
/// Notchling's saved logins, one item per account id.
pub const SAVED_SERVICE: &str = "Notchling Accounts";

/// A Claude subscription Notchling can switch Claude Code to (the Swift `ClaudeAccount`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClaudeAccount {
    /// `<accountUuid>:<organizationUuid>`.
    pub id: String,
    pub email: String,
    pub plan: String,
    /// The saved login was refused; the user has to sign in to this account again.
    #[serde(rename = "needsLogin", default)]
    pub needs_login: bool,
}

/// The saved accounts and the main login's, as last seen.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct Snapshot {
    pub accounts: Vec<ClaudeAccount>,
    /// The main login's account; `None` when signed out or not read yet.
    pub active: Option<String>,
}

/// Why an operation failed, in words for the user. Never holds a token.
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

/// Everything the store reaches outside itself.
pub struct ClaudeEnv {
    pub keys: Keys,
    pub http: Arc<dyn HttpClient>,
    pub now_ms: Arc<dyn Fn() -> i64 + Send + Sync>,
    /// `~/.claude.json`.
    pub config_file: PathBuf,
    /// The saved accounts list (no tokens).
    pub list_file: PathBuf,
    pub lanes: LaneStore,
}

#[derive(Default)]
struct State {
    active: Option<String>,
    /// SHA-256 of access tokens already matched to an account, so the profile is asked once per
    /// token. Hashes, so no token sits in memory longer than a call.
    known: HashMap<[u8; 32], String>,
}

pub struct ClaudeAccounts {
    env: ClaudeEnv,
    /// Held for a whole operation, Keychain and network included.
    state: Mutex<State>,
    /// The main login's account as the last operation left it, for [`ClaudeAccounts::snapshot`],
    /// which must not wait behind a switch stuck on a Keychain prompt.
    seen: Mutex<Option<String>>,
}

/// The operation lock. On release it publishes `active` to [`ClaudeAccounts::seen`].
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

    /// The saved accounts list, from its file only: no Keychain, no network.
    pub fn saved_list(&self) -> Vec<ClaudeAccount> {
        std::fs::read(&self.env.list_file)
            .ok()
            .and_then(|data| serde_json::from_slice(&data).ok())
            .unwrap_or_default()
    }

    fn save_list(&self, list: &[ClaudeAccount]) -> Result<(), AccountError> {
        if let Some(dir) = self.env.list_file.parent() {
            notchling_platform::files::ensure_private_dir(dir).map_err(|e| err(e.to_string()))?;
        }
        let data = serde_json::to_vec(list).map_err(|e| err(e.to_string()))?;
        notchling_platform::files::write_private_no_follow(&self.env.list_file, &data)
            .map_err(|e| err(format!("Couldn't save the accounts list: {e}")))
    }

    /// The list and the main login's account as last read; no Keychain, no network, and no
    /// waiting for an operation under way.
    pub fn snapshot(&self) -> Snapshot {
        Snapshot {
            accounts: self.saved_list(),
            active: self.seen.lock().unwrap_or_else(|e| e.into_inner()).clone(),
        }
    }

    /// Reads the current login, saves a copy, and returns the list.
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

    /// Points Claude Code at a saved account. The current login is saved first, since Claude Code
    /// may have renewed it since the last sync.
    pub fn switch_to(&self, id: &str) -> Result<Snapshot, AccountError> {
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

    /// Saves a login made in Notchling's own sign-in. Claude Code keeps the account it's on.
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
        self.write_saved(&id, &slot)?;
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

    /// Forgets a saved account and its saved login. Claude Code stays signed in if it's the current
    /// one. An account live in a lane is refused: the lane gives its login back to that slot.
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
        self.env.keys.delete(SAVED_SERVICE, id)?;
        let mut list = self.saved_list();
        list.retain(|a| a.id != id);
        self.save_list(&list)?;
        Ok(Snapshot {
            accounts: list,
            active: state.active.clone(),
        })
    }

    // MARK: Capturing the current login

    /// Works out which account Claude Code is signed in to and stores a copy of its login.
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
        // An expired token can't be checked; Claude Code keeps ~/.claude.json in step with its login.
        if id.is_none() {
            id = config.as_ref().and_then(account_id_from_config);
        }
        let Some(id) = id.filter(|id| valid_id(id)) else {
            return Ok(());
        };
        state.known.insert(key, id.clone());
        state.active = Some(id.clone());
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

    // MARK: Switching

    /// Ok when Claude Code has no login, or its `claudeAiOauth` reads back from the active
    /// account's saved slot unchanged: only then may a switch replace it, since Claude Code renews
    /// it in place and the live item can hold the only working refresh token.
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
        // Checked before the Keychain changes, so a file that can't take it stops the switch.
        config::spliced(&self.env.config_file, &account).map_err(err)?;
        let user = self.env.keys.user.clone();
        let old = self.env.keys.read_text(LIVE_SERVICE, &user)?;
        let value = oauth.into_value();
        // Keep the MCP logins and anything else in the item; replace only the Claude login.
        let new = old
            .as_ref()
            .and_then(|t| set_top_level(t.expose(), "claudeAiOauth", &value).ok())
            .or_else(|| set_top_level("{}", "claudeAiOauth", &value).ok())
            .ok_or_else(|| err("Couldn't update Claude Code's login."))?;
        self.env
            .keys
            .write(LIVE_SERVICE, &user, &new)
            .map_err(|e| {
                err(format!(
                    "Couldn't update Claude Code's login in the Keychain: {e}"
                ))
            })?;
        if let Err(why) = config::set_account(&self.env.config_file, &account) {
            // Put the old login back, so the Keychain and ~/.claude.json name one account.
            if let Some(old) = old {
                let _ = self.env.keys.write(LIVE_SERVICE, &user, old.expose());
            }
            return Err(err(why));
        }
        Ok(())
    }

    // MARK: Keychain and profile

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

    /// `GET /api/oauth/profile` with `token`; `None` unless it answered 200 with an object.
    fn profile(&self, token: &Secret) -> Option<Login> {
        self.env
            .http
            .send(&HttpRequest::get(PROFILE_URL, token))
            .json_ok()
            .map(Login)
    }

    /// The main login's account by `~/.claude.json` (no Keychain).
    fn config_account(&self) -> Option<String> {
        config::oauth_account(&self.env.config_file)
            .as_ref()
            .and_then(account_id_from_config)
    }
}

/// An account as JSON (`{id, email, plan, needsLogin}`: no token).
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
mod tests_safe;
#[cfg(test)]
mod tests_support;
