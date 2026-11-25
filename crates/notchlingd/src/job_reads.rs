use super::{bad, caller_of, JobAsks, JobRow, NO_JOB};
use crate::job_book::Record;
use notchling_local_api::{ApiError, Caller};
use serde_json::{json, Value};

fn full(job: &JobRow, record: Option<&Record>) -> Value {
    json!({
        "id": job.id, "own": true, "kind": "dispatch", "state": job.state,
        "tool": job.tool, "machine": job.machine, "organization": job.organization,
        "folder": job.folder, "createdMs": job.created_ms, "session": job.session,
        "detail": job.detail, "secrets": job.secrets,
        "dispatchedBy": record.map(|r| r.session.clone()),
    })
}

fn delegate_state(record: &Record) -> &'static str {
    match (record.ended_ms, record.exit) {
        (None, _) => "running",
        (Some(_), Some(0) | None) => "done",
        _ => "failed",
    }
}

fn delegate_full(id: &str, record: &Record) -> Value {
    json!({
        "id": id, "own": true, "kind": "delegate", "state": delegate_state(record),
        "tool": record.tool, "machine": Value::Null, "organization": record.organization,
        "folder": record.folder, "createdMs": record.started_ms, "endedMs": record.ended_ms,
        "exit": record.exit,
    })
}

fn state_only(id: &str, state: &str) -> Value {
    json!({"id": id, "own": false, "state": state})
}

impl JobAsks {
    fn session_of(&self, caller: &Caller) -> Option<(String, String)> {
        caller_of(&self.consent.who(caller)).ok()
    }

    pub(super) fn mine(&self, caller: &Caller) -> Result<Value, ApiError> {
        self.gate()?;
        let Some((session, organization)) = self.session_of(caller) else {
            return Ok(json!({"jobs": [], "reason": "no_session"}));
        };
        let rows = self.fleet.jobs();
        let mut out: Vec<Value> = Vec::new();
        let mut seen: Vec<String> = Vec::new();
        for (id, record) in self.book.mine(&session) {
            seen.push(id.clone());
            match rows.iter().find(|r| r.id == id) {
                Some(job) => out.push(full(job, Some(&record))),
                None if record.kind == "delegate" => out.push(delegate_full(&id, &record)),
                None => {}
            }
        }
        for job in &rows {
            if seen.contains(&job.id) {
                continue;
            }
            if organization != job.organization {
                continue;
            }
            out.push(state_only(&job.id, &job.state));
        }
        Ok(json!({"jobs": out}))
    }

    pub(super) fn get(&self, caller: &Caller, args: &Value) -> Result<Value, ApiError> {
        let id = args
            .get("id")
            .and_then(Value::as_str)
            .filter(|i| !i.trim().is_empty())
            .ok_or_else(|| bad("jobs.get takes {\"id\": text}"))?;
        self.gate()?;
        let (session, organization) = match self.session_of(caller) {
            Some((session, organization)) => (Some(session), Some(organization)),
            None => (None, None),
        };
        let record = self.book.get(id);
        let own = session
            .as_deref()
            .is_some_and(|s| record.as_ref().is_some_and(|r| r.session == s));
        let rows = self.fleet.jobs();
        if let Some(job) = rows.iter().find(|j| j.id == id) {
            if own {
                return Ok(full(job, record.as_ref()));
            }
            if organization.as_deref() != Some(job.organization.as_str()) {
                return Err(bad(NO_JOB).with_reason("no_job"));
            }
            return Ok(state_only(id, &job.state));
        }
        match record {
            Some(record) if own && record.kind == "delegate" => Ok(delegate_full(id, &record)),
            _ => Err(bad(NO_JOB).with_reason("no_job")),
        }
    }
}
