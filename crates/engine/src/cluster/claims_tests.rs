use super::claims::{before_tool_call, winner, CallGate, Claim, SideCopy};
use super::members::Term;
use super::persist::{load, save};
use super::state::{apply, start, ClusterState, Command, CommandError};
use super::testkit::{id, member};
use crate::sessions::testutil::TempDir;

pub(super) fn claim(session: &str, node: &str, n: u64, term_n: u64, touched: bool) -> Claim {
    Claim {
        session: session.to_string(),
        node: id(node),
        n,
        term: Term {
            n: term_n,
            node: id(node),
        },
        base_set: "set-1".to_string(),
        base_len: 1_000,
        touched,
    }
}

pub(super) fn copy(session: &str, node: &str, base_set: &str, extra: u64) -> SideCopy {
    SideCopy {
        session: session.to_string(),
        node: id(node),
        base_set: base_set.to_string(),
        extra_bytes: extra,
        repo_changed: false,
        path: None,
        at_ms: 500,
        hidden: false,
    }
}

pub(super) fn cluster() -> ClusterState {
    let (mut state, _, _) = start(
        Command::Create {
            id: "c1".to_string(),
            name: "home".to_string(),
            home: member("m5", "macos", true),
        },
        10,
    )
    .expect("create");
    for name in ["m1", "w1"] {
        let m = member(name, "macos", false);
        apply(
            &mut state,
            Command::ProposeJoin {
                member: m.clone(),
                item: format!("node-join:{}", m.id),
            },
            20,
        )
        .expect("propose");
        apply(
            &mut state,
            Command::Approve {
                node: m.id,
                client: false,
                can_be_master: name == "m1",
            },
            30,
        )
        .expect("approve");
    }
    state
}

fn claim_command(c: Claim) -> Command {
    Command::Claim { claim: c }
}

#[test]
fn higher_claim_wins_whatever_the_term_order() {
    let old_high_term = claim("s1", "m1", 2, 9, false);
    let new_low_term = claim("s1", "m5", 3, 1, false);
    assert_eq!(winner(&old_high_term, &new_low_term), &new_low_term);
    assert_eq!(winner(&new_low_term, &old_high_term), &new_low_term);

    let low = claim("s1", "m5", 3, 1, false);
    let high = claim("s1", "m5", 3, 2, false);
    assert_eq!(winner(&low, &high), &high);
    assert_eq!(winner(&high, &low), &high);

    let a = claim("s1", "m5", 3, 2, false);
    let b = claim("s1", "m1", 3, 2, false);
    let higher = if (&a.term, &a.node) > (&b.term, &b.node) {
        &a
    } else {
        &b
    };
    assert_eq!(winner(&a, &b), higher);
    assert_eq!(winner(&b, &a), higher);

    let mut state = cluster();
    apply(&mut state, claim_command(old_high_term.clone()), 40).expect("older");
    apply(&mut state, claim_command(new_low_term.clone()), 41).expect("newer");
    assert_eq!(state.claims.get("s1"), Some(&new_low_term));
    let before = state.clone();
    let again = apply(&mut state, claim_command(old_high_term), 42).expect("older again");
    assert!(again.entry.is_none() && again.changes.is_empty());
    assert_eq!(state, before);
}

#[test]
fn a_claim_by_an_unknown_machine_is_refused() {
    let mut state = cluster();
    let stranger = claim("s1", "nobody", 1, 1, false);
    assert!(matches!(
        apply(&mut state, claim_command(stranger), 40),
        Err(CommandError::Unknown(_))
    ));
    assert!(state.claims.is_empty());
}

#[test]
fn touched_is_recorded_before_the_first_call() {
    let mut state = cluster();
    apply(
        &mut state,
        claim_command(claim("s1", "m1", 1, 2, false)),
        40,
    )
    .expect("claim");
    let me = id("m1");

    let gate = before_tool_call(&state.claims, &me, "s1");
    let CallGate::Record(command) = gate else {
        panic!("the first call must wait for the record, got {gate:?}");
    };
    assert_eq!(
        *command,
        Command::Touched {
            session: "s1".to_string()
        }
    );
    assert!(!state.claims["s1"].touched);

    let applied = apply(&mut state, (*command).clone(), 41).expect("touched");
    assert!(applied.entry.is_some());
    assert!(state.claims["s1"].touched);
    assert_eq!(
        before_tool_call(&state.claims, &me, "s1"),
        CallGate::Pass,
        "the call passes only once the record is in the state"
    );

    let again = apply(&mut state, *command, 42).expect("touched twice");
    assert!(again.entry.is_none());

    assert_eq!(
        before_tool_call(&state.claims, &id("m5"), "s1"),
        CallGate::Foreign
    );
    assert_eq!(
        before_tool_call(&state.claims, &me, "unclaimed"),
        CallGate::Pass
    );
}

#[test]
fn touched_never_goes_back_and_needs_a_claim() {
    let mut state = cluster();
    assert!(matches!(
        apply(
            &mut state,
            Command::Touched {
                session: "s1".to_string()
            },
            40
        ),
        Err(CommandError::BadTransition)
    ));
    apply(&mut state, claim_command(claim("s1", "m1", 1, 2, true)), 41).expect("claim");
    apply(
        &mut state,
        claim_command(claim("s1", "m1", 1, 2, false)),
        42,
    )
    .expect("replay");
    assert!(state.claims["s1"].touched);
    apply(&mut state, claim_command(claim("s1", "m1", 1, 2, true)), 43).expect("replay");
    assert!(state.claims["s1"].touched);
}

#[test]
fn hiding_a_side_copy_keeps_it() {
    let mut state = cluster();
    let first = copy("s1", "m5", "set-1", 4_000);
    apply(&mut state, Command::SideCopy { copy: first }, 50).expect("side copy");
    apply(
        &mut state,
        Command::HideSideCopy {
            session: "s1".to_string(),
            node: id("m5"),
        },
        51,
    )
    .expect("hide");
    assert_eq!(state.side_copies.len(), 1);
    assert!(state.side_copies[0].hidden);
    assert_eq!(state.side_copies[0].extra_bytes, 4_000);

    let mut grown = copy("s1", "m5", "set-1", 9_000);
    grown.repo_changed = true;
    grown.path = Some("/copies/s1".to_string());
    grown.at_ms = 900;
    apply(&mut state, Command::SideCopy { copy: grown }, 52).expect("update");
    assert_eq!(state.side_copies.len(), 1, "an update never drops a copy");
    let kept = &state.side_copies[0];
    assert!(kept.hidden, "an update never shows a hidden copy again");
    assert_eq!(kept.extra_bytes, 9_000);
    assert!(kept.repo_changed);
    assert_eq!(kept.path.as_deref(), Some("/copies/s1"));
    assert_eq!(kept.at_ms, 500, "since when stays the first time");

    let smaller = copy("s1", "m5", "set-1", 10);
    apply(&mut state, Command::SideCopy { copy: smaller }, 53).expect("smaller");
    assert_eq!(state.side_copies[0].extra_bytes, 9_000);
    assert!(state.side_copies[0].repo_changed);
    assert_eq!(state.side_copies[0].path.as_deref(), Some("/copies/s1"));

    apply(
        &mut state,
        Command::SideCopy {
            copy: copy("s1", "m5", "set-2", 1),
        },
        54,
    )
    .expect("another set");
    apply(
        &mut state,
        Command::SideCopy {
            copy: copy("s1", "m1", "set-1", 1),
        },
        55,
    )
    .expect("another node");
    assert_eq!(state.side_copies.len(), 3);

    let again = apply(
        &mut state,
        Command::SideCopy {
            copy: copy("s1", "m1", "set-1", 1),
        },
        56,
    )
    .expect("same facts");
    assert!(again.entry.is_none());

    assert!(matches!(
        apply(
            &mut state,
            Command::HideSideCopy {
                session: "nope".to_string(),
                node: id("m5")
            },
            57
        ),
        Err(CommandError::BadTransition)
    ));
    apply(&mut state, Command::Remove { node: id("w1") }, 58).expect("remove");
    apply(
        &mut state,
        Command::Term {
            term: Term {
                n: 5,
                node: id("m1"),
            },
        },
        59,
    )
    .expect("term");
    assert_eq!(state.side_copies.len(), 3);
}

#[test]
fn a_side_copy_with_no_time_takes_the_journal_time() {
    let mut state = cluster();
    let mut c = copy("s1", "m5", "set-1", 3);
    c.at_ms = 0;
    apply(&mut state, Command::SideCopy { copy: c }, 777).expect("side copy");
    assert_eq!(state.side_copies[0].at_ms, 777);
}

#[test]
fn claims_and_side_copies_survive_cluster_db() {
    let tmp = TempDir::new("cluster-claims");
    let dir = tmp.path("cluster");
    let (mut state, entry, changes) = start(
        Command::Create {
            id: "c1".to_string(),
            name: "home".to_string(),
            home: member("m5", "macos", true),
        },
        1,
    )
    .expect("create");
    save(&dir, &entry, &changes).expect("save");
    for command in [
        claim_command(claim("s1", "m5", 1, 1, false)),
        claim_command(claim("s2", "m5", 4, 3, true)),
        Command::Touched {
            session: "s1".to_string(),
        },
        Command::SideCopy {
            copy: copy("s1", "m5", "set-1", 10),
        },
        Command::SideCopy {
            copy: copy("s2", "m5", "set-3", 20),
        },
        Command::HideSideCopy {
            session: "s2".to_string(),
            node: id("m5"),
        },
    ] {
        let applied = apply(&mut state, command, 5).expect("apply");
        save(
            &dir,
            applied.entry.as_ref().expect("entry"),
            &applied.changes,
        )
        .expect("save");
    }
    let loaded = load(&dir).expect("load").expect("state");
    assert_eq!(loaded, state);
    assert!(loaded.claims["s1"].touched);
    assert!(loaded.side_copies[1].hidden);
}

#[test]
fn a_state_written_before_claims_loads_with_none() {
    let json = serde_json::json!({
        "id": "c1", "name": "home",
        "term": {"n": 1, "node": id("m5")},
        "home": id("m5"), "vault": id("m5"),
        "members": {}, "jobs": {}, "seq": 0
    });
    let state: ClusterState = serde_json::from_value(json).expect("old state");
    assert!(state.claims.is_empty() && state.side_copies.is_empty());
}
