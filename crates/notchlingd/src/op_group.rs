//! A group of ops the daemon's [`crate::daemon::Ops`] serves beside its own: Milestone 6's switches,
//! accounts, vault and install each add one with [`crate::daemon::Ops::with_group`], so a new area
//! is one call there rather than another field and another branch in every method.

use notchling_local_api::{ApiError, Caller, OpSpec, Tier};
use serde_json::Value;

pub trait OpGroup: Send + Sync {
    /// The op's tier and approval; `None` for an op this group doesn't have.
    fn spec(&self, op: &str) -> Option<OpSpec>;
    /// Runs an op whose [`OpGroup::spec`] was `Some` and whose tier the server checked.
    fn call(&self, caller: &Caller, op: &str, args: &Value) -> Result<Value, ApiError>;
    /// The topics this group publishes on.
    fn topics(&self) -> &[&'static str] {
        &[]
    }
    /// The tier a client needs to follow `topic`, when this group says (`None`: not its topic).
    fn topic_tier(&self, _topic: &str) -> Option<Tier> {
        None
    }
}
