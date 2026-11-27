use std::path::Path;

use super::claims::Claim;
use super::claims_tests::{claim, cluster, copy};
use super::members::Term;
use super::returns::{
    continue_here, on_meet, on_return_with_primary_off, on_step_down, take_back, HandoverRequest,
    LocalCopy, Meet, ReturnAction, ReturnChoice, TakeBackStep,
};
use super::state::{apply, Command};
use super::testkit::{id, member};

fn local(len: u64, folder_changed: bool) -> LocalCopy {
    LocalCopy {
        len,
        folder_changed,
        path: Some("/home/me/.claude/projects/p/x.jsonl".to_string()),
        at_ms: 4_000,
    }
}

fn names(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| s.to_string()).collect()
}

fn newer_claim(session: &str, node: &str, n: u64, touched: bool) -> Claim {
    claim(session, node, n, 4, touched)
}

#[test]
fn newer_term_moves_claimed_sessions_and_stops_them() {
    let newer = member("m1", "macos", true);
    let claims = vec![
        newer_claim("s1", "m1", 2, false),
        newer_claim("s2", "m1", 2, true),
    ];
    let down = on_step_down(
        &id("m5"),
        &newer,
        &names(&["s1", "s2", "s3"]),
        &claims,
        &|_| local(1_000, false),
    );
    assert_eq!(
        down.moved,
        vec![
            ("s1".to_string(), id("m1").as_str().to_string()),
            ("s2".to_string(), id("m1").as_str().to_string())
        ]
    );
    assert_eq!(down.staying, names(&["s3"]));
    assert!(down.side_copies.is_empty());
}

#[test]
fn unclaimed_sessions_stay_worker_hosted() {
    let newer = member("m1", "macos", true);
    let claims = vec![
        claim("mine", "m5", 1, 1, false),
        newer_claim("not-hosted-here", "m1", 2, false),
    ];
    let down = on_step_down(
        &id("m5"),
        &newer,
        &names(&["a", "b", "mine"]),
        &claims,
        &|_| local(5_000, true),
    );
    assert!(down.moved.is_empty());
    assert_eq!(down.staying, names(&["a", "b", "mine"]));
    assert!(
        down.side_copies.is_empty(),
        "a session that stays has nothing to keep apart"
    );
}

#[test]
fn stepped_down_primary_keeps_side_copy() {
    let newer = member("m1", "macos", true);
    let claims = vec![
        newer_claim("longer", "m1", 2, false),
        newer_claim("folder", "m1", 2, false),
        newer_claim("both", "m1", 2, true),
        newer_claim("clean", "m1", 2, false),
    ];
    let down = on_step_down(
        &id("m5"),
        &newer,
        &names(&["longer", "folder", "both", "clean"]),
        &claims,
        &|session| match session {
            "longer" => local(1_500, false),
            "folder" => local(1_000, true),
            "both" => local(1_200, true),
            _ => local(1_000, false),
        },
    );
    assert_eq!(down.moved.len(), 4, "every one of them moved");
    let by_session = |s: &str| down.side_copies.iter().find(|c| c.session == s);
    let longer = by_session("longer").expect("longer transcript");
    assert_eq!(longer.extra_bytes, 500);
    assert!(!longer.repo_changed);
    assert_eq!(longer.node, id("m5"));
    assert_eq!(longer.base_set, "set-1");
    assert_eq!(
        longer.path.as_deref(),
        Some("/home/me/.claude/projects/p/x.jsonl")
    );
    assert_eq!(longer.at_ms, 4_000);
    assert!(!longer.hidden);
    let folder = by_session("folder").expect("changed folder");
    assert_eq!(folder.extra_bytes, 0);
    assert!(folder.repo_changed);
    let both = by_session("both").expect("both");
    assert_eq!(both.extra_bytes, 200);
    assert!(both.repo_changed);
    assert!(by_session("clean").is_none());
    assert_eq!(down.side_copies.len(), 3);

    let mut state = cluster();
    for c in &down.side_copies {
        apply(&mut state, Command::SideCopy { copy: c.clone() }, 9).expect("record");
    }
    assert_eq!(state.side_copies.len(), 3, "nothing is deleted or merged");
}

#[test]
fn primary_back_notice_offers_take_back_and_keep() {
    let newer = member("m1", "macos", true);
    let down = on_step_down(&id("m5"), &newer, &[], &[], &|_| local(0, false));
    let notice = down.notice;
    assert_eq!(notice.kind, "primary_back");
    assert_eq!(notice.text, "m1 is primary");
    let actions: Vec<_> = notice
        .actions
        .iter()
        .map(|a| (a.id, a.label.as_str(), a.primary))
        .collect();
    assert_eq!(
        actions,
        vec![
            (ReturnAction::TakeBack, "Take Back", true),
            (ReturnAction::Keep, "Keep m1", false)
        ]
    );
}

#[test]
fn untouched_session_continues() {
    let learned = vec![newer_claim("s1", "m1", 2, false)];
    let out = on_return_with_primary_off(&id("m5"), &learned, &names(&["s1"]));
    assert_eq!(out, vec![("s1".to_string(), ReturnChoice::Continue)]);
}

#[test]
fn touched_session_waits_by_default() {
    let learned = vec![
        newer_claim("touched", "m1", 2, true),
        newer_claim("fine", "m1", 2, false),
        claim("third", "w1", 5, 3, false),
        claim("older", "m1", 2, 4, false),
        claim("older", "m1", 1, 4, true),
    ];
    let out = on_return_with_primary_off(
        &id("m5"),
        &learned,
        &names(&["touched", "fine", "third", "older", "heard-nothing"]),
    );
    let m1 = id("m1").as_str().to_string();
    assert_eq!(
        out,
        vec![
            (
                "touched".to_string(),
                ReturnChoice::Wait {
                    for_node: m1.clone()
                }
            ),
            ("fine".to_string(), ReturnChoice::Continue),
            (
                "third".to_string(),
                ReturnChoice::Wait {
                    for_node: id("w1").as_str().to_string()
                }
            ),
            ("older".to_string(), ReturnChoice::Continue),
            (
                "heard-nothing".to_string(),
                ReturnChoice::Wait { for_node: m1 }
            ),
        ]
    );
}

#[test]
fn no_claim_record_means_wait_for_every_session() {
    let out = on_return_with_primary_off(&id("m5"), &[], &names(&["a", "b"]));
    assert_eq!(out.len(), 2);
    assert!(out
        .iter()
        .all(|(_, c)| matches!(c, ReturnChoice::Wait { for_node } if for_node.is_empty())));

    let only_mine = vec![claim("a", "m5", 3, 2, false), claim("b", "m5", 1, 2, true)];
    let out = on_return_with_primary_off(&id("m5"), &only_mine, &names(&["a", "b"]));
    assert!(out
        .iter()
        .all(|(_, c)| matches!(c, ReturnChoice::Wait { .. })));
}

#[test]
fn continue_from_this_copy_claims_here() {
    let theirs = newer_claim("s1", "m1", 2, true);
    let term = Term {
        n: 4,
        node: id("m1"),
    };
    let mine = continue_here("s1", &id("m5"), &term, Some(&theirs), "set-2", 2_000);
    assert_eq!(mine.node, id("m5"));
    assert_eq!(mine.n, 3);
    assert_eq!(mine.term, term);
    assert_eq!(mine.base_set, "set-2");
    assert_eq!(mine.base_len, 2_000);
    assert!(!mine.touched);
    assert_eq!(mine.session, "s1");
    assert_eq!(super::claims::winner(&theirs, &mine), &mine);

    let mut state = cluster();
    apply(&mut state, Command::Claim { claim: theirs }, 1).expect("theirs");
    apply(
        &mut state,
        Command::Claim {
            claim: mine.clone(),
        },
        2,
    )
    .expect("mine");
    assert_eq!(state.claims.get("s1"), Some(&mine));

    let fresh = continue_here("s9", &id("m5"), &term, None, "set-2", 0);
    assert_eq!(fresh.n, 1);
    assert_eq!(fresh.session, "s9");
}

#[test]
fn continue_from_this_copy_keeps_the_other() {
    let theirs = newer_claim("s1", "m1", 2, true);
    let term = Term {
        n: 4,
        node: id("m1"),
    };
    let mine = continue_here("s1", &id("m5"), &term, Some(&theirs), "set-2", 2_000);

    assert_eq!(on_meet(&mine, &theirs, 2_400), Meet::Keep);

    let Meet::Yield { owner, side_copy } = on_meet(&theirs, &mine, 3_500) else {
        panic!("the older claim yields");
    };
    assert_eq!(owner, id("m5"));
    let kept = side_copy.expect("its extra work is kept");
    assert_eq!(kept.node, id("m1"));
    assert_eq!(kept.session, "s1");
    assert_eq!(kept.base_set, "set-2");
    assert_eq!(kept.extra_bytes, 1_500);
    assert!(!kept.hidden);

    let Meet::Yield { side_copy, .. } = on_meet(&newer_claim("s1", "m1", 2, false), &mine, 1_900)
    else {
        panic!("the older claim yields");
    };
    assert!(side_copy.is_none(), "an untouched copy that grew nothing");

    let Meet::Yield { side_copy, .. } = on_meet(&theirs, &mine, 1_900) else {
        panic!("the older claim yields");
    };
    let kept = side_copy.expect("a touched copy is kept even when it looks shorter");
    assert_eq!(kept.extra_bytes, 0);

    assert_eq!(on_meet(&mine, &mine, 5), Meet::Keep);
}

#[cfg(unix)]
#[test]
fn take_back_moves_side_copies_aside_first() {
    let data = Path::new("/data");
    let mut one = copy("s1", "m5", "set-1", 10);
    one.path = Some("/home/me/.claude/projects/p/s1.jsonl".to_string());
    let mut hidden = copy("s2", "m5", "set-2", 10);
    hidden.path = Some("/home/me/.claude/projects/p/s2.jsonl".to_string());
    hidden.hidden = true;
    let no_path = copy("s3", "m5", "set-1", 10);
    let mut elsewhere = copy("s4", "m1", "set-1", 10);
    elsewhere.path = Some("/other/s4.jsonl".to_string());
    let mut already = copy("s5", "m5", "set-1", 10);
    already.path = Some("/data/cluster/copies/s5/set-1/s5.jsonl".to_string());

    let steps = take_back(
        &id("m5"),
        &id("m1"),
        &[one.clone(), hidden, no_path, elsewhere, already],
        data,
    )
    .expect("steps");
    assert_eq!(steps.len(), 3);
    let TakeBackStep::MoveAside { record, from, to } = &steps[0] else {
        panic!("a copy moves first");
    };
    assert_eq!(from, "/home/me/.claude/projects/p/s1.jsonl");
    assert_eq!(to, "/data/cluster/copies/s1/set-1/s1.jsonl");
    assert_eq!(record.path.as_deref(), Some(to.as_str()));
    assert_eq!(record.session, "s1");
    assert_eq!(record.extra_bytes, one.extra_bytes);
    let TakeBackStep::MoveAside { to, record, .. } = &steps[1] else {
        panic!("a hidden copy moves too");
    };
    assert_eq!(to, "/data/cluster/copies/s2/set-2/s2.jsonl");
    assert!(record.hidden);
    assert_eq!(
        steps[2],
        TakeBackStep::Handover(HandoverRequest {
            via: id("m1"),
            to: id("m5"),
            aside: true
        })
    );

    let bare = take_back(&id("m5"), &id("m1"), &[], data).expect("no copies");
    assert_eq!(
        bare,
        vec![TakeBackStep::Handover(HandoverRequest {
            via: id("m1"),
            to: id("m5"),
            aside: true
        })]
    );

    assert!(take_back(&id("m5"), &id("m5"), &[], data).is_err());
}

#[test]
fn take_back_refuses_names_that_would_leave_the_copies_folder() {
    let data = Path::new("/data");
    for (session, set) in [
        ("../x", "set-1"),
        ("s1", "../../etc"),
        ("a/b", "set-1"),
        ("s1", ""),
        ("", "set-1"),
        (".", "set-1"),
        ("s1", "a\\b"),
    ] {
        let mut c = copy(session, "m5", set, 1);
        c.path = Some("/home/me/s.jsonl".to_string());
        assert!(
            take_back(&id("m5"), &id("m1"), &[c], data).is_err(),
            "{session:?} {set:?}"
        );
    }
}
