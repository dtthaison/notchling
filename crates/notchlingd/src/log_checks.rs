use crate::inbox_ops::InboxOps;
use crate::log_activity::Publish;
use crate::log_audit::{AuditSink, SealState};
use notchling_engine::inbox::{Action, ActionId, Item, Kind, Source, State as ItemState};
use notchling_engine::logs::{AuditDir, KeyStore};
use notchling_events::audit_check::{self, CheckInput, CheckOutcome};
use notchling_events::audit_line::{read_head, seal_of, signed_part};
use notchling_events::kinds::security_log::{AuditChecked, CheckResult as Logged};
use notchling_events::spec::Outcome;
use notchling_events::{Count, Emit, Event, Seal};
use notchling_local_api::topics;
use serde_json::{json, Value};
use std::sync::mpsc::{channel, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

pub const HOURLY: Duration = Duration::from_secs(3600);
const SEALING_WAIT: Duration = Duration::from_secs(5);

pub trait Warnings: Send + Sync {
    fn raise(&self, id: &str, title: &str, detail: &str);
    fn resolve(&self, id: &str);
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Report {
    pub result: &'static str,
    pub sealing: &'static str,
    pub why: Option<&'static str>,
    pub from_n: u64,
    pub to_n: u64,
    pub first_bad_n: Option<u64>,
    pub checkpoint_n: Option<u64>,
    pub chain_started: Option<u64>,
    pub at: i64,
}

impl Report {
    pub fn json(&self) -> Value {
        json!({
            "result": self.result,
            "sealing": self.sealing,
            "why": self.why,
            "from_n": self.from_n,
            "to_n": self.to_n,
            "first_bad_n": self.first_bad_n,
            "checkpoint_n": self.checkpoint_n,
            "chain_started": self.chain_started,
            "at": self.at,
        })
    }
}

#[derive(Default)]
struct State {
    last_to_n: Option<u64>,
    last_shown: Option<(&'static str, &'static str)>,
    warning: Option<(String, &'static str)>,
    write_warning: Option<String>,
    counter: u64,
}

pub struct Checker {
    dir: AuditDir,
    keys: Arc<dyn KeyStore>,
    audit: Arc<Mutex<AuditSink>>,
    emit: Arc<dyn Emit>,
    publish: Publish,
    warnings: Arc<dyn Warnings>,
    clock: Arc<dyn Fn() -> i64 + Send + Sync>,
    state: Mutex<State>,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

fn name_of(result: audit_check::CheckResult) -> &'static str {
    result.name()
}

fn logged(result: audit_check::CheckResult) -> Logged {
    use audit_check::CheckResult as R;
    match result {
        R::Intact => Logged::Intact,
        R::Broken => Logged::Broken,
        R::Gap => Logged::Gap,
        R::TailMissing => Logged::TailMissing,
        R::FileMissing => Logged::FileMissing,
        R::Unanchored => Logged::Unanchored,
        R::Unsealed => Logged::Unsealed,
        R::Resumed => Logged::Resumed,
    }
}

fn next_month(month: u32) -> u32 {
    if month % 100 >= 12 {
        (month / 100 + 1) * 100 + 1
    } else {
        month + 1
    }
}

fn last_n_of(text: &str) -> Option<u64> {
    let line = text.lines().rev().find(|l| !l.trim().is_empty())?;
    read_head(line).map(|h| h.n)
}

fn since(months: Vec<(u32, Option<String>)>, from: u64) -> Vec<(u32, Option<String>)> {
    let mut out = Vec::new();
    for (month, text) in months {
        let Some(text) = text else { continue };
        if last_n_of(&text).is_none_or(|last| last < from) {
            continue;
        }
        let needle = format!("{{\"n\":{from},");
        let start = if text.starts_with(&needle) {
            Some(0)
        } else {
            text.find(&format!("\n{needle}")).map(|at| at + 1)
        };
        match start {
            Some(at) => out.push((month, Some(text[at..].to_owned()))),
            None => out.push((month, Some(text))),
        }
    }
    out
}

fn warning_text(
    result: audit_check::CheckResult,
    first_bad: Option<u64>,
) -> (&'static str, String) {
    use audit_check::CheckResult as R;
    match result {
        R::Unsealed => (
            "Audit Log Not Sealed",
            "New entries can't be sealed because Notchling can't read its audit key. They are still written. Open Settings, Logs to fix it or start a new chain."
                .to_owned(),
        ),
        other => (
            "Audit Log Changed Outside Notchling",
            match first_bad {
                Some(n) => format!(
                    "The audit log was changed outside Notchling ({}). Entries from {n} on can't be trusted.",
                    other.name()
                ),
                None => format!(
                    "The audit log was changed outside Notchling ({}). Some entries can't be trusted.",
                    other.name()
                ),
            },
        ),
    }
}

impl Checker {
    pub fn new(
        dir: AuditDir,
        keys: Arc<dyn KeyStore>,
        audit: Arc<Mutex<AuditSink>>,
        emit: Arc<dyn Emit>,
        publish: Publish,
        warnings: Arc<dyn Warnings>,
        clock: Arc<dyn Fn() -> i64 + Send + Sync>,
    ) -> Arc<Checker> {
        Arc::new(Checker {
            dir,
            keys,
            audit,
            emit,
            publish,
            warnings,
            clock,
            state: Mutex::new(State::default()),
        })
    }

    fn months(&self) -> Vec<(u32, Option<String>)> {
        let present = self.dir.months().unwrap_or_default();
        let (Some(first), Some(last)) = (present.first().copied(), present.last().copied()) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        let mut month = first;
        while month <= last {
            if present.contains(&month) {
                out.push((month, self.dir.read(month).ok()));
            } else {
                out.push((month, None));
            }
            month = next_month(month);
        }
        out
    }

    pub fn run(&self, full: bool) -> Report {
        let (key, sealing) = {
            let audit = lock(&self.audit);
            (audit.key, audit.sealing())
        };
        let all = self.months();
        let from_n = if full {
            None
        } else {
            lock(&self.state).last_to_n
        };
        let months = match from_n {
            Some(from) => since(all.clone(), from),
            None => all.clone(),
        };
        let checkpoint = self.keys.read_head();
        let anchor = checkpoint
            .as_ref()
            .and_then(|c| c.prune_anchor.as_deref())
            .and_then(Seal::parse);
        let checkpoint_seal = checkpoint.as_ref().and_then(|c| Seal::parse(&c.seal));
        let first_prev = months
            .iter()
            .find_map(|(_, text)| text.as_deref())
            .and_then(|text| text.lines().next())
            .and_then(read_head)
            .and_then(|head| head.prev);
        let refs: Vec<(u32, Option<&str>)> =
            months.iter().map(|(m, t)| (*m, t.as_deref())).collect();
        let outcome = audit_check::check(&CheckInput {
            key: key.as_ref(),
            months: &refs,
            anchor: if from_n.is_some() {
                first_prev.as_ref()
            } else {
                anchor.as_ref()
            },
            checkpoint: checkpoint
                .as_ref()
                .zip(checkpoint_seal.as_ref())
                .map(|(c, seal)| (c.n, seal)),
            from_n,
        });
        let chain_started = all
            .iter()
            .filter_map(|(_, text)| text.as_deref())
            .flat_map(str::lines)
            .filter(|line| line.contains("\"kind\":\"audit.chain_started\""))
            .filter_map(|line| read_head(line).map(|h| h.n))
            .next_back();
        let report = Report {
            result: name_of(outcome.result),
            sealing: match sealing {
                SealState::Sealed => "sealed",
                SealState::Starting => "starting",
                SealState::Unsealed(_) => "unsealed",
            },
            why: match sealing {
                SealState::Unsealed(why) => Some(why.name()),
                _ => None,
            },
            from_n: outcome.from_n,
            to_n: outcome.to_n,
            first_bad_n: outcome.first_bad_n,
            checkpoint_n: checkpoint.as_ref().map(|c| c.n),
            chain_started,
            at: (self.clock)(),
        };
        self.after(&outcome, &report, full);
        report
    }

    fn after(&self, outcome: &CheckOutcome, report: &Report, full: bool) {
        let warning = outcome.result.is_warning();
        let mut state = lock(&self.state);
        if !warning {
            state.last_to_n = Some(outcome.to_n);
        }
        let changed = state.last_shown != Some((report.result, report.sealing));
        state.last_shown = Some((report.result, report.sealing));
        if warning {
            let same = state
                .warning
                .as_ref()
                .is_some_and(|(_, open)| *open == report.result);
            if !same {
                if let Some((old, _)) = state.warning.take() {
                    self.warnings.resolve(&old);
                }
                state.counter += 1;
                let id = format!("log-warning:{}", state.counter);
                let (title, detail) = warning_text(outcome.result, outcome.first_bad_n);
                self.warnings.raise(&id, title, &detail);
                state.warning = Some((id, report.result));
            }
        } else if report.sealing == "sealed" {
            if let Some((old, _)) = state.warning.take() {
                self.warnings.resolve(&old);
            }
        }
        drop(state);
        if changed || full || warning {
            self.emit.push(Event::new(
                &AuditChecked {
                    result: logged(outcome.result),
                    from_n: Some(Count(outcome.from_n)),
                    to_n: Some(Count(outcome.to_n)),
                    first_bad_n: outcome.first_bad_n.map(Count),
                },
                if warning {
                    Outcome::Failed
                } else {
                    Outcome::Ok
                },
                report.at,
            ));
        }
        if changed {
            (self.publish)(topics::LOG, topics::LOG_CHECK, report.json());
        }
    }

    pub fn write_health(&self, ok: bool) {
        let mut state = lock(&self.state);
        if ok {
            if let Some(id) = state.write_warning.take() {
                self.warnings.resolve(&id);
            }
        } else if state.write_warning.is_none() {
            state.counter += 1;
            let id = format!("log-warning:{}", state.counter);
            self.warnings.raise(
                &id,
                "Audit Log Can't Be Written",
                "Notchling can't write the audit log, so requests that must be recorded are refused until it can.",
            );
            state.write_warning = Some(id);
        }
    }

    pub fn line(&self, n: u64) -> Option<Value> {
        let key = lock(&self.audit).key;
        let prefix = format!("{{\"n\":{n},");
        for (_, text) in self.months() {
            let Some(text) = text else { continue };
            let Some(line) = text.lines().find(|l| l.starts_with(&prefix)) else {
                continue;
            };
            let head = read_head(line)?;
            let sealed = head.seal.is_some();
            let seal_ok = match (&head.seal, key.as_ref(), signed_part(line)) {
                (Some(seal), Some(key), Some(signed)) => {
                    seal_of(key, signed.as_bytes()).as_str() == seal.as_str()
                }
                _ => false,
            };
            let f = serde_json::from_str::<Value>(line)
                .ok()
                .and_then(|v| v.get("f").cloned())
                .unwrap_or(Value::Null);
            return Some(json!({"n": n, "sealed": sealed, "seal_ok": seal_ok, "f": f}));
        }
        None
    }

    pub fn spawn_schedule(self: &Arc<Checker>) -> Sender<()> {
        let (stop, rx) = channel::<()>();
        let checker = self.clone();
        let _ = std::thread::Builder::new()
            .name("notchling-log-check".into())
            .spawn(move || {
                let begun = Instant::now();
                while lock(&checker.audit).sealing() == SealState::Starting
                    && begun.elapsed() < SEALING_WAIT
                {
                    if !matches!(
                        rx.recv_timeout(Duration::from_millis(50)),
                        Err(RecvTimeoutError::Timeout)
                    ) {
                        return;
                    }
                }
                checker.run(true);
                loop {
                    match rx.recv_timeout(HOURLY) {
                        Err(RecvTimeoutError::Timeout) => {
                            checker.run(false);
                        }
                        _ => return,
                    }
                }
            });
        stop
    }
}

pub struct InboxWarnings {
    inbox: Arc<InboxOps>,
    clock: Arc<dyn Fn() -> i64 + Send + Sync>,
}

impl InboxWarnings {
    pub fn new(inbox: Arc<InboxOps>, clock: Arc<dyn Fn() -> i64 + Send + Sync>) -> InboxWarnings {
        InboxWarnings { inbox, clock }
    }
}

impl Warnings for InboxWarnings {
    fn raise(&self, id: &str, title: &str, detail: &str) {
        self.inbox.raise(
            Item {
                id: id.to_owned(),
                kind: Kind::Notice,
                title: title.to_owned(),
                detail: detail.to_owned(),
                source: Source::default(),
                created_ms: (self.clock)(),
                expires_ms: None,
                actions: vec![Action {
                    id: ActionId::Open,
                    label: "Open Logs".to_owned(),
                    primary: true,
                }],
                state: ItemState::Open,
                answer: None,
                answered_by: None,
                seen: false,
                open: Some(json!({"settings": "logs"})),
                closed_ms: None,
            },
            None,
        );
    }

    fn resolve(&self, id: &str) {
        self.inbox.resolve(id, ItemState::Withdrawn, None, None);
    }
}
