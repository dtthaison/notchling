use notchling_local_api::{ApiError, Caller, OpSpec, Tier};
use serde_json::Value;

pub trait OpGroup: Send + Sync {
    fn spec(&self, op: &str) -> Option<OpSpec>;
    fn call(&self, caller: &Caller, op: &str, args: &Value) -> Result<Value, ApiError>;
    fn topics(&self) -> &[&'static str] {
        &[]
    }
    fn topic_tier(&self, _topic: &str) -> Option<Tier> {
        None
    }
}
