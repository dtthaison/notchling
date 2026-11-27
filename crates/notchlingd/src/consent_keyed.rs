use crate::consent_ops::{ConsentOps, Opening};
use notchling_engine::consent::{clean_reason, spec, Consent, New};
use notchling_local_api::{ApiError, Caller};
use serde_json::Value;

impl ConsentOps {
    pub(crate) fn build(&self, caller: &Caller, o: Opening) -> Result<New, ApiError> {
        let spec = spec(o.kind).ok_or_else(|| ApiError::internal("no such kind"))?;
        let expires_in_ms = Self::expiry(o.kind, o.expires_in_ms)?;
        let who = self.who(caller);
        let started = caller.pid.and_then(|pid| {
            crate::vault_caller::Processes::start_time(&crate::vault_caller::SystemProcesses, pid)
        });
        Ok(New {
            kind: o.kind.to_string(),
            target: o.target,
            change: o.change,
            facts: o.facts,
            request: o.request,
            parts: o.parts,
            reason: o.reason.map(|r| clean_reason(&r)).filter(|r| !r.is_empty()),
            session: who.session.clone().filter(|_| who.verified),
            tool: who.tool.name().to_string(),
            verified: who.verified,
            root: who.root.clone(),
            folder: who.folder.clone(),
            organization: who.organization.clone(),
            pid: caller.pid.unwrap_or(0),
            pid_start: started,
            idem_key: o
                .key
                .filter(|k| !k.is_empty())
                .map(|k| k.chars().take(64).collect()),
            answer_on: spec.answer_on,
            applied_by: spec.applied_by,
            expires_in_ms,
        })
    }

    pub fn earlier(
        &self,
        caller: &Caller,
        kind: &'static str,
        key: &str,
    ) -> Result<Option<Consent>, ApiError> {
        let probe = self.build(
            caller,
            Opening {
                parts: None,
                kind,
                target: String::new(),
                change: Value::Null,
                facts: Value::Null,
                request: Value::Null,
                reason: None,
                expires_in_ms: None,
                key: Some(key.to_string()),
                retry_of: None,
            },
        )?;
        self.store
            .earlier(&probe, self.now())
            .map_err(Self::refusal)
    }
}
