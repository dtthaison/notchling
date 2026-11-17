use crate::acting_ops::forbidden;
use crate::ask_ops::{id_arg, AskOps, Taken, NO_SESSION, PERSON_ONLY};
use crate::ask_rows::wire;
use crate::ask_waiters::{budget, ANY};
use notchling_engine::ask::{main_may_see, Asker, By, Row, Who};
use notchling_local_api::{ApiError, Caller, Tier};
use serde_json::{json, Value};
use std::time::{Duration, Instant};

fn lost_reply(state: &str, by: Option<By>) -> Value {
    let mut out = json!({"answered": false, "state": state});
    if let Some(by) = by {
        out["by"] = json!(by.as_str());
    }
    out
}

fn waited(args: &Value) -> Duration {
    if args.get("waitMs").is_some() {
        budget(args)
    } else {
        Duration::ZERO
    }
}

impl AskOps {
    fn mine_rows(&self, session: &str) -> Result<Vec<Row>, ApiError> {
        let rows = self.store.list(Some(session)).map_err(|_| {
            ApiError::new(
                notchling_local_api::ErrorCode::Failed,
                "Couldn't read the questions.",
            )
        })?;
        Ok(rows
            .into_iter()
            .filter(|row| main_may_see(row, session))
            .collect())
    }

    pub(crate) fn mine_op(&self, caller: &Caller, args: &Value) -> Result<Value, ApiError> {
        let pid = caller.pid.filter(|pid| *pid > 0);
        let Some(hit) = pid.and_then(|pid| self.session_of(pid)) else {
            return Err(forbidden(NO_SESSION));
        };
        let session = self.session_key(&hit);
        let until = Instant::now() + waited(args);
        let here = self.waiters.enter(ANY);
        let rows = loop {
            let seen = here.mark();
            let rows = self.mine_rows(&session)?;
            if !rows.is_empty() || self.waiters.is_closed() || !here.block(seen, until) {
                break rows;
            }
        };
        drop(here);
        self.waiters.release(ANY);
        Ok(Value::Array(
            rows.iter()
                .map(|row| wire(row, self.waiters.connected(&row.id), false))
                .collect(),
        ))
    }

    pub(crate) fn answer_op(&self, caller: &Caller, args: &Value) -> Result<Value, ApiError> {
        let id = id_arg(args)?;
        let text = args.get("text").and_then(Value::as_str);
        let choice = args
            .get("choice")
            .and_then(Value::as_u64)
            .map(|n| u8::try_from(n).unwrap_or(0));
        let taken = if caller.tier == Tier::Agent {
            self.agent_answer(caller, id, text, choice)?
        } else {
            self.answer(id, text, choice, By::Mac)?
        };
        Ok(match taken {
            Taken::Won => json!({"answered": true}),
            Taken::Lost { state, by } => lost_reply(state.as_str(), by),
        })
    }

    fn agent_answer(
        &self,
        caller: &Caller,
        id: &str,
        text: Option<&str>,
        choice: Option<u8>,
    ) -> Result<Taken, ApiError> {
        let pid = caller.pid.filter(|pid| *pid > 0);
        let Some((pid, hit)) = pid.and_then(|pid| Some((pid, self.session_of(pid)?))) else {
            return Err(forbidden(PERSON_ONLY));
        };
        let row = self.row(id)?;
        let session = self.session_key(&hit);
        if row.session != session {
            return Err(forbidden(PERSON_ONLY));
        }
        let is_main = main_may_see(&row, &session)
            && self
                .finder
                .get()
                .and_then(|finder| finder.find_main(&hit, pid))
                .is_some_and(|found| found.asker == Asker::Main);
        let who = Who::Agent {
            session: &session,
            is_main,
        };
        self.answer_as(id, text, choice, By::Main, &who)
    }
}
