use super::profile::{access_token, account_id_from_profile, is_expired, valid_id};
use super::{err, AccountError, ClaudeAccount, ClaudeAccounts, State, SwitchWhy};
use crate::accounts::Login;
use crate::control::MoveAllPlan;
use crate::lanes::{new_name, service_for, Lane};
use crate::sessions::Agent;
use notchling_events::kinds::accounts::LaneAction;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MoveAllAccounts {
    pub closed_lane: Option<String>,
    pub switched_to: Option<String>,
    pub errors: Vec<String>,
}

impl ClaudeAccounts {
    pub fn lanes(&self) -> Vec<Lane> {
        self.env.lanes.all()
    }

    fn now_secs(&self) -> u64 {
        u64::try_from(self.now() / 1000).unwrap_or(0)
    }

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
        self.push_lane(&lane.name, LaneAction::Created, Some(id));
        Ok(lane)
    }

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
        self.lane_login_is_saved(&lanes[i])?;
        self.env
            .lanes
            .set_login(&self.env.keys, &lanes[i], &oauth)
            .map_err(|e| {
                err(format!(
                    "Couldn't write the lane's login to the Keychain: {e}"
                ))
            })?;
        let _ = self.env.lanes.set_profile(&lanes[i], &profile);
        lanes[i].account_id = id.to_string();
        lanes[i].email = account.email;
        self.env.lanes.save(&lanes)?;
        self.save_list(&list)?;
        self.push_lane(&lanes[i].name, LaneAction::Switched, Some(id));
        Ok(lanes[i].clone())
    }

    pub fn close_lane(&self, name: &str) -> Result<bool, AccountError> {
        let _state = self.lock();
        let mut lanes = self.env.lanes.all();
        let Some(i) = lanes.iter().position(|l| l.name == name) else {
            return Ok(false);
        };
        self.return_login(&lanes[i])?;
        self.lane_login_is_saved(&lanes[i])?;
        self.env.lanes.remove_login(&self.env.keys, &lanes[i])?;
        let _ = self.env.lanes.remove_folder(&lanes[i], self.now_secs());
        let gone = lanes.remove(i);
        self.env.lanes.save(&lanes)?;
        self.push_lane(&gone.name, LaneAction::Removed, Some(&gone.account_id));
        Ok(true)
    }

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
            match self.switch_for(id, &SwitchWhy::MOVE_ALL) {
                Ok(_) => done.switched_to = Some(id.clone()),
                Err(e) => done
                    .errors
                    .push(format!("Couldn't switch the main login: {e}")),
            }
        }
        done
    }

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

    fn lane_login_is_saved(&self, lane: &Lane) -> Result<(), AccountError> {
        let Some(oauth) = self.env.lanes.login(&self.env.keys, lane)? else {
            return Ok(());
        };
        let saved = self
            .saved_login(&lane.account_id)?
            .and_then(|s| s.object("claudeAiOauth"));
        if saved.as_ref() == Some(&oauth) {
            return Ok(());
        }
        Err(err(format!(
            "The {} lane's login couldn't be saved (it may be signed in to another account), so the lane was left as it is.",
            lane.name
        )))
    }

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
        if saved.object("claudeAiOauth").as_ref() == Some(&oauth) {
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
