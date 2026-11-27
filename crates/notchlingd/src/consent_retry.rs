use crate::consent_ops::{failed, ConsentOps};
use notchling_engine::consent::retry::{check_retry, ALREADY, NO_OLD};
use notchling_engine::consent::{New, Refusal};
use notchling_local_api::ApiError;
use serde_json::Value;

impl ConsentOps {
    pub(crate) fn retried(&self, old: &str, new: &New) -> Result<Value, ApiError> {
        let row = self
            .store
            .get(old)
            .map_err(failed)?
            .ok_or_else(|| Self::refusal(Refusal::RetryRefused(NO_OLD)))?;
        if self.store.retried_by(old).map_err(failed)? {
            return Err(Self::refusal(Refusal::RetryRefused(ALREADY)));
        }
        check_retry(&row, new).map_err(Self::refusal)
    }
}
