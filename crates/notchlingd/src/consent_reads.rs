use crate::consent_ops::{
    failed, forbidden, lock, ConsentOps, NO_SUCH, OTHER_SESSION, SLICE, WAIT_MAX_MS,
};
use crate::consent_wire::view;
use notchling_engine::consent::{Consent, State};
use notchling_local_api::{ApiError, Caller};
use serde_json::{json, Value};
use std::time::{Duration, Instant};

impl ConsentOps {
    pub fn live_twins(&self, row: &Consent) -> Vec<Consent> {
        let (true, Some(session)) = (row.verified, row.session.as_ref()) else {
            return Vec::new();
        };
        let now = self.now();
        let mut twins: Vec<Consent> = self
            .store
            .open_rows()
            .unwrap_or_default()
            .into_iter()
            .filter(|r| {
                r.id != row.id
                    && r.kind == row.kind
                    && r.target.to_lowercase() == row.target.to_lowercase()
                    && r.state.open()
                    && r.expires_ms > now
                    && r.verified
                    && r.session.as_ref() == Some(session)
                    && r.applied_by == notchling_engine::consent::AppliedBy::App
            })
            .collect();
        twins.sort_by_key(|r| -r.created_ms);
        twins
    }

    pub fn get_row(&self, id: &str) -> Result<Consent, ApiError> {
        self.store
            .get(id)
            .map_err(failed)?
            .ok_or_else(|| failed(NO_SUCH))
    }

    fn may_see(&self, caller: &Caller, row: &Consent) -> bool {
        if !row.verified {
            return true;
        }
        let who = self.who(caller);
        who.verified && who.session.is_some() && who.session == row.session
    }

    pub(crate) fn row_for(
        &self,
        caller: &Caller,
        args: &Value,
        usage: &str,
    ) -> Result<Consent, ApiError> {
        let id = args
            .get("id")
            .and_then(Value::as_str)
            .ok_or_else(|| ApiError::bad_request(usage.to_string()))?;
        self.row_of(caller, id)
    }

    pub fn row_of(&self, caller: &Caller, id: &str) -> Result<Consent, ApiError> {
        let row = self
            .store
            .get(id)
            .map_err(failed)?
            .ok_or_else(|| failed(NO_SUCH))?;
        if Self::app_only(caller).is_err() && !self.may_see(caller, &row) {
            return Err(forbidden(OTHER_SESSION, "other_session"));
        }
        Ok(row)
    }

    pub fn wait_for(&self, id: &str, wait: Duration) -> Option<Consent> {
        let end = Instant::now() + wait;
        let first = self.store.get(id).ok().flatten()?;
        let _here = (!wait.is_zero() && !first.state.closed()).then(|| self.here(id));
        loop {
            let row = self.store.get(id).ok().flatten()?;
            let left = end.saturating_duration_since(Instant::now());
            if row.state.closed() || left.is_zero() {
                return Some(row);
            }
            let gate = lock(&self.gate);
            let _ = self
                .changed
                .wait_timeout(gate, left.min(SLICE))
                .unwrap_or_else(|e| e.into_inner());
        }
    }

    pub(crate) fn get(&self, caller: &Caller, args: &Value) -> Result<Value, ApiError> {
        let row = self.row_for(caller, args, "consent.get takes {id, waitMs?}")?;
        let wait = args
            .get("waitMs")
            .and_then(Value::as_u64)
            .unwrap_or(0)
            .min(WAIT_MAX_MS);
        let row = if wait == 0 {
            row
        } else {
            self.wait_for(&row.id, Duration::from_millis(wait))
                .unwrap_or(row)
        };
        let agent = Self::app_only(caller).is_err();
        let mut out = self.project(&row, agent);
        if agent && row.state.closed() {
            let who = self.who(caller);
            if let Some(held) = self.handler(&row.kind).and_then(|h| h.handover(&row, &who)) {
                out["handover"] = held;
            }
        }
        Ok(out)
    }

    pub fn project(&self, row: &Consent, agent: bool) -> Value {
        let mut out = view(row);
        if let Some(handler) = self.handler(&row.kind) {
            handler.shape(row, &mut out, agent);
        }
        out
    }

    pub(crate) fn mine(&self, caller: &Caller, args: &Value) -> Result<Value, ApiError> {
        let who = self.who(caller);
        let rows = match (who.verified, who.session) {
            (true, Some(session)) => self.store.of_session(&session).map_err(failed)?,
            _ => Vec::new(),
        };
        let states: Option<Vec<State>> = args.get("states").and_then(Value::as_array).map(|a| {
            a.iter()
                .filter_map(|s| s.as_str().and_then(State::parse))
                .collect()
        });
        let rows: Vec<Value> = rows
            .iter()
            .filter(|r| states.as_ref().is_none_or(|s| s.contains(&r.state)))
            .map(|r| self.project(r, true))
            .collect();
        Ok(json!({"requests": rows}))
    }

    pub(crate) fn list(&self, caller: &Caller) -> Result<Value, ApiError> {
        Self::app_only(caller)?;
        let mut rows = self.store.all().map_err(failed)?;
        rows.sort_by_key(|r| (r.state.closed(), -r.created_ms));
        let rows: Vec<Value> = rows
            .iter()
            .take(200)
            .map(|r| self.project(r, false))
            .collect();
        Ok(json!({"requests": rows}))
    }

    pub fn waiting_for(&self, kind: &str, target: &str) -> Vec<Consent> {
        self.store
            .open_rows()
            .unwrap_or_default()
            .into_iter()
            .filter(|r| r.kind == kind && r.target == target && r.state.open())
            .collect()
    }

    pub fn pending_for(&self, caller: &Caller, kind: &str, target: &str) -> Vec<Consent> {
        let rows = self.waiting_for(kind, target);
        if Self::app_only(caller).is_ok() {
            return rows;
        }
        let who = self.who(caller);
        rows.into_iter()
            .filter(|r| {
                r.verified && who.verified && who.session.is_some() && who.session == r.session
            })
            .collect()
    }
}
