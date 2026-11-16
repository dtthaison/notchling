use std::sync::Arc;

use super::claims::Claim;
use super::failover::{arming_id, Failover, Grantor, REACH_MS};
use super::failover_tests::{cluster, HALF_HOUR, HOUR};
use super::id::Tool;
use super::members::{MemberState, Term, MAX_TERM_N};
use super::outward::CutStep;
use super::promote::{
    next, Facts, MovingSession, Promote, PromoteAction, PromoteEvent, PromoteStep, RestoredSession,
    SetFacts,
};
use super::snapshot::repo_state::Movable;
use super::state::{apply, ClusterState, Command};
use super::testkit::{id, versions};

const T0: u64 = 1_000_000;

fn session(name: &str, tool: Tool, movable: Movable) -> MovingSession {
    MovingSession {
        id: name.to_string(),
        tool,
        movable,
        flags: vec!["--model".to_string(), "opus".to_string()],
        previous: None,
        base_len: 4_000,
    }
}

fn set_with(sessions: Vec<MovingSession>) -> SetFacts {
    SetFacts {
        id: "set-7".to_string(),
        versions: versions("macos"),
        sessions,
    }
}

fn facts(state: ClusterState, set: Option<SetFacts>) -> Facts {
    Facts {
        state,
        grantor: Grantor::new(0),
        mine: versions("macos"),
        ready: Ok(()),
        peers: vec![id("m2")],
        set,
    }
}

fn plain() -> Facts {
    facts(
        cluster(),
        Some(set_with(vec![
            session("s1", Tool::Claude, Movable::Yes),
            session("s2", Tool::Claude, Movable::No("a change".to_string())),
            session("s3", Tool::Codex, Movable::Yes),
        ])),
    )
}

fn armed(failover: Failover) -> Facts {
    let mut state = cluster();
    apply(&mut state, Command::SetFailover { failover }, 300).expect("set failover");
    let term = state.term.clone();
    let armed_id = arming_id(&state);
    apply(
        &mut state,
        Command::Armed {
            node: id("m1"),
            term,
            id: armed_id,
        },
        310,
    )
    .expect("armed");
    let mut f = facts(
        state,
        Some(set_with(vec![session("s1", Tool::Claude, Movable::Yes)])),
    );
    f.grantor.grant(0, failover);
    f
}

struct Run {
    p: Promote,
    all: Vec<PromoteAction>,
    now: u64,
}

impl Run {
    fn new(auto: bool, confirmed: bool) -> Run {
        Run {
            p: Promote::new(id("m1"), id("m5"), auto, confirmed, T0),
            all: Vec::new(),
            now: T0,
        }
    }

    fn feed(&mut self, ev: PromoteEvent) -> Vec<PromoteAction> {
        let (p, actions) = next(&self.p, ev, self.now);
        self.p = p;
        self.all.extend(actions.clone());
        actions
    }

    fn begin(&mut self, f: Facts) -> Vec<PromoteAction> {
        self.feed(PromoteEvent::Begin(Arc::new(f)))
    }

    fn quiet_reach(&mut self) -> Vec<PromoteAction> {
        self.now += REACH_MS;
        self.feed(PromoteEvent::Tick)
    }
}

fn restored(session: &str, cut: Vec<CutStep>) -> RestoredSession {
    RestoredSession {
        session: session.to_string(),
        cut,
    }
}

fn through_bump(run: &mut Run, f: Facts) {
    run.begin(f);
    run.quiet_reach();
    run.feed(PromoteEvent::Bumped(Ok(())));
}

fn failed(run: &Run) -> Option<&str> {
    match &run.p.step {
        PromoteStep::Failed(reason) => Some(reason),
        _ => None,
    }
}

fn position(all: &[PromoteAction], f: impl Fn(&PromoteAction) -> bool) -> Option<usize> {
    all.iter().position(f)
}

fn is_resume(a: &PromoteAction) -> bool {
    matches!(a, PromoteAction::Resume { .. })
}

fn is_term(a: &PromoteAction) -> bool {
    matches!(a, PromoteAction::Journal(c) if matches!(**c, Command::Term { .. }))
}

fn claim_of(a: &PromoteAction) -> Option<&Claim> {
    match a {
        PromoteAction::Journal(c) => match &**c {
            Command::Claim { claim } => Some(claim),
            _ => None,
        },
        _ => None,
    }
}

fn push_step() -> CutStep {
    CutStep {
        tool: "Bash".to_string(),
        summary: "git push origin main".to_string(),
        outward: true,
    }
}

fn texts(all: &[PromoteAction]) -> Vec<String> {
    all.iter()
        .filter_map(|a| match a {
            PromoteAction::Announce(n) => Some(n.text.clone()),
            _ => None,
        })
        .collect()
}

#[test]
fn check_refuses_a_machine_that_isnt_ready() {
    let mut run = Run::new(false, false);
    let mut f = plain();
    f.ready = Err("m1 has no disk encryption".to_string());
    let actions = run.begin(f);
    assert_eq!(failed(&run), Some("m1 has no disk encryption"));
    assert_eq!(
        actions,
        vec![PromoteAction::Fail {
            reason: "m1 has no disk encryption".to_string()
        }]
    );

    let mut run = Run::new(false, false);
    let mut f = plain();
    f.state
        .members
        .get_mut(&id("m1"))
        .expect("m1")
        .can_be_master = false;
    run.begin(f);
    assert_eq!(failed(&run), Some("m1 has Can Be Master off"));

    let mut run = Run::new(false, false);
    let mut f = plain();
    f.mine.notchling = "0.2.0".to_string();
    run.begin(f);
    let reason = failed(&run).expect("failed");
    assert!(reason.contains("Notchling 0.2.0"), "{reason}");

    let p = Promote::new(id("m5"), id("m5"), false, false, T0);
    let (q, _) = next(&p, PromoteEvent::Begin(Arc::new(plain())), T0);
    assert_eq!(
        q.step,
        PromoteStep::Failed("m5 is primary already".to_string())
    );

    let mut run = Run::new(false, false);
    run.begin(plain());
    assert_eq!(run.p.step, PromoteStep::Reach);
}

#[test]
fn machine_without_a_set_controls_jobs_only() {
    let mut run = Run::new(false, true);
    run.begin(facts(cluster(), None));
    assert_eq!(run.p.step, PromoteStep::Reach);
    run.quiet_reach();
    let after = run.feed(PromoteEvent::Bumped(Ok(())));
    assert_eq!(run.p.step, PromoteStep::Held);
    assert!(after.contains(&PromoteAction::RunHeld));
    assert!(
        !run.all
            .iter()
            .any(|a| matches!(a, PromoteAction::Restore { .. }) || is_resume(a)),
        "{:?}",
        run.all
    );
    run.feed(PromoteEvent::HeldRun);
    run.feed(PromoteEvent::Announced);
    assert_eq!(run.p.step, PromoteStep::Done);
    assert_eq!(texts(&run.all), vec!["m1 is primary now".to_string()]);
}

#[test]
fn reachable_primary_relays_instead() {
    let mut run = Run::new(false, false);
    let first = run.begin(plain());
    assert_eq!(
        first,
        vec![PromoteAction::Probe {
            primary: id("m5"),
            via: vec![id("m2")],
            within_ms: REACH_MS,
        }]
    );
    run.now += 3_000;
    let actions = run.feed(PromoteEvent::Answer);
    assert_eq!(run.p.step, PromoteStep::Relayed);
    assert_eq!(
        actions,
        vec![PromoteAction::Relayed {
            text: "m5 is reachable through m1; nothing moved".to_string()
        }]
    );
    assert!(!run.all.iter().any(is_term));
    assert!(run.feed(PromoteEvent::Tick).is_empty());
}

#[test]
fn reached_through_a_peer_relays_too() {
    let mut run = Run::new(true, false);
    run.begin(plain());
    run.now += REACH_MS;
    let actions = run.feed(PromoteEvent::Answer);
    assert_eq!(run.p.step, PromoteStep::Relayed);
    assert!(
        matches!(&actions[..], [PromoteAction::Relayed { text }] if text.contains("nothing moved"))
    );
    assert!(!run.all.iter().any(is_term));
}

#[test]
fn a_late_answer_is_no_answer() {
    let mut run = Run::new(false, true);
    run.begin(plain());
    run.now += REACH_MS + 1;
    run.feed(PromoteEvent::Answer);
    assert_eq!(run.p.step, PromoteStep::Bump);
    assert!(run.all.iter().any(is_term));
}

#[test]
fn reach_waits_for_its_window() {
    let mut run = Run::new(false, true);
    run.begin(plain());
    run.now += REACH_MS - 1;
    assert!(run.feed(PromoteEvent::Tick).is_empty());
    assert_eq!(run.p.step, PromoteStep::Reach);
    let actions = run.feed(PromoteEvent::NoAnswer);
    assert_eq!(run.p.step, PromoteStep::Bump);
    assert_eq!(actions.len(), 2);
}

#[test]
fn unsafe_promote_needs_confirm() {
    let mut run = Run::new(false, false);
    run.begin(plain());
    let actions = run.quiet_reach();
    let text = match &actions[..] {
        [PromoteAction::NeedsConfirm { text }] => text.clone(),
        other => panic!("{other:?}"),
    };
    assert!(text.starts_with("m5 can't be reached."), "{text}");
    assert!(text.contains("kept as a copy on m5"), "{text}");
    assert_eq!(failed(&run), Some(text.as_str()));
    assert!(!run.all.iter().any(is_term));
    assert!(run.feed(PromoteEvent::Bumped(Ok(()))).is_empty());
}

#[test]
fn confirmed_promote_goes_on() {
    let mut run = Run::new(false, true);
    run.begin(plain());
    let actions = run.quiet_reach();
    assert_eq!(run.p.step, PromoteStep::Bump);
    assert_eq!(
        actions,
        vec![
            PromoteAction::Revoke,
            PromoteAction::Journal(Box::new(Command::Term {
                term: Term {
                    n: 2,
                    node: id("m1")
                }
            })),
        ]
    );
}

#[test]
fn auto_promote_after_deadline_needs_no_confirm() {
    let mut run = Run::new(true, false);
    run.now = HALF_HOUR;
    run.begin(armed(Failover::After30m));
    run.now += REACH_MS;
    run.feed(PromoteEvent::Tick);
    assert_eq!(run.p.step, PromoteStep::Bump);
    run.feed(PromoteEvent::Bumped(Ok(())));
    run.feed(PromoteEvent::Restored(Ok(vec![restored("s1", vec![])])));
    run.feed(PromoteEvent::Resumed { failed: vec![] });
    run.feed(PromoteEvent::HeldRun);
    assert_eq!(
        texts(&run.all),
        vec!["m1 is primary now, after m5 was unreachable for 30 minutes".to_string()]
    );

    let mut early = Run::new(true, false);
    early.now = HALF_HOUR - 60_000;
    early.begin(armed(Failover::After30m));
    early.now += REACH_MS;
    let actions = early.feed(PromoteEvent::Tick);
    assert!(matches!(&actions[..], [PromoteAction::NeedsConfirm { .. }]));
}

#[test]
fn an_hour_is_named_as_an_hour() {
    let mut run = Run::new(true, false);
    run.now = HOUR;
    run.begin(armed(Failover::After1h));
    run.quiet_reach();
    run.feed(PromoteEvent::Bumped(Ok(())));
    run.feed(PromoteEvent::Restored(Ok(vec![restored("s1", vec![])])));
    run.feed(PromoteEvent::Resumed { failed: vec![] });
    run.feed(PromoteEvent::HeldRun);
    assert_eq!(
        texts(&run.all),
        vec!["m1 is primary now, after m5 was unreachable for 1 hour".to_string()]
    );
}

#[test]
fn bump_before_resume() {
    let mut run = Run::new(false, true);
    run.begin(plain());
    run.quiet_reach();
    run.feed(PromoteEvent::Bumped(Ok(())));
    assert_eq!(run.p.step, PromoteStep::Restore);
    run.feed(PromoteEvent::Restored(Ok(vec![
        restored("s1", vec![]),
        restored("s3", vec![]),
    ])));
    let revoke = position(&run.all, |a| *a == PromoteAction::Revoke).expect("revoke");
    let term = position(&run.all, is_term).expect("term");
    let push = position(&run.all, |a| matches!(a, PromoteAction::PushTerm { .. })).expect("push");
    let restore =
        position(&run.all, |a| matches!(a, PromoteAction::Restore { .. })).expect("restore");
    let resume = position(&run.all, is_resume).expect("resume");
    assert!(revoke < term && term < push && push < restore && restore < resume);
    let want = Term {
        n: 2,
        node: id("m1"),
    };
    assert_eq!(
        run.all[term],
        PromoteAction::Journal(Box::new(Command::Term { term: want.clone() }))
    );
    assert_eq!(run.all[push], PromoteAction::PushTerm { term: want });
}

#[test]
fn a_failed_journal_stops_before_anything_starts() {
    let mut run = Run::new(false, true);
    run.begin(plain());
    run.quiet_reach();
    let actions = run.feed(PromoteEvent::Bumped(Err("journal is full".to_string())));
    assert_eq!(failed(&run), Some("journal is full"));
    assert_eq!(actions.len(), 1);
    assert!(!run.all.iter().any(|a| matches!(
        a,
        PromoteAction::PushTerm { .. } | PromoteAction::Restore { .. }
    )));
}

#[test]
fn only_movable_sessions_resume() {
    let mut run = Run::new(false, true);
    through_bump(&mut run, plain());
    let restore = run
        .all
        .iter()
        .find_map(|a| match a {
            PromoteAction::Restore { sessions } => Some(sessions.clone()),
            _ => None,
        })
        .expect("restore");
    assert_eq!(restore, vec!["s1".to_string(), "s3".to_string()]);
    run.feed(PromoteEvent::Restored(Ok(vec![
        restored("s1", vec![]),
        restored("s2", vec![]),
        restored("s3", vec![]),
    ])));
    let resumes: Vec<_> = run
        .all
        .iter()
        .filter_map(|a| match a {
            PromoteAction::Resume { session, flags } => Some((session.clone(), flags.clone())),
            _ => None,
        })
        .collect();
    let flags = vec!["--model".to_string(), "opus".to_string()];
    assert_eq!(
        resumes,
        vec![
            ("s1".to_string(), flags.clone()),
            ("s3".to_string(), flags.clone())
        ]
    );
    let claimed: Vec<_> = run
        .all
        .iter()
        .filter_map(claim_of)
        .map(|claim| claim.session.clone())
        .collect();
    assert_eq!(claimed, vec!["s1".to_string(), "s3".to_string()]);
}

#[test]
fn each_resume_follows_its_claim() {
    let mut f = plain();
    f.set.as_mut().expect("set").sessions[0].previous = Some(Claim {
        session: "s1".to_string(),
        node: id("m5"),
        n: 4,
        term: Term {
            n: 1,
            node: id("m5"),
        },
        base_set: "set-3".to_string(),
        base_len: 100,
        touched: true,
    });
    let mut run = Run::new(false, true);
    through_bump(&mut run, f);
    let actions = run.feed(PromoteEvent::Restored(Ok(vec![
        restored("s1", vec![]),
        restored("s3", vec![]),
    ])));
    let want = Claim {
        session: "s1".to_string(),
        node: id("m1"),
        n: 5,
        term: Term {
            n: 2,
            node: id("m1"),
        },
        base_set: "set-7".to_string(),
        base_len: 4_000,
        touched: false,
    };
    assert_eq!(
        actions[0],
        PromoteAction::Journal(Box::new(Command::Claim { claim: want }))
    );
    assert!(is_resume(&actions[1]));
    let third = claim_of(&actions[2]).expect("claim");
    assert!(third.session == "s3" && third.n == 1 && !third.touched);
    assert!(is_resume(&actions[3]));
    assert_eq!(actions.len(), 4);
}

#[test]
fn held_commands_run_after_restore() {
    let mut run = Run::new(false, true);
    through_bump(&mut run, plain());
    assert!(!run.all.contains(&PromoteAction::RunHeld));
    run.feed(PromoteEvent::Restored(Ok(vec![restored("s1", vec![])])));
    assert!(!run.all.contains(&PromoteAction::RunHeld));
    assert_eq!(run.p.step, PromoteStep::Resume);
    run.feed(PromoteEvent::Resumed { failed: vec![] });
    assert_eq!(run.p.step, PromoteStep::Held);
    let held = position(&run.all, |a| *a == PromoteAction::RunHeld).expect("held");
    let restore =
        position(&run.all, |a| matches!(a, PromoteAction::Restore { .. })).expect("restore");
    let resume = position(&run.all, is_resume).expect("resume");
    assert!(restore < resume && resume < held);
    assert_eq!(run.all.len(), held + 1);
}

#[test]
fn cut_outward_step_raises_item_and_sends_nothing() {
    let mut run = Run::new(false, true);
    through_bump(&mut run, plain());
    run.feed(PromoteEvent::Restored(Ok(vec![restored(
        "s1",
        vec![push_step()],
    )])));
    run.feed(PromoteEvent::Resumed { failed: vec![] });
    let actions = run.feed(PromoteEvent::HeldRun);
    assert_eq!(
        actions[0],
        PromoteAction::PausedStep {
            session: "s1".to_string(),
            step: push_step(),
        }
    );
    let resumes = run.all.iter().filter(|a| is_resume(a)).count();
    assert_eq!(resumes, 1);
    let paused =
        position(&run.all, |a| matches!(a, PromoteAction::PausedStep { .. })).expect("item");
    let resume = position(&run.all, is_resume).expect("resume");
    assert!(resume < paused);
    assert_eq!(
        run.all
            .iter()
            .filter(|a| matches!(a, PromoteAction::PausedStep { .. }))
            .count(),
        1
    );
}

#[test]
fn no_cut_steps_no_items() {
    let mut run = Run::new(false, true);
    through_bump(&mut run, plain());
    run.feed(PromoteEvent::Restored(Ok(vec![
        restored("s1", vec![]),
        restored("s3", vec![]),
    ])));
    run.feed(PromoteEvent::Resumed { failed: vec![] });
    run.feed(PromoteEvent::HeldRun);
    assert!(!run
        .all
        .iter()
        .any(|a| matches!(a, PromoteAction::PausedStep { .. })));
}

#[test]
fn a_session_that_did_not_resume_raises_no_item() {
    let mut run = Run::new(false, true);
    through_bump(&mut run, plain());
    run.feed(PromoteEvent::Restored(Ok(vec![restored(
        "s1",
        vec![push_step()],
    )])));
    run.feed(PromoteEvent::Resumed {
        failed: vec![("s1".to_string(), "claude isn't there".to_string())],
    });
    run.feed(PromoteEvent::HeldRun);
    assert!(!run
        .all
        .iter()
        .any(|a| matches!(a, PromoteAction::PausedStep { .. })));
    assert_eq!(run.p.step, PromoteStep::Announce);
}

#[test]
fn codex_sessions_are_claimed_and_resumed() {
    let mut f = plain();
    f.set = Some(set_with(vec![
        session("c1", Tool::Codex, Movable::Yes),
        session("a1", Tool::Agy, Movable::Yes),
    ]));
    let mut run = Run::new(false, true);
    through_bump(&mut run, f);
    let restore = run
        .all
        .iter()
        .find_map(|a| match a {
            PromoteAction::Restore { sessions } => Some(sessions.clone()),
            _ => None,
        })
        .expect("restore");
    assert_eq!(restore, vec!["c1".to_string()]);
    run.feed(PromoteEvent::Restored(Ok(vec![restored("c1", vec![])])));
    assert!(run
        .all
        .iter()
        .filter_map(claim_of)
        .any(|claim| claim.session == "c1"));
    assert!(run
        .all
        .iter()
        .any(|a| matches!(a, PromoteAction::Resume { session, .. } if session == "c1")));
}

#[test]
fn a_codex_only_promote_runs_held_and_finishes() {
    let mut f = plain();
    f.set = Some(set_with(vec![session("c1", Tool::Codex, Movable::Yes)]));
    let mut run = Run::new(false, true);
    through_bump(&mut run, f);
    let actions = run.feed(PromoteEvent::Restored(Ok(vec![restored(
        "c1",
        vec![push_step()],
    )])));
    assert_eq!(run.p.step, PromoteStep::Resume);
    assert!(is_resume(actions.last().expect("resume")));
    assert!(!run.all.contains(&PromoteAction::RunHeld));
    run.feed(PromoteEvent::Resumed { failed: vec![] });
    assert_eq!(run.p.step, PromoteStep::Held);
    let actions = run.feed(PromoteEvent::HeldRun);
    assert!(matches!(&actions[0], PromoteAction::PausedStep { session, .. } if session == "c1"));
    run.feed(PromoteEvent::Announced);
    assert_eq!(run.p.step, PromoteStep::Done);
}

#[test]
fn a_mixed_set_waits_for_the_claude_resume() {
    let mut f = plain();
    f.set = Some(set_with(vec![
        session("c1", Tool::Codex, Movable::Yes),
        session("s1", Tool::Claude, Movable::Yes),
    ]));
    let mut run = Run::new(false, true);
    through_bump(&mut run, f);
    run.feed(PromoteEvent::Restored(Ok(vec![
        restored("c1", vec![]),
        restored("s1", vec![]),
    ])));
    assert_eq!(run.p.step, PromoteStep::Resume);
    assert!(!run.all.contains(&PromoteAction::RunHeld));
    run.feed(PromoteEvent::Resumed { failed: vec![] });
    assert_eq!(run.p.step, PromoteStep::Held);
}

#[test]
fn a_removed_member_cannot_promote() {
    let mut run = Run::new(false, true);
    let mut f = plain();
    f.state.members.get_mut(&id("m1")).expect("m1").state = MemberState::Removed;
    let actions = run.begin(f);
    let reason = failed(&run).expect("failed");
    assert!(reason.contains("isn't an active member"), "{reason}");
    assert_eq!(actions.len(), 1);
    assert!(!run.all.iter().any(is_term));
}

#[test]
fn a_failed_restore_is_terminal_and_leaves_the_term() {
    let mut run = Run::new(false, true);
    through_bump(&mut run, plain());
    let actions = run.feed(PromoteEvent::Restored(Err(
        "s1 ran on m5 after this set".to_string()
    )));
    assert_eq!(failed(&run), Some("s1 ran on m5 after this set"));
    assert_eq!(actions.len(), 1);
    assert!(run.all.iter().any(is_term));
    assert!(!run.all.iter().any(is_resume));
}

#[test]
fn failed_is_terminal() {
    let mut run = Run::new(false, true);
    run.begin(plain());
    let actions = run.feed(PromoteEvent::Abort("daemon is stopping".to_string()));
    assert_eq!(actions.len(), 1);
    let before = run.all.len();
    for ev in [
        PromoteEvent::Tick,
        PromoteEvent::NoAnswer,
        PromoteEvent::Answer,
        PromoteEvent::Bumped(Ok(())),
        PromoteEvent::Restored(Ok(vec![])),
        PromoteEvent::Resumed { failed: vec![] },
        PromoteEvent::HeldRun,
        PromoteEvent::Announced,
        PromoteEvent::Begin(Arc::new(plain())),
    ] {
        assert!(run.feed(ev).is_empty());
    }
    assert_eq!(run.all.len(), before);
    assert_eq!(failed(&run), Some("daemon is stopping"));
    assert!(run.p.is_terminal());
}

#[test]
fn events_for_another_step_change_nothing() {
    let mut run = Run::new(false, true);
    run.begin(plain());
    let before = run.p.clone();
    for ev in [
        PromoteEvent::Bumped(Ok(())),
        PromoteEvent::HeldRun,
        PromoteEvent::Announced,
        PromoteEvent::Resumed { failed: vec![] },
    ] {
        assert!(run.feed(ev).is_empty());
        assert_eq!(run.p, before);
    }
}

#[test]
fn announce_closes_the_saga() {
    let mut run = Run::new(false, true);
    through_bump(&mut run, plain());
    run.feed(PromoteEvent::Restored(Ok(vec![restored("s1", vec![])])));
    run.feed(PromoteEvent::Resumed { failed: vec![] });
    let actions = run.feed(PromoteEvent::HeldRun);
    assert_eq!(run.p.step, PromoteStep::Announce);
    let notice = actions
        .iter()
        .find_map(|a| match a {
            PromoteAction::Announce(n) => Some(n.clone()),
            _ => None,
        })
        .expect("announce");
    assert_eq!(notice.kind, "failover");
    assert_eq!(notice.text, "m1 is primary now");
    assert!(notice.actions.is_empty());
    assert_eq!(actions.last(), Some(&PromoteAction::ClusterChanged));
    assert!(run.feed(PromoteEvent::Announced).is_empty());
    assert_eq!(run.p.step, PromoteStep::Done);
    assert!(run.p.is_terminal());
}

#[test]
fn a_term_that_cannot_grow_fails_the_promote_cleanly() {
    let mut run = Run::new(false, true);
    let mut f = plain();
    f.state.term.n = MAX_TERM_N;
    run.begin(f);
    let actions = run.quiet_reach();
    assert!(
        matches!(&actions[..], [PromoteAction::Fail { .. }]),
        "{actions:?}"
    );
    assert!(failed(&run).is_some_and(|r| r.contains("can't go any higher")));
    assert!(!run.all.iter().any(is_term));
    assert!(!run.all.iter().any(|a| matches!(a, PromoteAction::Revoke)));
    assert!(run.p.is_terminal());
}

#[test]
fn the_last_storable_term_is_still_a_promote() {
    let mut run = Run::new(false, true);
    let mut f = plain();
    f.state.term.n = MAX_TERM_N - 1;
    run.begin(f);
    run.quiet_reach();
    assert_eq!(run.p.step, PromoteStep::Bump);
    let want = Term {
        n: MAX_TERM_N,
        node: id("m1"),
    };
    assert!(run
        .all
        .contains(&PromoteAction::Journal(Box::new(Command::Term {
            term: want.clone()
        }))));
    run.feed(PromoteEvent::Bumped(Ok(())));
    assert!(run.all.contains(&PromoteAction::PushTerm { term: want }));
}
