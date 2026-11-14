use crate::log_audit::AuditSink;
use notchling_engine::logs::{month_end_ms, month_of};
use notchling_events::audit_line::read_head;
use notchling_events::kinds::choices::PruneReason;
use notchling_events::kinds::security_log::AuditPruned;
use notchling_events::spec::Outcome;
use notchling_events::{Count, Event, List, Seal, Time};
use std::io;

pub const DAY_MS: i64 = 86_400_000;
pub const CAP_BYTES: u64 = 500 * 1024 * 1024;
pub const USER_DELETE_AGE_DAYS: i64 = 30;
pub const RETENTION_EVERY_MS: i64 = 3_600_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeepAudit {
    Days90,
    Year1,
    Years2,
}

impl KeepAudit {
    pub fn days(self) -> i64 {
        match self {
            KeepAudit::Days90 => 90,
            KeepAudit::Year1 => 365,
            KeepAudit::Years2 => 730,
        }
    }
}

#[derive(Debug)]
pub enum PruneError {
    Current,
    Missing,
    Io(io::ErrorKind),
}

pub fn months_to_prune(
    months: &[u32],
    sizes: &[u64],
    now_ms: i64,
    keep: KeepAudit,
    cap_bytes: u64,
) -> Vec<(u32, PruneReason)> {
    let current = month_of(now_ms);
    let mut order: Vec<(u32, u64)> = months.iter().copied().zip(sizes.iter().copied()).collect();
    order.sort_by_key(|(month, _)| *month);
    let cutoff = now_ms - keep.days() * DAY_MS;
    let mut picked: Vec<(u32, PruneReason)> = Vec::new();
    let mut total: u64 = order.iter().map(|(_, size)| *size).sum();
    for (month, size) in &order {
        if *month >= current {
            break;
        }
        if month_end_ms(*month) <= cutoff {
            picked.push((*month, PruneReason::Retention));
            total = total.saturating_sub(*size);
        } else {
            break;
        }
    }
    for (month, size) in &order {
        if total <= cap_bytes || *month >= current {
            break;
        }
        if picked.iter().any(|(m, _)| m == month) {
            continue;
        }
        picked.push((*month, PruneReason::SizeCap));
        total = total.saturating_sub(*size);
    }
    picked.sort_by_key(|(month, _)| *month);
    picked
}

pub fn deletable(month: u32, now_ms: i64) -> bool {
    month < month_of(now_ms) && month_end_ms(month) + USER_DELETE_AGE_DAYS * DAY_MS <= now_ms
}

impl AuditSink {
    pub fn prune(&mut self, month: u32, reason: PruneReason) -> Result<u64, PruneError> {
        let now = self.now_ms();
        if month >= self.month.max(month_of(now)) {
            return Err(PruneError::Current);
        }
        let text = self.dir.read(month).map_err(|e| match e.kind() {
            io::ErrorKind::NotFound => PruneError::Missing,
            other => PruneError::Io(other),
        })?;
        let lines = text.lines().filter(|l| !l.trim().is_empty()).count() as u64;
        let last_seal: Seal = text
            .lines()
            .rev()
            .filter_map(read_head)
            .find_map(|head| head.seal)
            .or_else(|| self.last_seal.clone())
            .unwrap_or_else(|| Seal::from_bytes([0; 32]));
        if reason == PruneReason::User {
            self.carried.user_deleted.push(month, now);
        }
        let deleted = self.carried.user_deleted.clone();
        let event = Event::new(
            &AuditPruned {
                through_month: Count(u64::from(month)),
                last_seal: last_seal.clone(),
                lines: Count(lines),
                reason,
                user_deleted_months: List::new(
                    deleted
                        .months
                        .iter()
                        .map(|m| Count(u64::from(*m)))
                        .collect(),
                ),
                user_deleted_at: List::new(deleted.at.iter().map(|at| Time(*at)).collect()),
                user_deleted_count: Count(deleted.count),
            },
            Outcome::Info,
            now,
        );
        let n = self
            .make(event)
            .ok_or(PruneError::Io(io::ErrorKind::Other))?;
        self.dir.flush(true).map_err(|e| PruneError::Io(e.kind()))?;
        self.carried.prune_anchor = Some(last_seal.as_str().to_owned());
        self.write_checkpoint_now(now)
            .map_err(|e| PruneError::Io(e.kind()))?;
        self.dir
            .remove(month)
            .map_err(|e| PruneError::Io(e.kind()))?;
        Ok(n)
    }

    pub fn retention(&mut self, now_ms: i64) {
        let months = self.dir.months().unwrap_or_default();
        let sizes: Vec<u64> = months.iter().map(|m| self.dir.month_size(*m)).collect();
        for (month, reason) in months_to_prune(&months, &sizes, now_ms, self.keep, self.cap_bytes) {
            if self.prune(month, reason).is_err() {
                break;
            }
        }
    }

    pub fn set_keep(&mut self, keep: KeepAudit) {
        self.keep = keep;
    }

    pub fn set_cap(&mut self, bytes: u64) {
        self.cap_bytes = bytes;
    }

    pub fn delete_through(&mut self, through: u32) -> Result<(u64, Option<u64>), PruneError> {
        let now = self.now_ms();
        let months: Vec<u32> = self
            .dir
            .months()
            .map_err(|e| PruneError::Io(e.kind()))?
            .into_iter()
            .filter(|m| *m <= through)
            .collect();
        if months.iter().any(|m| !deletable(*m, now)) {
            return Err(PruneError::Current);
        }
        let mut last = None;
        for month in &months {
            last = Some(self.prune(*month, PruneReason::User)?);
        }
        Ok((months.len() as u64, last))
    }
}
