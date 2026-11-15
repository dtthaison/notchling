use super::{prune, take, Context, SnapRunner};
use crate::snap_files_tests::{make_db, put};
use crate::snap_testkit::{id_of, Fixture};
use notchling_engine::cluster::core::SnapAsk;
use notchling_engine::cluster::id::Tool;
use notchling_engine::cluster::main_sessions::MainSession;
use notchling_engine::cluster::members::Term;
use notchling_engine::cluster::snapshot::{Movable, Trigger};
use notchling_engine::control::{FakeLauncher, LaunchError, Launcher, RealLauncher};
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

const WAIT: Duration = Duration::from_secs(20);

fn context(fx: &Fixture, launcher: Arc<dyn Launcher>) -> Context {
    Context {
        launcher,
        home: fx.t.path("home"),
        data: fx.t.path("home/data"),
    }
}

fn real() -> Arc<dyn Launcher> {
    Arc::new(RealLauncher::with_claude(PathBuf::from(
        "/nonexistent/claude",
    )))
}

fn lay_out(fx: &Fixture) -> MainSession {
    put(&fx.transcript(), b"one\ntwo\n");
    put(&fx.t.path("home/data/pages/a.html"), b"<p>a</p>");
    put(&fx.t.path("home/AgentConsole/bridge/devices.json"), b"{}");
    drop(make_db(&fx.t.path("home/data/cluster/cluster.db"), 5));
    let work = fx.t.path("work/repo");
    std::fs::create_dir_all(&work).unwrap();
    MainSession {
        id: "s1".to_string(),
        tool: Tool::Claude,
        cwd: work.display().to_string(),
        config_dir: None,
        transcript: Some(fx.transcript().display().to_string()),
        respawn: Vec::new(),
        since_ms: 1,
    }
}

fn ask(fx: &Fixture, session: &MainSession, seq: u64) -> SnapAsk {
    SnapAsk {
        seq,
        trigger: Trigger::Timer,
        term: Term {
            n: 1,
            node: fx.primary.clone(),
        },
        home: fx.primary.clone(),
        main: vec![session.clone()],
        targets: vec![id_of(1)],
        acked: BTreeMap::new(),
    }
}

fn sets_in(ctx: &Context) -> BTreeSet<String> {
    std::fs::read_dir(ctx.outgoing())
        .map(|dir| {
            dir.flatten()
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default()
}

#[test]
fn a_snapshot_is_staged_and_offered_to_every_ready_standby() {
    let fx = Fixture::new("run-offer", &[(1, true), (2, true)]);
    fx.make_ready(&[id_of(1), id_of(2)]);
    let session = lay_out(&fx);
    let ctx = context(&fx, real());
    let set = take(
        &fx.sender,
        &ctx,
        &ask(&fx, &session, 5),
        crate::snap_files_tests::now_ms(),
    )
    .unwrap();
    assert_eq!(set, Some("s-5".to_string()));
    assert!(fx.sender.wait_idle(WAIT));
    for n in [1, 2] {
        let manifest = fx.store_of(&id_of(n)).current().expect("a current set");
        assert_eq!(manifest.set, "s-5");
        assert_eq!(manifest.seq, 5);
        assert_eq!(manifest.sessions.len(), 1);
        assert!(matches!(manifest.sessions[0].movable, Movable::No(_)));
    }
}

#[test]
fn nothing_is_staged_while_no_standby_is_ready() {
    let fx = Fixture::new("run-notready", &[(1, true)]);
    let session = lay_out(&fx);
    let ctx = context(&fx, real());
    assert_eq!(
        take(&fx.sender, &ctx, &ask(&fx, &session, 1), 1).unwrap(),
        None
    );
    assert!(sets_in(&ctx).is_empty());
    assert_eq!(fx.net.calls(&id_of(1), "snap.offer"), 0);
}

#[test]
fn a_repository_that_cannot_be_read_leaves_the_session_unmovable() {
    let fx = Fixture::new("run-unreadable", &[(1, true)]);
    fx.make_ready(&[id_of(1)]);
    let session = lay_out(&fx);
    let broken = FakeLauncher::new();
    for _ in 0..8 {
        broken.push_output(Err(LaunchError::Spawn("no git".to_string())));
    }
    let ctx = context(&fx, Arc::new(broken));
    take(
        &fx.sender,
        &ctx,
        &ask(&fx, &session, 2),
        crate::snap_files_tests::now_ms(),
    )
    .unwrap();
    assert!(fx.sender.wait_idle(WAIT));
    let manifest = fx.store_of(&id_of(1)).current().expect("a set");
    let entry = &manifest.sessions[0];
    assert!(entry.repo.is_none());
    assert!(matches!(&entry.movable, Movable::No(why) if why.contains("couldn't be read")));
}

#[test]
fn a_transcript_still_being_written_stages_nothing() {
    let fx = Fixture::new("run-busy", &[(1, true)]);
    fx.make_ready(&[id_of(1)]);
    let session = lay_out(&fx);
    std::fs::write(fx.transcript(), b"one\ntwo\nthree\n").unwrap();
    let ctx = context(&fx, real());
    let now = crate::snap_files_tests::now_ms();
    let why = take(&fx.sender, &ctx, &ask(&fx, &session, 3), now).unwrap_err();
    assert!(why.contains("still being written"), "{why}");
    assert!(fx.sender.wait_idle(WAIT));
    assert!(sets_in(&ctx).is_empty());
    assert_eq!(fx.net.calls(&id_of(1), "snap.offer"), 0);
}

#[test]
fn old_sets_are_pruned_but_pinned_ones_stay() {
    let fx = Fixture::new("run-prune", &[(1, true)]);
    fx.make_ready(&[id_of(1)]);
    let session = lay_out(&fx);
    let ctx = context(&fx, real());
    for seq in [5, 6] {
        take(
            &fx.sender,
            &ctx,
            &ask(&fx, &session, seq),
            crate::snap_files_tests::now_ms(),
        )
        .unwrap();
        assert!(fx.sender.wait_idle(WAIT));
    }
    assert!(sets_in(&ctx).contains("s-6"));
    std::fs::create_dir_all(ctx.outgoing().join("s-1")).unwrap();
    std::fs::create_dir_all(ctx.outgoing().join("notes")).unwrap();
    prune(&ctx.outgoing(), &fx.sender.pinned());
    let left = sets_in(&ctx);
    assert!(left.contains("s-6"), "{left:?}");
    assert!(!left.contains("s-5") && !left.contains("s-1"), "{left:?}");
    assert!(left.contains("notes"));
}

#[test]
fn the_runner_stages_the_newest_ask_off_the_caller_thread() {
    let fx = Fixture::new("run-worker", &[(1, true)]);
    fx.make_ready(&[id_of(1)]);
    let session = lay_out(&fx);
    let ctx = context(&fx, real());
    let sender = Arc::new(take_sender(&fx));
    let runner = SnapRunner::spawn(sender.clone(), ctx, Duration::from_millis(10));
    runner.submit(ask(&fx, &session, 7));
    assert!(runner.wait_idle(WAIT));
    assert_eq!(runner.last_set(), Some("s-7".to_string()));
    assert_eq!(runner.last_error(), None);
    assert!(sender.wait_idle(WAIT));
    runner.close();
    assert_eq!(
        fx.store_of(&id_of(1)).current().map(|m| m.set),
        Some("s-7".to_string())
    );
}

fn take_sender(fx: &Fixture) -> crate::snap_send::SnapSender {
    fx.rebuild_sender()
}

#[test]
fn a_failed_stage_is_reported() {
    let fx = Fixture::new("run-retry", &[(1, true)]);
    fx.make_ready(&[id_of(1)]);
    let session = lay_out(&fx);
    std::fs::write(fx.transcript(), b"one\ntwo\nthree\n").unwrap();
    let ctx = context(&fx, real());
    let runner = SnapRunner::spawn(Arc::new(take_sender(&fx)), ctx, Duration::from_millis(10));
    runner.submit(ask(&fx, &session, 8));
    assert!(runner.wait_idle(WAIT));
    assert!(runner
        .last_error()
        .is_some_and(|e| e.contains("still being written")));
    assert_eq!(runner.last_set(), None);
    runner.close();
}
