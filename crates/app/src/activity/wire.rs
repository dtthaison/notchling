use notchling_local_api::ops;
use serde_json::json;

use crate::acting::Request;

pub const LIMIT: u64 = 500;

pub fn verify_request(n: u64) -> Request {
    Request {
        op: ops::LOG_VERIFY,
        args: json!({ "n": n }),
    }
}

pub fn request() -> Request {
    Request {
        op: ops::ACTIVITY_LIST,
        args: json!({ "limit": LIMIT }),
    }
}
