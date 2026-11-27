use crate::log_audit::{AuditSink, Carried, SealState};
use crate::log_keys::Loaded;
use notchling_engine::logs::KeyRead;
use notchling_events::audit_line::UnsealedWhy;
use notchling_events::kinds::security::{DaemonStarted, Sealing};
use notchling_events::kinds::security_log::{AuditChecked, ChainReason, CheckResult};
use notchling_events::spec::Outcome;
use notchling_events::{Count, Event};
use std::sync::mpsc::{RecvTimeoutError, TryRecvError};
use std::time::Duration;

impl AuditSink {
    pub(crate) fn poll_key(&mut self, wait: Duration) {
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

    fn apply(&mut self, loaded: Loaded) -> Option<ChainReason> {
        self.loaded = true;
        self.carried = loaded
            .checkpoint
            .as_ref()
            .map(|c| Carried {
                settings_hash: c.settings_hash.clone(),
                prune_anchor: c.prune_anchor.clone(),
                user_deleted: c.user_deleted.clone(),
            })
            .unwrap_or_default();
        self.checkpoint = loaded.checkpoint;
        match (loaded.read, loaded.created) {
            (KeyRead::Found(key), _) => {
                self.key = Some(key);
                self.state = SealState::Sealed;
                None
            }
            (KeyRead::Missing, Some(Ok(key))) => {
                self.key = Some(key);
                self.state = SealState::Sealed;
                self.last_seal = None;
                Some(ChainReason::FirstRun)
            }
            (KeyRead::Missing, _) => {
                self.state = SealState::Unsealed(UnsealedWhy::KeyMissing);
                None
            }
            (KeyRead::Denied, _) => {
                self.state = SealState::Unsealed(UnsealedWhy::KeyDenied);
                None
            }
        }
    }

    fn unsealed_check(&mut self) {
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

    fn settle(&mut self, loaded: Option<Loaded>) {
        let chain = match loaded {
            Some(loaded) => self.apply(loaded),
            None => {
                self.state = SealState::Unsealed(UnsealedWhy::KeyWaiting);
                None
            }
        };
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
            self.unsealed_check();
        }
        for held in std::mem::take(&mut self.held) {
            let _ = self.put_offered(&held);
        }
    }

    fn resume(&mut self, loaded: Loaded) {
        if let Some(reason) = self.apply(loaded) {
            self.make(Event::new(
                &self.chain_started(reason, None),
                Outcome::Info,
                self.now(),
            ));
        }
        if self.state != SealState::Sealed {
            self.unsealed_check();
            return;
        }
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
}
