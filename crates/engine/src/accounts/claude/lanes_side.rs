//! The account side of lanes (`AccountStore`'s lane extension) and of Move All.
//!
//! - Opening a lane hands a saved account's login to the lane's own Keychain item; the account
//!   must not be the main login's or another lane's.
//! - Switching a lane first copies its login back to the old account's saved slot (Claude Code may
//!   have renewed it there, retiring the old refresh token), after checking with the profile
//!   endpoint that it still belongs to that account, then sets the new one.
//! - Closing a lane gives its login back the same way, removes the lane's item and its folder.
//! - Move All closes the lane holding the target account, then switches the main login to it.

use super::profile::{access_token, account_id_from_profile, is_expired, valid_id};
use super::{err, AccountError, ClaudeAccount, ClaudeAccounts, State};
use crate::accounts::Login;
use crate::control::MoveAllPlan;
use crate::lanes::{new_name, service_for, Lane};
use crate::sessions::Agent;

/// What Move All's account work did.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MoveAllAccounts {
    /// The lane closed to free the account.
    pub closed_lane: Option<String>,
    /// The account the main login switched to.
    pub switched_to: Option<String>,
    /// Why a step failed; the steps after a failed close still run (the switch then refuses).
    pub errors: Vec<String>,
}

impl ClaudeAccounts {
    /// The lanes in `lanes.json` (a file read only).
    pub fn lanes(&self) -> Vec<Lane> {
        self.env.lanes.all()
    }

    fn now_secs(&self) -> u64 {
        u64::try_from(self.now() / 1000).unwrap_or(0)
    }

    /// Starts a lane on a saved account, or returns the one it is already in.
    pub fn open_lane(&self, id: &str) -> Result<Lane, AccountError> {
        if !valid_id(id) {
            return Err(err("That isn't an account id."));
        }
        let mut state = self.lock();
        let mut list = self.saved_list();
        let mut lanes = self.env.lanes.all();
        if let Some(lane) = lanes.iter().find(|l| l.account_id == id) {
            crate::lanes::relink(lane, &self.env.lanes.main_dir, self.now_secs());
            return Ok(lane.clone());
        }
        self.capture(&mut state, &mut list)?;
        if state.active.as_deref() == Some(id) {
            return Err(err(
                "That account is the main login. Open Claude Code as usual for it.",
            ));
        }
        let account = find(&list, id)?;
        let oauth = self.login_for_lane(&mut state, &mut list, id)?;
        let profile = self.saved_profile(id)?;
        let names: Vec<String> = lanes.iter().map(|l| l.name.clone()).collect();
        let name = new_name(&account.email, &names);
        let lane = Lane {
            path: self.env.lanes.path_for(&name),
            name,
            account_id: id.to_string(),
            email: account.email.clone(),
            folder: None,
        };
        service_for(&lane.path)?;
        self.env.lanes.prepare(&lane, &profile, self.now_secs())?;
        if let Err(e) = self.env.lanes.set_login(&self.env.keys, &lane, &oauth) {
            let _ = self.env.lanes.remove_folder(&lane, self.now_secs());
            return Err(err(format!(
                "Couldn't write the lane's login to the Keychain: {e}"
            )));
        }
        lanes.push(lane.clone());
        self.env.lanes.save(&lanes)?;
        self.save_list(&list)?;
        Ok(lane)
    }

    /// Moves a lane to another account. Sessions in the lane carry on with the new login on their
    /// next request.
    pub fn switch_lane(&self, name: &str, id: &str) -> Result<Lane, AccountError> {
        if !valid_id(id) {
            return Err(err("That isn't an account id."));
        }
        let mut state = self.lock();
        let mut list = self.saved_list();
        let mut lanes = self.env.lanes.all();
        let i = lanes
            .iter()
            .position(|l| l.name == name)
            .ok_or_else(|| err(format!("There's no {name} lane.")))?;
        if lanes[i].account_id == id {
            return Ok(lanes[i].clone());
        }
        self.capture(&mut state, &mut list)?;
        if state.active.as_deref() == Some(id) || lanes.iter().any(|l| l.account_id == id) {
            return Err(err(
                "That account is already in use in the main login or another lane.",
            ));
        }
        let account = find(&list, id)?;
        let oauth = self.login_for_lane(&mut state, &mut list, id)?;
        let profile = self.saved_profile(id)?;
        self.return_login(&lanes[i])?;
        self.env
            .lanes
            .set_login(&self.env.keys, &lanes[i], &oauth)
            .map_err(|e| {
                err(format!(
                    "Couldn't write the lane's login to the Keychain: {e}"
                ))
            })?;
        // The profile only names the account in `/status`; the login is what counts.
        let _ = self.env.lanes.set_profile(&lanes[i], &profile);
        lanes[i].account_id = id.to_string();
        lanes[i].email = account.email;
        self.env.lanes.save(&lanes)?;
        self.save_list(&list)?;
        Ok(lanes[i].clone())
    }

    /// Ends a lane: its login goes back to the saved account, the lane's Keychain item and folder
    /// are removed. False when there was no such lane. A login that can't be read back stops it,
    /// since removing the item would lose a renewed login.
    pub fn close_lane(&self, name: &str) -> Result<bool, AccountError> {
        let _state = self.lock();
        let mut lanes = self.env.lanes.all();
        let Some(i) = lanes.iter().position(|l| l.name == name) else {
            return Ok(false);
        };
        self.return_login(&lanes[i])?;
        self.env.lanes.remove_login(&self.env.keys, &lanes[i])?;
        // A folder that can't be removed stays; the lane is closed either way.
        let _ = self.env.lanes.remove_folder(&lanes[i], self.now_secs());
        lanes.remove(i);
        self.env.lanes.save(&lanes)?;
        Ok(true)
    }

    /// Move All's account work: close the lane holding the account, then switch the main login.
    pub fn move_all_accounts(&self, plan: &MoveAllPlan) -> MoveAllAccounts {
        let mut done = MoveAllAccounts::default();
        if let Some(name) = &plan.close_lane {
            match self.close_lane(name) {
                Ok(true) => done.closed_lane = Some(name.clone()),
                Ok(false) => {}
                Err(e) => done
                    .errors
                    .push(format!("Couldn't close the {name} lane: {e}")),
            }
        }
        if let Some(id) = &plan.switch_main {
            match self.switch_to(id) {
                Ok(_) => done.switched_to = Some(id.clone()),
                Err(e) => done
                    .errors
                    .push(format!("Couldn't switch the main login: {e}")),
            }
        }
        done
    }

    /// The Move All plan for putting every session on `target` (`None`: the main login's own
    /// account), with the main login read fresh.
    pub fn move_all_plan(
        &self,
        agents: &[Agent],
        target: Option<&str>,
    ) -> Result<MoveAllPlan, AccountError> {
        let active = self.sync()?.active;
        Ok(MoveAllPlan::make(
            agents,
            &self.env.lanes.all(),
            active.as_deref(),
            target,
        ))
    }

    /// A saved login fit to hand to a lane, renewed first when it has expired (it isn't live
    /// anywhere yet).
    fn login_for_lane(
        &self,
        state: &mut State,
        list: &mut [ClaudeAccount],
        id: &str,
    ) -> Result<Login, AccountError> {
        let signed_out = || {
            err("The saved login for that account is signed out. Add the account again in Notchling.")
        };
        self.token_locked(state, list, id)?;
        if list.iter().any(|a| a.id == id && a.needs_login) {
            return Err(signed_out());
        }
        self.saved_login(id)?
            .and_then(|s| s.object("claudeAiOauth"))
            .ok_or_else(signed_out)
    }

    fn saved_profile(&self, id: &str) -> Result<serde_json::Value, AccountError> {
        self.saved_login(id)?
            .and_then(|s| s.get("oauthAccount").cloned())
            .ok_or_else(|| {
                err("Notchling has no profile saved for that account. Add the account again.")
            })
    }

    /// Copies a lane's login back to its account's saved slot. A login the profile endpoint says
    /// belongs to another account never goes back; one that can't be checked (expired, no answer)
    /// goes back as the Swift app did, since only Notchling writes a lane's item and Claude Code
    /// renews it in place for the same account. True when the slot changed.
    pub(super) fn return_login(&self, lane: &Lane) -> Result<bool, AccountError> {
        let Some(oauth) = self.env.lanes.login(&self.env.keys, lane)? else {
            return Ok(false);
        };
        let Some(mut saved) = self.saved_login(&lane.account_id)? else {
            return Ok(false);
        };
        if !is_expired(&oauth, self.now()) {
            if let Some(profile) = access_token(&oauth).and_then(|t| self.profile(&t)) {
                if account_id_from_profile(&profile).as_deref() != Some(lane.account_id.as_str()) {
                    return Ok(false);
                }
            }
        }
        let current = saved
            .object("claudeAiOauth")
            .and_then(|o| o.str("refreshToken").map(String::from));
        if current.as_deref() == oauth.str("refreshToken") {
            return Ok(false);
        }
        saved.0.insert("claudeAiOauth".into(), oauth.into_value());
        self.write_saved(&lane.account_id, &saved)?;
        Ok(true)
    }
}

fn find(list: &[ClaudeAccount], id: &str) -> Result<ClaudeAccount, AccountError> {
    list.iter()
        .find(|a| a.id == id)
        .cloned()
        .ok_or_else(|| err("Notchling has no saved login for that account."))
}
