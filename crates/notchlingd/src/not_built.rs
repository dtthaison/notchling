use crate::op_group::OpGroup;
use notchling_local_api::{ApiError, Caller, ErrorCode, OpSpec};
use serde_json::Value;

pub struct NotBuilt {
    pub ops: &'static [(&'static str, OpSpec)],
}

impl OpGroup for NotBuilt {
    fn spec(&self, op: &str) -> Option<OpSpec> {
        self.ops
            .iter()
            .find(|(name, _)| *name == op)
            .map(|(_, s)| *s)
    }

    fn call(&self, _caller: &Caller, op: &str, _args: &Value) -> Result<Value, ApiError> {
        Err(ApiError::new(
            ErrorCode::Unimplemented,
            format!("{op} is not built yet"),
        ))
    }
}
