use notchling_local_api::ops;
use serde_json::json;

use crate::acting::Request;

pub const LIMIT: u64 = 500;

pub fn request() -> Request {
    Request {
        op: ops::ACTIVITY_LIST,
        args: json!({ "limit": LIMIT }),
    }
}
