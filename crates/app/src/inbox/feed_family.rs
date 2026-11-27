use notchling_local_api::ClientError;
use serde_json::Value;

use super::{InboxFeed, Transport};
use crate::inbox::consent_family::{applied_note, stale_note, Admin, FamilyApply, Part};

fn is_stale(data: Option<&Value>) -> bool {
    data.and_then(|d| d.get("reason")).and_then(Value::as_str) == Some("stale")
}

impl InboxFeed {
    pub fn allow_family(
        &self,
        id: &str,
        apply: &FamilyApply,
        parts: &[Part],
        admin: Option<&Admin>,
    ) -> Result<Option<String>, String> {
        let Some(transport) = &self.transport else {
            return Err(crate::acting::DETACHED_MESSAGE.into());
        };
        let transport: &dyn Transport = transport.as_ref();
        match transport.call(&apply.op, apply.request(id, parts, admin)) {
            Ok(reply) => Ok(applied_note(&reply, parts)),
            Err(ClientError::Api(api)) if is_stale(api.data.as_ref()) => Err(stale_note(&[])),
            Err(ClientError::Api(api)) => Err(api.message),
            Err(other) => Err(other.to_string()),
        }
    }
}
