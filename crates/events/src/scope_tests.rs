use crate::envelope::{Actor, ActorTier};
use crate::ids::new_op_id;
use crate::scope::{current, enter, mark_recorded, take_recorded, Scope};

fn scope_with_id() -> Scope {
    Scope {
        op_id: Some(new_op_id()),
        actor: Some(Actor {
            tier: ActorTier::Agent,
            via: None,
            exe: None,
            device: None,
            session: None,
        }),
    }
}

#[test]
fn enter_and_drop_restore_previous() {
    assert!(current().op_id.is_none());
    let outer = scope_with_id();
    let outer_id = outer.op_id.clone();
    let guard = enter(outer);
    assert_eq!(current().op_id, outer_id);
    {
        let _inner = enter(scope_with_id());
        assert_ne!(current().op_id, outer_id);
    }
    assert_eq!(current().op_id, outer_id);
    drop(guard);
    assert!(current().op_id.is_none());
    assert!(current().actor.is_none());
}

#[test]
fn mark_recorded_is_taken_once() {
    let _guard = enter(scope_with_id());
    assert!(!take_recorded());
    mark_recorded();
    assert!(take_recorded());
    assert!(!take_recorded());
}

#[test]
fn recorded_in_an_inner_scope_reaches_the_outer_one() {
    let _outer = enter(scope_with_id());
    {
        let _inner = enter(scope_with_id());
        mark_recorded();
    }
    assert!(take_recorded());
}

#[test]
fn run_carries_scope_to_another_thread() {
    let scope = scope_with_id();
    let expected = scope.op_id.clone();
    let _guard = enter(scope);
    let carried = current();
    let seen = std::thread::spawn(move || {
        assert!(current().op_id.is_none());
        carried.run(|| current().op_id)
    })
    .join()
    .expect("thread");
    assert_eq!(seen, expected);
}
