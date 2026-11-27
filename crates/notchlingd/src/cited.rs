use notchling_local_api::{ApiError, Caller};
use serde_json::Value;

pub trait Cited: Send + Sync {
    fn claims(&self, op: &str, args: &Value) -> bool;

    fn apply(
        &self,
        caller: &Caller,
        op: &str,
        args: &Value,
        run: &dyn Fn(&Value) -> Result<Value, ApiError>,
    ) -> Result<Value, ApiError>;
}
