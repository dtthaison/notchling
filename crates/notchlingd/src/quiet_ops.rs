use notchling_local_api::{ops, OpSpec};
use serde_json::Value;

use crate::limits_ops::LIMITS_STATUS;
use crate::limits_usage::LIMITS_USAGE;

pub const QUIET_READS: [&str; 9] = [
    ops::BRIDGE_STATUS,
    ops::BRIDGE_DEVICES,
    ops::BRIDGE_TUNNEL_STATUS,
    ops::ACCOUNTS_LIST,
    ops::LANES_LIST,
    ops::PROVIDERS_LIST,
    ops::PROVIDERS_USAGE,
    LIMITS_STATUS,
    LIMITS_USAGE,
];

const SYNCING: [(&str, &str); 4] = [
    (ops::ACCOUNTS_LIST, "sync"),
    (ops::PROVIDERS_LIST, "sync"),
    (ops::PROVIDERS_USAGE, "refresh"),
    (LIMITS_USAGE, "refresh"),
];

pub fn quiet(spec: Option<OpSpec>, op: &str) -> Option<OpSpec> {
    match spec {
        Some(s) if s == OpSpec::APP && QUIET_READS.contains(&op) => Some(OpSpec::APP_QUIET),
        other => other,
    }
}

pub fn loud(op: &str, args: &Value) -> bool {
    SYNCING
        .iter()
        .any(|(name, flag)| *name == op && args.get(*flag).and_then(Value::as_bool) == Some(true))
}
