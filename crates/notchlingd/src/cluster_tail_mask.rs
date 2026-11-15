use crate::cluster_node::LocalCalls;
use crate::cluster_redact::{mask_tail, JobMasks, TailMasks};
use crate::cluster_sessions::{tail, LocalRows, NODE_TAIL};
use crate::grant_broker::View;
use crate::grant_roots::valid_job;
use crate::shell_jobs::SESSION_PREFIX;
use notchling_engine::bridge::net::crypto::{base64, base64_decode};
use notchling_engine::cluster::wire::Origin;
use notchling_engine::cluster::JobState;
use notchling_local_api::{ApiError, ErrorCode};
use serde_json::Value;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;
use std::sync::Arc;

pub type Closed = Arc<dyn Fn(&str) -> bool + Send + Sync>;

pub struct MaskedTails {
    inner: Arc<dyn LocalCalls>,
    rows: Arc<dyn LocalRows>,
    masks: Arc<TailMasks>,
    view: Arc<dyn View>,
    closed: Closed,
}

fn failed(why: impl Into<String>) -> ApiError {
    ApiError::new(ErrorCode::Failed, why)
}

fn lead_of(path: &Path, start: u64, want: usize) -> Result<Vec<u8>, ApiError> {
    let take = start.min(want as u64);
    if take == 0 {
        return Ok(Vec::new());
    }
    let broke = |e: std::io::Error| failed(format!("the transcript: {e}"));
    let mut file = std::fs::File::open(path).map_err(broke)?;
    file.seek(SeekFrom::Start(start - take)).map_err(broke)?;
    let mut lead = Vec::new();
    file.take(take).read_to_end(&mut lead).map_err(broke)?;
    Ok(lead)
}

impl MaskedTails {
    pub fn new(
        inner: Arc<dyn LocalCalls>,
        rows: Arc<dyn LocalRows>,
        (masks, view): (Arc<TailMasks>, Arc<dyn View>),
        closed: Closed,
    ) -> MaskedTails {
        MaskedTails {
            inner,
            rows,
            masks,
            view,
            closed,
        }
    }

    fn job_for(&self, id: &str) -> Option<String> {
        if let Some(job) = id.strip_prefix(SESSION_PREFIX) {
            return valid_job(job).then(|| job.to_string());
        }
        let state = self.view.state()?;
        let me = self.view.me()?;
        state
            .jobs
            .values()
            .find(|j| j.spec.node == me && j.session.as_deref() == Some(id))
            .map(|j| j.id.clone())
    }

    fn over(&self, job: &str) -> bool {
        let Some(state) = self.view.state() else {
            return false;
        };
        let ended = state
            .jobs
            .get(job)
            .is_none_or(|j| !matches!(j.state, JobState::Running | JobState::Dispatching));
        ended && (self.closed)(job)
    }

    fn tail(&self, args: &Value) -> Result<Value, ApiError> {
        let rows = self.rows.rows()?;
        let mut out = tail(&rows, args)?;
        let id = args.get("id").and_then(Value::as_str).unwrap_or_default();
        let Some(job) = self.job_for(id) else {
            return Ok(out);
        };
        if self.masks.state(&job) == JobMasks::Never {
            return Ok(out);
        }
        let data = out
            .get("data")
            .and_then(Value::as_str)
            .and_then(base64_decode)
            .ok_or_else(|| failed("the tail can't be read"))?;
        let next = out.get("next").and_then(Value::as_u64).unwrap_or(0);
        let size = out.get("size").and_then(Value::as_u64).unwrap_or(0);
        let start = next
            .checked_sub(data.len() as u64)
            .ok_or_else(|| failed("the tail can't be read"))?;
        let path = rows
            .iter()
            .find(|row| row.get("id").and_then(Value::as_str) == Some(id))
            .and_then(|row| row.get("transcriptPath").and_then(Value::as_str))
            .ok_or_else(|| failed("that session has no transcript"))?;
        let lead = lead_of(Path::new(path), start, self.masks.lookback(&job))?;
        let at_end = next >= size && self.over(&job);
        let (masked, held_next) =
            mask_tail(&self.masks, &job, start, &lead, &data, at_end).map_err(failed)?;
        out["data"] = Value::String(base64(&masked));
        out["next"] = Value::from(held_next);
        Ok(out)
    }
}

impl LocalCalls for MaskedTails {
    fn call(&self, op: &str, args: &Value, origin: &Origin) -> Result<Value, ApiError> {
        match op {
            NODE_TAIL => self.tail(args),
            _ => self.inner.call(op, args, origin),
        }
    }
}

#[cfg(test)]
#[path = "cluster_tail_mask_tests.rs"]
mod tests;
