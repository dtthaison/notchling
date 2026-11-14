use crate::awake_loop::AwakeLoop;
use crate::op_group::OpGroup;
use notchling_engine::awake::state_json;
use notchling_local_api::{ops, ApiError, Caller, OpSpec};
use serde_json::Value;
use std::sync::Arc;

pub struct AwakeOps {
    awake: Arc<AwakeLoop>,
}

impl AwakeOps {
    pub fn new(awake: Arc<AwakeLoop>) -> AwakeOps {
        AwakeOps { awake }
    }
}

impl OpGroup for AwakeOps {
    fn spec(&self, op: &str) -> Option<OpSpec> {
        (op == ops::AWAKE_STATE).then_some(OpSpec::APP)
    }

    fn call(&self, _caller: &Caller, op: &str, _args: &Value) -> Result<Value, ApiError> {
        match op {
            ops::AWAKE_STATE => Ok(state_json(self.awake.holding())),
            other => Err(ApiError::internal(format!("{other} has no handler"))),
        }
    }
}
