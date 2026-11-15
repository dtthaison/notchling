use crate::cluster_node::LocalCalls;
use crate::cluster_redact::{mask_tail, mask_with, JobMasks, TailMasks};
use crate::cluster_sessions::{tail, LocalRows, NODE_SESSIONS, NODE_TAIL};
use crate::grant_broker::{Clock, View};
use crate::grant_roots::{valid_job, JobRoots};
use crate::shell_jobs::SESSION_PREFIX;
use notchling_engine::bridge::net::crypto::{base64, base64_decode};
use notchling_engine::cluster::wire::Origin;
use notchling_engine::cluster::{ClusterState, JobState, NodeId};
use notchling_engine::vault::Redactor;
use notchling_local_api::{ApiError, ErrorCode};
use serde_json::Value;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;
use std::sync::Arc;

pub const UNMAPPED_LOST: &str =
    "this machine lost the masks of a job that had secrets; other output isn't served";
const AGY_CONVERSATION: &str = "agy.conversation";
const PROMPTS_LIST: &str = "prompts.list";
const ROW_TEXT: [&str; 3] = ["detail", "suggestedReply", "command"];

pub type Closed = Arc<dyn Fn(&str) -> bool + Send + Sync>;

pub struct MaskedTails {
    inner: Arc<dyn LocalCalls>,
    rows: Arc<dyn LocalRows>,
    masks: Arc<TailMasks>,
    view: Arc<dyn View>,
    closed: Closed,
    roots: Arc<JobRoots>,
    clock: Clock,
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

struct Snap {
    state: Option<ClusterState>,
    me: Option<NodeId>,
    union: Option<Redactor>,
    blanket: bool,
    login_blanket: bool,
}

fn running(state: JobState) -> bool {
    matches!(state, JobState::Running | JobState::Dispatching)
}

fn blanked(mut row: Value) -> Value {
    if let Some(fields) = row.as_object_mut() {
        for key in ROW_TEXT {
            fields.remove(key);
        }
    }
    row
}

impl MaskedTails {
    pub fn new(
        inner: Arc<dyn LocalCalls>,
        rows: Arc<dyn LocalRows>,
        (masks, view): (Arc<TailMasks>, Arc<dyn View>),
        closed: Closed,
        (roots, clock): (Arc<JobRoots>, Clock),
    ) -> MaskedTails {
        MaskedTails {
            inner,
            rows,
            masks,
            view,
            closed,
            roots,
            clock,
        }
    }

    fn snap(&self) -> Snap {
        Snap {
            state: self.view.state(),
            me: self.view.me(),
            union: self.masks.union(),
            blanket: self.masks.unidentified_lost(),
            login_blanket: self.masks.login_lost(),
        }
    }

    fn job_for(&self, snap: &Snap, id: &str, row: Option<&Value>) -> Option<String> {
        if let Some(job) = id.strip_prefix(SESSION_PREFIX) {
            return valid_job(job).then(|| job.to_string());
        }
        let short = row
            .and_then(|r| r.get("target"))
            .and_then(|t| t.get("job"))
            .and_then(Value::as_str);
        let from_state = match (&snap.state, &snap.me) {
            (Some(state), Some(me)) => state
                .jobs
                .values()
                .filter(|j| j.spec.node == *me)
                .find(|j| {
                    j.session
                        .as_deref()
                        .is_some_and(|s| s == id || Some(s) == short)
                })
                .map(|j| j.id.clone()),
            _ => None,
        };
        let keyed = self
            .masks
            .job_for_key(id)
            .or_else(|| short.and_then(|key| self.masks.job_for_key(key)));
        match keyed {
            Some(lost) if self.masks.lost(&lost) => Some(lost),
            keyed => from_state.or(keyed),
        }
    }

    fn here(&self, job: &str) -> bool {
        self.roots.is_live(job) || self.roots.is_pending(job, (self.clock)())
    }

    fn all_over(&self) -> bool {
        let Some(state) = self.view.state() else {
            return false;
        };
        let me = self.view.me();
        let listed = state
            .jobs
            .values()
            .filter(|j| Some(&j.spec.node) == me.as_ref())
            .all(|j| !running(j.state));
        let local = self.roots.local((self.clock)());
        listed
            && local
                .live
                .keys()
                .chain(&local.pending)
                .all(|job| state.jobs.contains_key(job))
    }

    fn over(&self, job: &str) -> bool {
        let Some(state) = self.view.state() else {
            return false;
        };
        let ended = match state.jobs.get(job) {
            Some(listed) => !running(listed.state),
            None => !self.here(job),
        };
        ended && (self.closed)(job)
    }

    fn ran_claude(&self, snap: &Snap, job: &str) -> bool {
        let listed = match (&snap.state, &snap.me) {
            (Some(state), Some(me)) => state
                .jobs
                .get(job)
                .is_some_and(|j| j.spec.node == *me && j.session.is_some()),
            _ => false,
        };
        listed || self.roots.ran_claude(job)
    }

    fn mask(
        &self,
        snap: &Snap,
        job: Option<&str>,
        (offset, lead, bytes): (u64, &[u8], &[u8]),
        (at_end, shell_log): (bool, bool),
    ) -> Result<Option<(Vec<u8>, u64)>, ApiError> {
        match job {
            Some(job) => {
                if snap.login_blanket && self.ran_claude(snap, job) {
                    return Err(failed(UNMAPPED_LOST));
                }
                if self.masks.state(job) == JobMasks::Never {
                    return Ok(None);
                }
                if shell_log && self.masks.lost(job) {
                    return Ok(None);
                }
                mask_tail(&self.masks, job, offset, lead, bytes, at_end)
                    .map(Some)
                    .map_err(failed)
            }
            None => {
                if snap.blanket {
                    return Err(failed(UNMAPPED_LOST));
                }
                match &snap.union {
                    None => Ok(None),
                    Some(redactor) => mask_with(redactor.clone(), offset, lead, bytes, at_end)
                        .map(Some)
                        .map_err(failed),
                }
            }
        }
    }

    fn lookback(&self, snap: &Snap, job: Option<&str>) -> usize {
        match job {
            Some(job) => self.masks.lookback(job),
            None => snap.union.as_ref().map_or(0, |r| r.lookback()),
        }
    }

    fn tail(&self, args: &Value) -> Result<Value, ApiError> {
        let rows = self.rows.rows()?;
        let mut out = tail(&rows, args)?;
        let id = args.get("id").and_then(Value::as_str).unwrap_or_default();
        let row = rows
            .iter()
            .find(|row| row.get("id").and_then(Value::as_str) == Some(id));
        let snap = self.snap();
        let job = self.job_for(&snap, id, row);
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
        let lead = lead_of(Path::new(path), start, self.lookback(&snap, job.as_deref()))?;
        let over = match &job {
            Some(job) => self.over(job),
            None => self.all_over(),
        };
        let at_end = next >= size && over;
        let Some((masked, held_next)) = self.mask(
            &snap,
            job.as_deref(),
            (start, &lead, &data),
            (at_end, id.starts_with(SESSION_PREFIX)),
        )?
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
        let snap = self.snap();
        let job = self.job_for(&snap, id, row);
        let text = reply.to_string();
        let Some((masked, _)) = self.mask(
            &snap,
            job.as_deref(),
            (0, &[], text.as_bytes()),
            (true, false),
        )?
        else {
            return Ok(reply);
        };
        serde_json::from_slice(&masked).map_err(|_| failed("the conversation can't be masked"))
    }

    fn session_rows(&self, op: &str, args: &Value, origin: &Origin) -> Result<Value, ApiError> {
        let reply = self.inner.call(op, args, origin)?;
        let Value::Array(list) = reply else {
            return Ok(reply);
        };
        let snap = self.snap();
        let rows = list
            .into_iter()
            .filter_map(|row| {
                let id = row.get("id").and_then(Value::as_str).unwrap_or_default();
                let job = self.job_for(&snap, id, Some(&row));
                let text = row.to_string();
                match self.mask(
                    &snap,
                    job.as_deref(),
                    (0, &[], text.as_bytes()),
                    (true, false),
                ) {
                    Ok(None) => Some(row),
                    Ok(Some((masked, _))) => serde_json::from_slice(&masked).ok(),
                    Err(_) => Some(blanked(row)),
                }
            })
            .collect();
        Ok(Value::Array(rows))
    }

    fn prompts(&self, op: &str, args: &Value, origin: &Origin) -> Result<Value, ApiError> {
        let reply = self.inner.call(op, args, origin)?;
        let Value::Array(list) = reply else {
            return Ok(reply);
        };
        let rows = self.rows.rows().unwrap_or_default();
        let snap = self.snap();
        let served = list
            .into_iter()
            .filter_map(|prompt| {
                let id = prompt
                    .get("sessionId")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                let row = rows
                    .iter()
                    .find(|row| row.get("id").and_then(Value::as_str) == Some(id));
                let job = self.job_for(&snap, id, row);
                let text = prompt.to_string();
                match self.mask(
                    &snap,
                    job.as_deref(),
                    (0, &[], text.as_bytes()),
                    (true, false),
                ) {
                    Ok(None) => Some(prompt),
                    Ok(Some((masked, _))) => serde_json::from_slice(&masked).ok(),
                    Err(_) => None,
                }
            })
            .collect();
        Ok(Value::Array(served))
    }
}

impl LocalCalls for MaskedTails {
    fn call(&self, op: &str, args: &Value, origin: &Origin) -> Result<Value, ApiError> {
        match op {
            NODE_TAIL => self.tail(args),
            AGY_CONVERSATION => self.conversation(op, args, origin),
            PROMPTS_LIST => self.prompts(op, args, origin),
            NODE_SESSIONS => self.session_rows(op, args, origin),
            _ => self.inner.call(op, args, origin),
        }
    }
}

#[cfg(test)]
#[path = "cluster_tail_mask_tests.rs"]
mod tests;
