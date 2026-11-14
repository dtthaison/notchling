use crate::log_audit::AuditSink;
use crate::log_checks::Checker;
use crate::op_group::OpGroup;
use notchling_events::kinds::security_log::ChainReason;
use notchling_local_api::{ops, ApiError, Caller, ErrorCode, OpSpec, Tier};
use serde_json::{json, Value};
use std::sync::{Arc, Mutex, PoisonError};

pub struct LogOps {
    audit: Arc<Mutex<AuditSink>>,
    checker: Arc<Checker>,
}

impl LogOps {
    pub fn new(audit: Arc<Mutex<AuditSink>>, checker: Arc<Checker>) -> LogOps {
        LogOps { audit, checker }
    }

    fn verify(&self, caller: &Caller, args: &Value) -> Result<Value, ApiError> {
        if let Some(n) = args.get("n").filter(|n| !n.is_null()) {
            if caller.tier != Tier::App {
                return Err(ApiError::new(
                    ErrorCode::Forbidden,
                    "Checking one line is the app's.",
                ));
            }
            let n = n
                .as_u64()
                .ok_or_else(|| ApiError::bad_request("n must be a line number"))?;
            return self
                .checker
                .line(n)
                .map(|line| json!({"line": line}))
                .ok_or_else(|| ApiError::bad_request("no such line"));
        }
        let full = args.get("full").and_then(Value::as_bool).unwrap_or(false);
        Ok(self.checker.run(full).json())
    }

    fn chain_start(&self, args: &Value) -> Result<Value, ApiError> {
        if args.get("confirm") != Some(&json!(true)) {
            return Err(ApiError::bad_request(
                "starting a new chain needs confirm: true",
            ));
        }
        let n = self
            .audit
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .start_new_chain(ChainReason::User)
            .map_err(|_| ApiError::new(ErrorCode::Failed, "a new audit key can't be made"))?;
        self.checker.run(true);
        Ok(json!({"chain_started_n": n}))
    }
}

impl OpGroup for LogOps {
    fn spec(&self, op: &str) -> Option<OpSpec> {
        match op {
            ops::LOG_VERIFY => Some(OpSpec::AGENT),
            ops::LOG_CHAIN_START => Some(OpSpec::APP),
            _ => None,
        }
    }

    fn call(&self, caller: &Caller, op: &str, args: &Value) -> Result<Value, ApiError> {
        match op {
            ops::LOG_VERIFY => self.verify(caller, args),
            ops::LOG_CHAIN_START => self.chain_start(args),
            other => Err(ApiError::internal(format!("{other} has no handler"))),
        }
    }
}
