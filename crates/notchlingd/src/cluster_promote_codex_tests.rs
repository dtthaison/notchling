use crate::cluster_fence::{mark_moved, read_fence};
use crate::cluster_move_tests::{logged, Spec, CODEX1};
use crate::cluster_promote_kit::{rig_with, Rig};
use notchling_engine::cluster::fence::Gate;
use notchling_engine::cluster::members::Term;
use notchling_engine::cluster::wire::WireError;
use notchling_engine::inbox::Kind;

fn gate(r: &Rig) -> Option<Gate> {
    read_fence(&r.p.w.cluster).and_then(|f| f.sessions.get(CODEX1).cloned())
}

fn codex_rig() -> Rig {
    let r = rig_with(
        &[Spec {
            id: CODEX1,
            flags: &[],
            movable: true,
        }],
        true,
    );
    let a = r.a();
    r.answer(&a, "node.load", Err(WireError::new("not_sent", "offline")));
    let term = Term { n: 1, node: r.a() };
    mark_moved(&r.p.w.cluster, &r.a(), term, &[CODEX1.to_string()], "m5").unwrap();
    assert_eq!(gate(&r), Some(Gate::Moved { to: "m5".into() }));
    r
}

#[test]
fn a_promoted_codex_session_has_its_moved_gate_lifted() {
    let r = codex_rig();
    r.promoter.start(true).expect("an answer");
    r.finish();
    assert_eq!(gate(&r), None);
    assert!(
        logged(&r.p).iter().all(|l| !l.contains("start")),
        "{:?}",
        logged(&r.p)
    );
    assert_eq!(r.promoter.status()["step"], "done");
}

#[test]
fn a_failed_codex_claim_is_reported_and_keeps_the_gate() {
    let r = codex_rig();
    *r.cluster.record_fail.lock().unwrap() = Some("no home".to_string());
    r.promoter.start(true).expect("an answer");
    r.finish();
    let item = r
        .item(&format!("promote-resume:2:{CODEX1}"))
        .expect("a notice");
    assert_eq!(item.kind, Kind::ActionFailed);
    assert!(item.detail.contains("no home"));
    assert_eq!(gate(&r), Some(Gate::Moved { to: "m5".into() }));
    assert_eq!(r.promoter.status()["step"], "done");
}
