use crate::layout::Layout;
use crate::op_group::OpGroup;
use notchling_engine::activity::{self, Files, Query};
use notchling_local_api::{ops, ApiError, Caller, OpSpec};
use serde_json::Value;

pub struct ActivityOps {
    files: Files,
}

impl ActivityOps {
    pub fn new(files: Files) -> ActivityOps {
        ActivityOps { files }
    }

    pub fn for_daemon(layout: &Layout) -> ActivityOps {
        ActivityOps::new(Files {
            audit: layout.audit_path(),
            remote_log: layout.remote_log_path(),
            account_log: layout.account_log_path(),
        })
    }
}

impl OpGroup for ActivityOps {
    fn spec(&self, op: &str) -> Option<OpSpec> {
        (op == ops::ACTIVITY_LIST).then_some(OpSpec::APP_QUIET)
    }

    fn call(&self, _caller: &Caller, op: &str, args: &Value) -> Result<Value, ApiError> {
        match op {
            ops::ACTIVITY_LIST => {
                let query = Query::from_args(args).map_err(ApiError::bad_request)?;
                Ok(activity::list(&self.files, &query))
            }
            other => Err(ApiError::internal(format!("{other} has no handler"))),
        }
    }
}
