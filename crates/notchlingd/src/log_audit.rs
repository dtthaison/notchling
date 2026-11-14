use crate::log_bus::{EnvMaker, Extra, Sink, SinkResult, Ticked};
use crate::log_fold::{Fold, FoldAction};
use crate::log_queue::Stamped;
use crate::log_retention::{KeepAudit, CAP_BYTES, RETENTION_EVERY_MS};
use notchling_engine::logs::{
    has_legacy, month_of, AuditDir, Checkpoint, KeyError, KeyRead, KeyStore, LegacyFile,
    UserDeleted,
};
use notchling_events::audit_line::{read_head, sealed_line, unsealed_line, UnsealedWhy};
use notchling_events::kinds::security::{DaemonStarted, DataRoot, Sealing, StartAfter};
use notchling_events::kinds::security_log::{
    AuditChainStarted, AuditChecked, ChainReason, CheckResult,
};
use notchling_events::spec::{Lock, Outcome, Sinks};
use notchling_events::{
    Count, Envelope, Event, Hex, List, RefMode, Salt, Seal, Version, MAX_EVENT_BYTES,
};
use std::io;
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError, TryRecvError};
use std::sync::Arc;
use std::time::{Duration, Instant};

pub const CHECKPOINT_MIN_MS: i64 = 10_000;
pub const CHECKPOINT_EVERY_MS: i64 = 300_000;
const KEY_POLL: Duration = Duration::from_millis(200);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SealState {
    Starting,
    Sealed,
    Unsealed(UnsealedWhy),
}

#[derive(Clone)]
pub struct StartInfo {
    pub daemon: Version,
    pub macos: Version,
    pub data_root: DataRoot,
    pub key_wait: Duration,
    pub env: EnvMaker,
    pub clock: Arc<dyn Fn() -> i64 + Send + Sync>,
}

pub struct AuditSink {
    pub(crate) dir: AuditDir,
    pub(crate) keys: Arc<dyn KeyStore>,
    pub(crate) start: StartInfo,
    pub(crate) salt: Salt,
    pub(crate) legacy: Vec<LegacyFile>,
    pub(crate) state: SealState,
    pub(crate) key: Option<[u8; 32]>,
    pub(crate) n: u64,
    pub(crate) last_seal: Option<Seal>,
    pub(crate) last_seal_n: u64,
    pub(crate) month: u32,
    pub(crate) carried: Carried,
    pub(crate) keep: KeepAudit,
    pub(crate) cap_bytes: u64,
    last_retention_ms: Option<i64>,
    key_rx: Option<Receiver<KeyRead>>,
    begun: Instant,
    fresh: bool,
    after: StartAfter,
    held: Vec<Stamped>,
    made: Vec<Extra>,
    fold: Fold,
    stretch: Option<(u64, u64)>,
    cp_want: bool,
    cp_last_ms: i64,
    cp_n: u64,
}

enum Offered {
    Written(u64),
    Held,
    Failed,
}

#[derive(Clone, Default)]
pub struct Carried {
    pub settings_hash: Option<String>,
    pub prune_anchor: Option<String>,
    pub user_deleted: UserDeleted,
}

fn last_sealed_before(dir: &AuditDir) -> Option<(u64, Seal)> {
    let months = dir.months().ok()?;
    for month in months.into_iter().rev() {
        let text = dir.read(month).ok()?;
        for line in text.lines().rev() {
            if let Some(head) = read_head(line) {
                if let Some(seal) = head.seal {
                    return Some((head.n, seal));
                }
            }
        }
    }
    None
}

impl AuditSink {
    pub fn new(
        dir: AuditDir,
        keys: Arc<dyn KeyStore>,
        start: StartInfo,
        legacy: Vec<LegacyFile>,
        salt: Salt,
    ) -> AuditSink {
        let tail = dir.tail_line().ok().flatten();
        let head = tail.as_deref().and_then(read_head);
        let (last_seal_n, last_seal) = match &head {
            Some(h) if h.seal.is_some() => (h.n, h.seal.clone()),
            Some(_) => match last_sealed_before(&dir) {
                Some((n, seal)) => (n, Some(seal)),
                None => (0, None),
            },
            None => (0, None),
        };
        let after = match &head {
            Some(h) if h.kind == "daemon.stopped" => StartAfter::CleanStop,
            _ => StartAfter::Unknown,
        };
        let months = dir.months().unwrap_or_default();
        let checkpoint = keys.read_head();
        let fresh =
            months.is_empty() && checkpoint.is_none() && !keys.head_exists() && !has_legacy(&dir);
        let carried = checkpoint
            .as_ref()
            .map(|c| Carried {
                settings_hash: c.settings_hash.clone(),
                prune_anchor: c.prune_anchor.clone(),
                user_deleted: c.user_deleted.clone(),
            })
            .unwrap_or_default();
        let month = months.last().copied().unwrap_or(0);
        let (tx, rx) = channel();
        let reader = keys.clone();
        let spawned = std::thread::Builder::new()
            .name("notchling-log-key".into())
            .spawn(move || {
                let _ = tx.send(reader.read_key());
            });
        let key_rx = spawned.is_ok().then_some(rx);
        AuditSink {
            dir,
            keys,
            start,
            salt,
            legacy,
            state: SealState::Starting,
            key: None,
            n: head.as_ref().map_or(0, |h| h.n),
            last_seal,
            last_seal_n,
            month,
            carried,
            keep: KeepAudit::Year1,
            cap_bytes: CAP_BYTES,
            last_retention_ms: None,
            key_rx,
            begun: Instant::now(),
            fresh,
            after,
            held: Vec::new(),
            made: Vec::new(),
            fold: Fold::default(),
            stretch: None,
            cp_want: false,
            cp_last_ms: 0,
            cp_n: head.as_ref().map_or(0, |h| h.n),
        }
    }

    pub fn sealing(&self) -> SealState {
        self.state
    }

    pub fn head(&self) -> (u64, Option<Seal>) {
        (self.last_seal_n, self.last_seal.clone())
    }

    pub fn start_new_chain(&mut self, reason: ChainReason) -> Result<u64, KeyError> {
        let previous = self.last_seal.clone().map(|s| (self.last_seal_n, s));
        let key = self.keys.create_key()?;
        self.key = Some(key);
        self.state = SealState::Sealed;
        self.last_seal = None;
        let event = Event::new(
            &self.chain_started(reason, previous),
            Outcome::Info,
            self.now(),
        );
        let n = self.make(event).ok_or(KeyError::Io(io::ErrorKind::Other))?;
        let _ = self.dir.flush(true);
        let now = self.now();
        let _ = self.write_checkpoint(now);
        Ok(n)
    }

    fn now(&self) -> i64 {
        (self.start.clock)()
    }

    pub(crate) fn now_ms(&self) -> i64 {
        self.now()
    }

    pub(crate) fn write_checkpoint_now(&mut self, now: i64) -> io::Result<()> {
        self.write_checkpoint(now)
    }

    fn poll_key(&mut self, wait: Duration) {
        let Some(rx) = &self.key_rx else {
            if self.state == SealState::Starting {
                self.settle(None);
            }
            return;
        };
        let answer = match rx.try_recv() {
            Ok(read) => Some(read),
            Err(TryRecvError::Disconnected) => {
                self.key_rx = None;
                if self.state == SealState::Starting {
                    self.settle(None);
                }
                return;
            }
            Err(TryRecvError::Empty) if self.state == SealState::Starting && !wait.is_zero() => {
                let left = self.start.key_wait.saturating_sub(self.begun.elapsed());
                match rx.recv_timeout(wait.min(left)) {
                    Ok(read) => Some(read),
                    Err(RecvTimeoutError::Disconnected) => {
                        self.key_rx = None;
                        None
                    }
                    Err(RecvTimeoutError::Timeout) => None,
                }
            }
            Err(TryRecvError::Empty) => None,
        };
        if let Some(read) = answer {
            self.key_rx = None;
            match self.state {
                SealState::Starting => self.settle(Some(read)),
                SealState::Unsealed(UnsealedWhy::KeyWaiting) => self.resume(read),
                SealState::Sealed | SealState::Unsealed(_) => {}
            }
        } else if self.state == SealState::Starting && self.begun.elapsed() >= self.start.key_wait {
            self.settle(None);
        }
    }

    fn settle(&mut self, read: Option<KeyRead>) {
        let mut chain: Option<ChainReason> = None;
        match read {
            Some(KeyRead::Found(key)) => {
                self.key = Some(key);
                self.state = SealState::Sealed;
            }
            Some(KeyRead::Missing) => {
                if self.fresh || !self.legacy.is_empty() {
                    match self.keys.create_key() {
                        Ok(key) => {
                            self.key = Some(key);
                            self.state = SealState::Sealed;
                            self.last_seal = None;
                            chain = Some(ChainReason::FirstRun);
                        }
                        Err(_) => self.state = SealState::Unsealed(UnsealedWhy::KeyMissing),
                    }
                } else {
                    self.state = SealState::Unsealed(UnsealedWhy::KeyMissing);
                }
            }
            Some(KeyRead::Denied) => self.state = SealState::Unsealed(UnsealedWhy::KeyDenied),
            None => self.state = SealState::Unsealed(UnsealedWhy::KeyWaiting),
        }
        if let Some(reason) = chain {
            self.make(Event::new(
                &self.chain_started(reason, None),
                Outcome::Info,
                self.now(),
            ));
        }
        self.make(Event::new(
            &DaemonStarted {
                daemon: self.start.daemon.clone(),
                macos: self.start.macos.clone(),
                data_root: self.start.data_root,
                after: self.after,
                sealing: match self.state {
                    SealState::Sealed => Sealing::Sealed,
                    SealState::Unsealed(UnsealedWhy::KeyWaiting) => Sealing::Waiting,
                    _ => Sealing::Unsealed,
                },
            },
            Outcome::Info,
            self.now(),
        ));
        if matches!(self.state, SealState::Unsealed(_)) {
            self.make(Event::new(
                &AuditChecked {
                    result: CheckResult::Unsealed,
                    from_n: None,
                    to_n: None,
                    first_bad_n: None,
                },
                Outcome::Info,
                self.now(),
            ));
        }
        for held in std::mem::take(&mut self.held) {
            let _ = self.put_offered(&held);
        }
    }

    fn resume(&mut self, read: KeyRead) {
        let KeyRead::Found(key) = read else {
            return;
        };
        self.key = Some(key);
        self.state = SealState::Sealed;
        let (from, to) = self.stretch.take().unwrap_or((self.n, self.n));
        self.make(Event::new(
            &AuditChecked {
                result: CheckResult::Resumed,
                from_n: Some(Count(from)),
                to_n: Some(Count(to)),
                first_bad_n: None,
            },
            Outcome::Info,
            self.now(),
        ));
    }

    pub(crate) fn chain_started(
        &self,
        reason: ChainReason,
        previous: Option<(u64, Seal)>,
    ) -> AuditChainStarted {
        let first = reason == ChainReason::FirstRun;
        AuditChainStarted {
            reason,
            previous_head_n: previous.as_ref().map(|(n, _)| Count(*n)),
            previous_head_seal: previous.map(|(_, seal)| seal),
            legacy_files: (first && !self.legacy.is_empty())
                .then_some(Count(self.legacy.len() as u64)),
            legacy_lines: (first && !self.legacy.is_empty())
                .then(|| Count(self.legacy.iter().map(|f| f.lines).sum())),
            legacy_sha: List::new(
                self.legacy
                    .iter()
                    .filter(|_| first)
                    .map(|f| f.sha256.clone())
                    .collect::<Vec<Hex>>(),
            ),
        }
    }

    pub(crate) fn make(&mut self, event: Event) -> Option<u64> {
        let stamped = Stamped {
            env: self.start.env.make(None, None),
            event,
            commit: None,
        };
        let n = self.put(&stamped);
        self.made.push(Extra {
            stamped,
            audit_n: n,
        });
        n
    }

    fn put_offered(&mut self, stamped: &Stamped) -> Offered {
        let now = stamped.event.ts;
        for due in self.fold.due(now) {
            let n = self.put(&due);
            self.made.push(Extra {
                stamped: due,
                audit_n: n,
            });
        }
        match self.fold.offer(stamped, now) {
            FoldAction::Write => match self.put(stamped) {
                Some(n) => Offered::Written(n),
                None => Offered::Failed,
            },
            FoldAction::Hold => Offered::Held,
        }
    }

    fn render(&self, n: u64, env: &Envelope, event: &Event) -> (String, Option<Seal>) {
        let mode = RefMode::Plain(self.salt);
        match (self.state, self.key.as_ref()) {
            (SealState::Sealed, Some(key)) => {
                let (line, seal) = sealed_line(key, n, self.last_seal.as_ref(), env, event, &mode);
                (line, Some(seal))
            }
            (SealState::Unsealed(why), _) => (unsealed_line(n, why, env, event, &mode), None),
            _ => (
                unsealed_line(n, UnsealedWhy::KeyWaiting, env, event, &mode),
                None,
            ),
        }
    }

    fn fitted(&self, n: u64, env: &Envelope, event: &Event) -> (String, Option<Seal>) {
        let mut event = event.clone();
        let mut env = env.clone();
        for _ in 0..3 {
            let (line, seal) = self.render(n, &env, &event);
            if line.len() < MAX_EVENT_BYTES {
                return (line, seal);
            }
            let over = line.len() - MAX_EVENT_BYTES + 1;
            let budget = event.fields.json_len().saturating_sub(over + 8);
            event.fields.fit(budget);
        }
        env.actor = None;
        env.op_id = None;
        self.render(n, &env, &event)
    }

    pub(crate) fn put(&mut self, stamped: &Stamped) -> Option<u64> {
        let n = self.n + 1;
        let (line, seal) = self.fitted(n, &stamped.env, &stamped.event);
        let month = month_of(stamped.event.ts).max(self.month);
        let mut bytes = line.into_bytes();
        bytes.push(b'\n');
        if self.dir.append(month, &bytes).is_err() {
            return None;
        }
        self.month = month;
        self.n = n;
        if let Some(seal) = seal {
            self.last_seal = Some(seal);
            self.last_seal_n = n;
        } else if let SealState::Unsealed(UnsealedWhy::KeyWaiting) = self.state {
            let first = self.stretch.map_or(n, |(from, _)| from);
            self.stretch = Some((first, n));
        }
        if matches!(stamped.event.spec.lock, Lock::Always(_)) {
            self.cp_want = true;
        }
        Some(n)
    }

    fn write_checkpoint(&mut self, now: i64) -> io::Result<()> {
        let Some(seal) = self.last_seal.clone() else {
            return Ok(());
        };
        if self.state != SealState::Sealed {
            return Ok(());
        }
        let checkpoint = Checkpoint {
            n: self.n,
            seal: seal.as_str().to_owned(),
            month: self.month,
            settings_hash: self.carried.settings_hash.clone(),
            prune_anchor: self.carried.prune_anchor.clone(),
            user_deleted: self.carried.user_deleted.clone(),
            at: now,
        };
        self.keys
            .write_head(&checkpoint)
            .map_err(|_| io::Error::other("the audit checkpoint can't be written"))?;
        self.cp_last_ms = now;
        self.cp_want = false;
        self.cp_n = self.n;
        Ok(())
    }

    fn take_made(&mut self) -> Vec<Extra> {
        std::mem::take(&mut self.made)
    }
}

impl Sink for AuditSink {
    fn sinks(&self) -> Sinks {
        Sinks::AUDIT
    }

    fn write(&mut self, batch: &[Stamped], _audit_n: &[Option<u64>]) -> SinkResult {
        let mut result = SinkResult::written(batch.len());
        self.poll_key(KEY_POLL);
        if self.state == SealState::Starting {
            for stamped in batch {
                if stamped.event.spec.sinks.has(Sinks::AUDIT) {
                    self.held.push(stamped.clone());
                }
            }
            result.extra = self.take_made();
            return result;
        }
        for (i, stamped) in batch.iter().enumerate() {
            if !stamped.event.spec.sinks.has(Sinks::AUDIT) {
                continue;
            }
            match self.put_offered(stamped) {
                Offered::Written(n) => result.audit_n[i] = Some(n),
                Offered::Held => {}
                Offered::Failed => result.failed = true,
            }
        }
        result.extra = self.take_made();
        result
    }

    fn flush(&mut self, full: bool) -> io::Result<()> {
        self.dir.flush(full)?;
        let now = self.now();
        if self.cp_want && now - self.cp_last_ms >= CHECKPOINT_MIN_MS {
            self.write_checkpoint(now)?;
        }
        Ok(())
    }

    fn tick(&mut self, now_ms: i64) -> Ticked {
        self.poll_key(Duration::ZERO);
        for due in self.fold.due(now_ms) {
            let n = self.put(&due);
            self.made.push(Extra {
                stamped: due,
                audit_n: n,
            });
        }
        if self.state != SealState::Starting
            && self
                .last_retention_ms
                .is_none_or(|last| now_ms - last >= RETENTION_EVERY_MS)
        {
            self.last_retention_ms = Some(now_ms);
            self.retention(now_ms);
        }
        if self.state == SealState::Sealed
            && self.n > self.cp_n
            && now_ms - self.cp_last_ms >= CHECKPOINT_EVERY_MS
        {
            let _ = self.dir.flush(false);
            let _ = self.write_checkpoint(now_ms);
        }
        Ticked {
            events: Vec::new(),
            extra: self.take_made(),
        }
    }
}

pub struct SharedAudit(pub Arc<std::sync::Mutex<AuditSink>>);

impl SharedAudit {
    fn sink(&self) -> std::sync::MutexGuard<'_, AuditSink> {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

impl Sink for SharedAudit {
    fn sinks(&self) -> Sinks {
        Sinks::AUDIT
    }

    fn write(&mut self, batch: &[Stamped], audit_n: &[Option<u64>]) -> SinkResult {
        self.sink().write(batch, audit_n)
    }

    fn flush(&mut self, full: bool) -> io::Result<()> {
        self.sink().flush(full)
    }

    fn tick(&mut self, now_ms: i64) -> Ticked {
        self.sink().tick(now_ms)
    }
}
