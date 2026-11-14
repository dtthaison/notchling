use super::profile::{access_token, is_expired};
use super::{err, AccountError, ClaudeAccount, ClaudeAccounts, State, SAVED_SERVICE};
use crate::accounts::http::{HttpRequest, CLIENT_ID, TOKEN_URL};
use crate::accounts::{Login, Secret};
use notchling_events::kinds::accounts::LoginAction;
use notchling_events::kinds::choices::DoneOrFailed;
use serde_json::{json, Map, Value};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Renewal {
    Renewed,
    Refused,
    Failed,
    Live,
}

impl ClaudeAccounts {
    pub fn usage_token(&self, id: &str) -> Result<Option<Secret>, AccountError> {
        let mut state = self.lock();
        let mut list = self.saved_list();
        let token = self.token_locked(&mut state, &mut list, id)?;
        self.save_list(&list)?;
        Ok(token)
    }

    pub fn live_token(&self) -> Result<Option<Secret>, AccountError> {
        let state = self.lock();
        if state.active.is_none() {
            return Ok(None);
        }
        let live = self
            .env
            .keys
            .read_json(super::LIVE_SERVICE, &self.env.keys.user)?;
        let now = self.now();
        Ok(live
            .and_then(|l| l.object("claudeAiOauth"))
            .filter(|o| !is_expired(o, now))
            .as_ref()
            .and_then(access_token))
    }

    pub(super) fn token_locked(
        &self,
        state: &mut State,
        list: &mut [ClaudeAccount],
        id: &str,
    ) -> Result<Option<Secret>, AccountError> {
        let now = self.now();
        let usable = |oauth: Option<Login>| {
            oauth
                .filter(|o| !is_expired(o, now))
                .as_ref()
                .and_then(access_token)
        };
        if state.active.as_deref() == Some(id) {
            let live = self
                .env
                .keys
                .read_json(super::LIVE_SERVICE, &self.env.keys.user)?;
            return Ok(usable(live.and_then(|l| l.object("claudeAiOauth"))));
        }
        if let Some(lane) = self.env.lanes.lane_for_account(id) {
            return Ok(usable(self.env.lanes.login(&self.env.keys, &lane)?));
        }
        let Some(saved) = self.saved_login(id)? else {
            return Ok(None);
        };
        let Some(oauth) = saved.object("claudeAiOauth") else {
            return Ok(None);
        };
        if !is_expired(&oauth, now) {
            return Ok(access_token(&oauth));
        }
        let renewal = self.renew_saved(state, id, saved)?;
        match renewal {
            Renewal::Renewed => {
                self.push_login(id, LoginAction::RenewedSaved, DoneOrFailed::Done);
            }
            Renewal::Refused => {
                self.push_login(id, LoginAction::RenewedSaved, DoneOrFailed::Failed);
            }
            Renewal::Failed | Renewal::Live => {}
        }
        if let Some(entry) = list.iter_mut().find(|a| a.id == id) {
            match renewal {
                Renewal::Renewed => entry.needs_login = false,
                Renewal::Refused => entry.needs_login = true,
                Renewal::Failed | Renewal::Live => {}
            }
        }
        if renewal != Renewal::Renewed {
            return Ok(None);
        }
        Ok(self
            .saved_login(id)?
            .and_then(|s| s.object("claudeAiOauth"))
            .as_ref()
            .and_then(access_token))
    }

    pub(super) fn live_somewhere(&self, state: &State, id: &str) -> bool {
        state.active.as_deref() == Some(id)
            || self.config_account().as_deref() == Some(id)
            || self.env.lanes.lane_for_account(id).is_some()
    }

    fn renew_saved(
        &self,
        state: &State,
        id: &str,
        mut saved: Login,
    ) -> Result<Renewal, AccountError> {
        if self.live_somewhere(state, id) {
            return Ok(Renewal::Live);
        }
        let Some(mut oauth) = saved.object("claudeAiOauth") else {
            return Ok(Renewal::Failed);
        };
        let Some(refresh) = oauth.str("refreshToken").map(Secret::new) else {
            return Ok(Renewal::Failed);
        };
        let scopes: Option<Vec<String>> = oauth.get("scopes").and_then(Value::as_array).map(|a| {
            a.iter()
                .filter_map(Value::as_str)
                .map(String::from)
                .collect()
        });
        let fresh = match refresh_request(self, &refresh, scopes.as_deref()) {
            Ok(fresh) => fresh,
            Err(renewal) => return Ok(renewal),
        };
        for (key, value) in fresh {
            oauth.0.insert(key, value);
        }
        saved.0.insert("claudeAiOauth".into(), oauth.into_value());
        let text = serde_json::to_string(&saved.0).map_err(|e| err(e.to_string()))?;
        self.env.keys.write(SAVED_SERVICE, id, &text)?;
        Ok(Renewal::Renewed)
    }
}

fn refresh_request(
    store: &ClaudeAccounts,
    refresh: &Secret,
    scopes: Option<&[String]>,
) -> Result<Map<String, Value>, Renewal> {
    let mut body = json!({
        "grant_type": "refresh_token",
        "refresh_token": refresh.expose(),
        "client_id": CLIENT_ID,
    });
    if let Some(scopes) = scopes.filter(|s| !s.is_empty()) {
        body["scope"] = json!(scopes.join(" "));
    }
    let answer = store.env.http.send(&HttpRequest::post(TOKEN_URL, body));
    if answer.status == 0 {
        return Err(Renewal::Failed);
    }
    let json = answer.json_ok().unwrap_or_default();
    let Some(access) = json.get("access_token").and_then(Value::as_str) else {
        return Err(if matches!(answer.status, 400 | 401) {
            Renewal::Refused
        } else {
            Renewal::Failed
        });
    };
    let now = store.now() as f64 / 1000.0;
    let mut fresh = Map::new();
    fresh.insert("accessToken".into(), json!(access));
    if let Some(r) = json.get("refresh_token").and_then(Value::as_str) {
        fresh.insert("refreshToken".into(), json!(r));
    }
    if let Some(s) = json.get("expires_in").and_then(Value::as_f64) {
        fresh.insert("expiresAt".into(), json!(((now + s) * 1000.0) as i64));
    }
    if let Some(s) = json.get("refresh_token_expires_in").and_then(Value::as_f64) {
        fresh.insert(
            "refreshTokenExpiresAt".into(),
            json!(((now + s) * 1000.0) as i64),
        );
    }
    Ok(fresh)
}
