use crate::limits_ops_tests::TempDir;
use crate::log_audit::{AuditSink, SealState, StartInfo};
use crate::log_bus::{EnvMaker, Sink};
use crate::log_queue::Stamped;
use notchling_engine::logs::{month_of, AuditDir, KeyStore, LegacyFile, MemoryKeyStore};
use notchling_events::audit_check::{check, CheckInput, CheckResult};
use notchling_events::audit_line::{sealed_line, UnsealedWhy};
use notchling_events::kinds::choices::CallerTier;
use notchling_events::kinds::diagnostics::{CallResult, OpCalled};
use notchling_events::kinds::secrets::{Approval, SecretResult, VaultSecretUsed};
use notchling_events::kinds::security::{
    ApiRefused, DaemonStopped, DataRoot, RefusedWhy, StopCause,
};
use notchling_events::kinds::security_log::ChainReason;
use notchling_events::spec::Outcome;
use notchling_events::{
    Actor, ActorTier, Event, Exe, Hex, List, Millis, Op, Ref, RefMode, Salt, Secret, Version, Via,
};
use serde_json::Value;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::Arc;
use std::time::Duration;

const T0: i64 = 1_800_000_000_000;
const KEY: [u8; 32] = [9; 32];

struct Rig {
    tmp: TempDir,
    keys: Arc<MemoryKeyStore>,
    clock: Arc<AtomicI64>,
    env: EnvMaker,
}

fn rig(tag: &str) -> Rig {
    Rig {
        tmp: TempDir::new(tag),
        keys: Arc::new(MemoryKeyStore::new()),
        clock: Arc::new(AtomicI64::new(T0)),
        env: EnvMaker::new(
            Hex::new("abcdef0123456789").unwrap(),
            Hex::new("b00710ad").unwrap(),
            7,
            1,
        ),
    }
}

impl Rig {
    fn dir(&self) -> AuditDir {
        let dir = AuditDir::new(self.tmp.path("audit"));
        dir.ensure().unwrap();
        dir
    }

    fn info(&self, key_wait: Duration) -> StartInfo {
        let clock = self.clock.clone();
        StartInfo {
            daemon: Version::new("0.1.0"),
            macos: Version::new("15.1"),
            data_root: DataRoot::Fixture,
            key_wait,
            env: self.env.clone(),
            clock: Arc::new(move || clock.load(Ordering::SeqCst)),
        }
    }

    fn sink(&self) -> AuditSink {
        self.sink_with(Duration::from_secs(2), Vec::new())
    }

    fn sink_with(&self, key_wait: Duration, legacy: Vec<LegacyFile>) -> AuditSink {
        AuditSink::new(
            self.dir(),
            self.keys.clone(),
            self.info(key_wait),
            legacy,
            Salt([3; 32]),
        )
    }

    fn stamp(&self, event: Event) -> Stamped {
        Stamped {
            env: self.env.make(None, None),
            event,
            commit: None,
        }
    }

    fn lines(&self) -> Vec<Value> {
        let dir = self.dir();
        let mut out = Vec::new();
        for month in dir.months().unwrap() {
            for line in dir.read(month).unwrap().lines() {
                out.push(serde_json::from_str(line).unwrap());
            }
        }
        out
    }

    fn kinds(&self) -> Vec<String> {
        self.lines()
            .iter()
            .map(|l| l["kind"].as_str().unwrap().to_owned())
            .collect()
    }

    fn check(&self, key: Option<&[u8; 32]>) -> CheckResult {
        let dir = self.dir();
        let texts: Vec<(u32, String)> = dir
            .months()
            .unwrap()
            .into_iter()
            .map(|m| (m, dir.read(m).unwrap()))
            .collect();
        let months: Vec<(u32, Option<&str>)> =
            texts.iter().map(|(m, t)| (*m, Some(t.as_str()))).collect();
        check(&CheckInput {
            key,
            months: &months,
            anchor: None,
            checkpoint: None,
            from_n: None,
        })
        .result
    }
}

fn called(ts: i64) -> Event {
    Event::new(
        &OpCalled {
            op: Op::named("session.reply"),
            tier: CallerTier::App,
            result: CallResult::Ok,
            code: None,
            ms: Millis(2),
        },
        Outcome::Ok,
        ts,
    )
}

fn refused(ts: i64) -> Event {
    Event::new(
        &ApiRefused {
            op: Op::named("session.reply"),
            exe: Some(Exe::named("codex")),
            tier: CallerTier::Agent,
            why: RefusedWhy::Forbidden,
            switch: None,
        },
        Outcome::Refused,
        ts,
    )
}

fn stopped(ts: i64) -> Event {
    Event::new(
        &DaemonStopped {
            cause: StopCause::Quit,
            uptime_ms: Millis(5),
        },
        Outcome::Info,
        ts,
    )
}

fn legacy_file() -> LegacyFile {
    LegacyFile {
        name: "audit.jsonl".into(),
        sha256: Hex::new("00ff00ff").unwrap(),
        lines: 12,
    }
}

#[test]
fn first_run_makes_key_and_writes_chain_started() {
    let rig = rig("audit-first");
    let mut sink = rig.sink();
    let batch = vec![rig.stamp(called(T0))];
    let result = sink.write(&batch, &[None]);
    assert!(!result.failed);
    assert_eq!(sink.sealing(), SealState::Sealed);
    assert_eq!(
        rig.kinds(),
        vec!["audit.chain_started", "daemon.started", "op.called"]
    );
    let lines = rig.lines();
    assert!(lines[0]["prev"].is_null());
    assert_eq!(lines[0]["f"]["reason"], "first_run");
    assert_eq!(lines[1]["f"]["sealing"], "sealed");
    assert_eq!(lines[1]["f"]["after"], "unknown");
    assert!(lines[2]["prev"].is_string());
    assert_eq!(result.audit_n, vec![Some(3)]);
    assert_eq!(result.extra.len(), 2);
    let key = rig.keys.key().expect("a key was made");
    assert_eq!(rig.check(Some(&key)), CheckResult::Intact);
}

#[test]
fn legacy_move_counts_as_first_run() {
    let rig = rig("audit-legacy");
    with_history(&rig);
    let mut sink = rig.sink_with(Duration::from_secs(2), vec![legacy_file()]);
    sink.write(&[rig.stamp(called(T0 + 1))], &[None]);
    assert_eq!(sink.sealing(), SealState::Sealed);
    let lines = rig.lines();
    let chain = lines
        .iter()
        .find(|l| l["kind"] == "audit.chain_started")
        .expect("a chain start");
    assert_eq!(chain["f"]["legacy_files"], 1);
    assert_eq!(chain["f"]["legacy_lines"], 12);
    assert_eq!(chain["f"]["legacy_sha"][0], "00ff00ff");
    assert!(chain["prev"].is_null());
}

fn with_history(rig: &Rig) {
    let key = KEY;
    let env = rig.env.make(None, None);
    let (line, _) = sealed_line(
        &key,
        1,
        None,
        &env,
        &called(T0),
        &RefMode::Plain(Salt([3; 32])),
    );
    let dir = rig.dir();
    std::fs::write(dir.path_of(month_of(T0)), format!("{line}\n")).unwrap();
}

#[test]
fn missing_key_with_history_is_unsealed_missing() {
    let rig = rig("audit-missing");
    with_history(&rig);
    let mut sink = rig.sink();
    sink.write(&[rig.stamp(called(T0 + 1))], &[None]);
    assert_eq!(sink.sealing(), SealState::Unsealed(UnsealedWhy::KeyMissing));
    assert_eq!(
        rig.kinds(),
        vec!["op.called", "daemon.started", "audit.checked", "op.called"]
    );
    let lines = rig.lines();
    assert_eq!(lines[1]["unsealed"], "key_missing");
    assert!(lines[1]["seal"].is_null());
    assert_eq!(lines[2]["f"]["result"], "unsealed");
    assert_eq!(lines[1]["f"]["sealing"], "unsealed");
    assert_eq!(
        rig.kinds().iter().filter(|k| *k == "audit.checked").count(),
        1
    );
}

#[test]
fn denied_key_is_unsealed_denied() {
    let rig = rig("audit-denied");
    with_history(&rig);
    rig.keys.deny_reads(true);
    let mut sink = rig.sink();
    sink.write(&[rig.stamp(called(T0 + 1))], &[None]);
    assert_eq!(sink.sealing(), SealState::Unsealed(UnsealedWhy::KeyDenied));
    assert_eq!(rig.lines().last().unwrap()["unsealed"], "key_denied");
}

#[test]
fn waiting_then_allowed_resumes_and_names_the_stretch() {
    let rig = rig("audit-waiting");
    rig.keys.set_key(Some(KEY));
    rig.keys.block_reads(Duration::from_millis(500));
    let mut sink = rig.sink_with(Duration::from_millis(40), Vec::new());
    sink.write(
        &[rig.stamp(called(T0)), rig.stamp(called(T0 + 1))],
        &[None, None],
    );
    assert_eq!(sink.sealing(), SealState::Unsealed(UnsealedWhy::KeyWaiting));
    let waiting: Vec<Value> = rig
        .lines()
        .into_iter()
        .filter(|l| l["unsealed"] == "key_waiting")
        .collect();
    assert_eq!(waiting.len(), 4);
    assert_eq!(rig.lines()[0]["f"]["sealing"], "waiting");
    std::thread::sleep(Duration::from_millis(700));
    let ticked = sink.tick(T0 + 5);
    assert_eq!(sink.sealing(), SealState::Sealed);
    assert_eq!(ticked.extra.len(), 1);
    sink.write(&[rig.stamp(called(T0 + 6))], &[None]);
    let lines = rig.lines();
    let resumed = lines
        .iter()
        .find(|l| l["kind"] == "audit.checked" && l["f"]["result"] == "resumed")
        .expect("resumed line");
    assert_eq!(resumed["f"]["from_n"], 1);
    assert_eq!(resumed["f"]["to_n"], 4);
    assert!(resumed["prev"].is_null());
    assert!(resumed["seal"].is_string());
    assert_eq!(rig.check(Some(&KEY)), CheckResult::Resumed);
}

#[test]
fn daemon_started_is_first_line_with_after() {
    let rig = rig("audit-after");
    let mut first = rig.sink();
    first.write(
        &[rig.stamp(called(T0)), rig.stamp(stopped(T0 + 1))],
        &[None, None],
    );
    drop(first);
    let before = rig.lines().len();
    let mut second = rig.sink();
    second.write(&[rig.stamp(called(T0 + 2))], &[None]);
    let lines = rig.lines();
    assert_eq!(lines[before]["kind"], "daemon.started");
    assert_eq!(lines[before]["f"]["after"], "clean_stop");
    drop(second);
    let before = rig.lines().len();
    let mut third = rig.sink();
    third.write(&[rig.stamp(called(T0 + 3))], &[None]);
    assert_eq!(rig.lines()[before]["f"]["after"], "unknown");
}

#[test]
fn n_and_prev_continue_across_restart() {
    let rig = rig("audit-restart");
    let mut first = rig.sink();
    first.write(&[rig.stamp(called(T0))], &[None]);
    let (n, seal) = first.head();
    drop(first);
    let mut second = rig.sink();
    let result = second.write(&[rig.stamp(called(T0 + 1))], &[None]);
    let lines = rig.lines();
    let started = &lines[n as usize];
    assert_eq!(started["n"].as_u64(), Some(n + 1));
    assert_eq!(started["prev"], seal.unwrap().as_str());
    assert_eq!(result.audit_n, vec![Some(n + 2)]);
    let key = rig.keys.key().unwrap();
    assert_eq!(rig.check(Some(&key)), CheckResult::Intact);
}

#[test]
fn month_change_carries_prev() {
    let rig = rig("audit-month");
    let mut sink = rig.sink();
    let next_month = T0 + 40 * 86_400_000;
    sink.write(
        &[rig.stamp(called(T0)), rig.stamp(called(next_month))],
        &[None, None],
    );
    let dir = rig.dir();
    let months = dir.months().unwrap();
    assert_eq!(months.len(), 2);
    let last_of_first = dir
        .read(months[0])
        .unwrap()
        .lines()
        .last()
        .unwrap()
        .to_owned();
    let first_of_second = dir
        .read(months[1])
        .unwrap()
        .lines()
        .next()
        .unwrap()
        .to_owned();
    let previous: Value = serde_json::from_str(&last_of_first).unwrap();
    let next: Value = serde_json::from_str(&first_of_second).unwrap();
    assert_eq!(next["prev"], previous["seal"]);
    let key = rig.keys.key().unwrap();
    assert_eq!(rig.check(Some(&key)), CheckResult::Intact);
}

#[test]
fn checkpoint_written_after_always_batch_and_every_five_minutes() {
    let rig = rig("audit-checkpoint");
    let mut sink = rig.sink();
    sink.write(&[rig.stamp(refused(T0))], &[None]);
    sink.flush(true).unwrap();
    let first = rig
        .keys
        .read_head()
        .expect("checkpoint after an always batch");
    assert_eq!(first.n, 3);
    rig.clock.store(T0 + 1_000, Ordering::SeqCst);
    sink.write(&[rig.stamp(refused(T0 + 1_000))], &[None]);
    sink.flush(true).unwrap();
    assert_eq!(rig.keys.read_head().unwrap().n, 3, "at most every 10 s");
    rig.clock.store(T0 + 11_000, Ordering::SeqCst);
    sink.write(&[rig.stamp(refused(T0 + 11_000))], &[None]);
    sink.flush(true).unwrap();
    assert_eq!(rig.keys.read_head().unwrap().n, 5);
    rig.clock.store(T0 + 12_000, Ordering::SeqCst);
    sink.write(&[rig.stamp(called(T0 + 12_000))], &[None]);
    sink.flush(false).unwrap();
    assert_eq!(
        rig.keys.read_head().unwrap().n,
        5,
        "a toggle batch writes none"
    );
    sink.tick(T0 + 12_000 + 60_000);
    assert_eq!(rig.keys.read_head().unwrap().n, 5);
    sink.tick(T0 + 12_000 + 301_000);
    assert_eq!(rig.keys.read_head().unwrap().n, 6);
}

#[test]
fn refusals_fold_into_repeat() {
    let rig = rig("audit-fold");
    let mut sink = rig.sink();
    let batch: Vec<Stamped> = (0..1_000).map(|_| rig.stamp(refused(T0))).collect();
    let audit_n = vec![None; batch.len()];
    let result = sink.write(&batch, &audit_n);
    assert_eq!(result.audit_n.iter().filter(|n| n.is_some()).count(), 1);
    sink.tick(T0 + 2_000);
    let refusals: Vec<Value> = rig
        .lines()
        .into_iter()
        .filter(|l| l["kind"] == "api.refused")
        .collect();
    assert_eq!(refusals.len(), 2);
    assert!(refusals[0].get("repeat").is_none());
    assert_eq!(refusals[1]["repeat"], 999);
    let key = rig.keys.key().unwrap();
    assert_eq!(rig.check(Some(&key)), CheckResult::Intact);
}

#[test]
fn start_new_chain_writes_sealed_chain_started_with_previous_head() {
    let rig = rig("audit-chain");
    with_history(&rig);
    let mut sink = rig.sink();
    sink.write(&[rig.stamp(called(T0 + 1))], &[None]);
    assert_eq!(sink.sealing(), SealState::Unsealed(UnsealedWhy::KeyMissing));
    let n = sink.start_new_chain(ChainReason::User).unwrap();
    assert_eq!(sink.sealing(), SealState::Sealed);
    let lines = rig.lines();
    let line = &lines[n as usize - 1];
    assert_eq!(line["kind"], "audit.chain_started");
    assert_eq!(line["f"]["reason"], "user");
    assert!(line["prev"].is_null());
    assert!(line["seal"].is_string());
    assert!(line["f"]["previous_head_n"].is_u64());
    let key = rig.keys.key().unwrap();
    assert_eq!(rig.check(Some(&key)), CheckResult::Intact);
    assert_eq!(rig.keys.read_head().unwrap().n, n);
}

#[test]
fn a_line_over_four_kilobytes_is_cut_to_fit() {
    let rig = rig("audit-fit");
    let mut sink = rig.sink();
    let names: Vec<Ref<Secret>> = (0..16)
        .map(|i| Ref::new(&format!("{i}{}", "s".repeat(250))))
        .collect();
    let event = Event::new(
        &VaultSecretUsed {
            secrets: List::new(names),
            session: Some(Ref::new(&"a".repeat(256))),
            folder: Some(Ref::new(&"f".repeat(256))),
            command_exe: Some(Exe::named("codex")),
            approval: Approval::Asked,
            answered_by: None,
            device: Some(Ref::new(&"d".repeat(256))),
            result: SecretResult::Given,
            waited_ms: None,
        },
        Outcome::Ok,
        T0,
    );
    let mut stamped = rig.stamp(event);
    stamped.env.actor = Some(Actor {
        tier: ActorTier::Agent,
        via: Some(Via::Cli),
        exe: Some(Exe::named("codex")),
        device: Some(Ref::new(&"p".repeat(256))),
        session: Some(Ref::new(&"q".repeat(256))),
    });
    let result = sink.write(&[stamped], &[None]);
    assert!(!result.failed);
    let text = std::fs::read_to_string(rig.dir().path_of(month_of(T0))).unwrap();
    assert!(text.lines().count() >= 3);
    for line in text.lines() {
        assert!(line.len() <= 4096, "{}", line.len());
    }
    let key = rig.keys.key().unwrap();
    assert_eq!(rig.check(Some(&key)), CheckResult::Intact);
}
