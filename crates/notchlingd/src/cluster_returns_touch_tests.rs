use crate::cluster_fence_keeper_kit::{claim, S1};
use crate::cluster_returns_kit::{agent_row, kit, Kit};
use notchling_engine::cluster::state::Command;
use notchling_engine::sessions::AgentGroup;
use serde_json::json;

fn mine() -> crate::cluster_fence_keeper_kit::Seed {
    let mut seed = crate::cluster_returns_kit::seed_after_promote(&[]);
    seed.home = "m5";
    seed.claims = vec![claim(S1, "m5", 1)];
    seed
}

fn reply(k: &Kit, op: &str) {
    k.mover
        .gate()
        .check(op, &json!({"id": S1, "text": "go on"}), &|_| None)
        .expect("the gate lets it through");
}

fn touches(k: &Kit) -> usize {
    k.recorded()
        .iter()
        .filter(|c| matches!(c, Command::Touched { .. }))
        .count()
}

#[test]
fn touched_recorded_before_the_first_reply() {
    let k = kit("touch-reply", mine());
    reply(&k, "session.reply");
    assert_eq!(k.logged(), vec![format!("record touched {S1}")]);
    assert!(k.state().claims[S1].touched);
}

#[test]
fn touched_recorded_once_per_claim() {
    let k = kit("touch-once", mine());
    reply(&k, "session.reply");
    reply(&k, "session.send");
    reply(&k, "codex.send");
    k.returns.tick();
    assert_eq!(touches(&k), 1);
}

#[test]
fn touched_recorded_when_first_seen_working() {
    let k = kit("touch-working", mine());
    k.returns.tick();
    assert_eq!(touches(&k), 0);
    k.rows
        .0
        .lock()
        .unwrap()
        .push(agent_row(S1, "x", AgentGroup::Working));
    k.returns.tick();
    k.returns.tick();
    assert_eq!(touches(&k), 1);
    assert!(k.state().claims[S1].touched);
}

#[test]
fn a_failed_touch_does_not_block_the_reply_and_is_retried_by_the_tick() {
    let k = kit("touch-fails", mine());
    *k.link.fail.lock().unwrap() = Some("offline".to_string());
    reply(&k, "session.reply");
    reply(&k, "session.reply");
    assert_eq!(touches(&k), 0);
    assert!(k.item(&format!("touch-failed:{S1}")).is_some());
    *k.link.fail.lock().unwrap() = None;
    k.returns.tick();
    assert_eq!(touches(&k), 1);
}

#[test]
fn a_claim_held_elsewhere_or_already_touched_records_nothing() {
    let mut seed = mine();
    seed.claims = vec![claim(S1, "m1", 1)];
    let k = kit("touch-foreign", seed);
    reply(&k, "session.reply");
    assert_eq!(touches(&k), 0);

    let mut seed = mine();
    let mut touched = claim(S1, "m5", 1);
    touched.touched = true;
    seed.claims = vec![touched];
    let k = kit("touch-already", seed);
    reply(&k, "session.reply");
    assert_eq!(touches(&k), 0);
}

#[test]
fn other_ops_do_not_touch() {
    let k = kit("touch-other", mine());
    reply(&k, "session.answer");
    reply(&k, "session.stop");
    assert_eq!(touches(&k), 0);
}
