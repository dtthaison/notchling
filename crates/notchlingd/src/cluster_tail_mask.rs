use crate::cluster_node::LocalCalls;
use crate::cluster_redact::{mask_tail, mask_with, JobMasks, TailMasks};
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

pub const UNMAPPED_LOST: &str =
    "this machine lost the masks of a job that had secrets; other output isn't served";
const AGY_CONVERSATION: &str = "agy.conversation";

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

    fn job_for(&self, id: &str, row: Option<&Value>) -> Option<String> {
        if let Some(job) = id.strip_prefix(SESSION_PREFIX) {
            return valid_job(job).then(|| job.to_string());
        }
        let state = self.view.state()?;
        let me = self.view.me()?;
        let short = row
            .and_then(|r| r.get("target"))
            .and_then(|t| t.get("job"))
            .and_then(Value::as_str);
        state
            .jobs
            .values()
            .filter(|j| j.spec.node == me)
            .find(|j| {
                j.session
                    .as_deref()
                    .is_some_and(|s| s == id || Some(s) == short)
            })
            .map(|j| j.id.clone())
    }

    fn all_over(&self) -> bool {
        let Some(state) = self.view.state() else {
            return false;
        };
        let me = self.view.me();
        state
            .jobs
            .values()
            .filter(|j| Some(&j.spec.node) == me.as_ref())
            .all(|j| !matches!(j.state, JobState::Running | JobState::Dispatching))
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

    fn mask(
        &self,
        job: Option<&str>,
        (offset, lead, bytes): (u64, &[u8], &[u8]),
        at_end: bool,
    ) -> Result<Option<(Vec<u8>, u64)>, ApiError> {
        match job {
            Some(job) => {
                if self.masks.state(job) == JobMasks::Never {
                    return Ok(None);
                }
                mask_tail(&self.masks, job, offset, lead, bytes, at_end)
                    .map(Some)
                    .map_err(failed)
            }
            None => {
                if self.masks.any_lost() {
                    return Err(failed(UNMAPPED_LOST));
                }
                match self.masks.union() {
                    None => Ok(None),
                    Some(redactor) => mask_with(redactor, offset, lead, bytes, at_end)
                        .map(Some)
                        .map_err(failed),
                }
            }
        }
    }

    fn lookback(&self, job: Option<&str>) -> usize {
        match job {
            Some(job) => self.masks.lookback(job),
            None => self.masks.union().map_or(0, |r| r.lookback()),
        }
    }

    fn tail(&self, args: &Value) -> Result<Value, ApiError> {
        let rows = self.rows.rows()?;
        let mut out = tail(&rows, args)?;
        let id = args.get("id").and_then(Value::as_str).unwrap_or_default();
        let row = rows
            .iter()
            .find(|row| row.get("id").and_then(Value::as_str) == Some(id));
        let job = self.job_for(id, row);
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
        let path = row
            .and_then(|row| row.get("transcriptPath").and_then(Value::as_str))
            .ok_or_else(|| failed("that session has no transcript"))?;
        let lead = lead_of(Path::new(path), start, self.lookback(job.as_deref()))?;
        let over = match &job {
            Some(job) => self.over(job),
            None => self.all_over(),
        };
        let at_end = next >= size && over;
        let Some((masked, held_next)) = self.mask(job.as_deref(), (start, &lead, &data), at_end)?
        else {
            return Ok(out);
        };
        out["data"] = Value::String(base64(&masked));
        out["next"] = Value::from(held_next);
        Ok(out)
    }

    fn conversation(&self, op: &str, args: &Value, origin: &Origin) -> Result<Value, ApiError> {
        let reply = self.inner.call(op, args, origin)?;
        let id = args
            .get("id")
            .or_else(|| args.get("session"))
            .and_then(Value::as_str)
            .unwrap_or_default();
        let rows = self.rows.rows().unwrap_or_default();
        let row = rows
            .iter()
            .find(|row| row.get("id").and_then(Value::as_str) == Some(id));
        let job = self.job_for(id, row);
        let text = reply.to_string();
        let Some((masked, _)) = self.mask(job.as_deref(), (0, &[], text.as_bytes()), true)? else {
            return Ok(reply);
        };
        serde_json::from_slice(&masked).map_err(|_| failed("the conversation can't be masked"))
    }
}

impl LocalCalls for MaskedTails {
    fn call(&self, op: &str, args: &Value, origin: &Origin) -> Result<Value, ApiError> {
        match op {
            NODE_TAIL => self.tail(args),
            AGY_CONVERSATION => self.conversation(op, args, origin),
            _ => self.inner.call(op, args, origin),
        }
    }
}

#[cfg(test)]
#[path = "cluster_tail_mask_tests.rs"]
mod tests;
