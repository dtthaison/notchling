use super::{check, move_cast, run_move_seed, run_move_world, PendingMove, World};

fn seeds() -> Vec<u64> {
    if let Ok(one) = std::env::var("NOTCHLING_SIM_SEED") {
        if let Ok(seed) = one.parse() {
            return vec![seed];
        }
    }
    let n: u64 = std::env::var("NOTCHLING_SIM_SEEDS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(200);
    (0..n).collect()
}

#[test]
fn move_under_partitions_keeps_one_home_per_term() {
    for seed in seeds() {
        if let Err(failure) = run_move_seed(seed, 120) {
            panic!("{failure}");
        }
    }
}

#[test]
fn moves_happen_in_the_sim() {
    let mut lines: Vec<String> = Vec::new();
    for seed in 0..40 {
        let (world, result) = run_move_world(seed, 120);
        assert_eq!(result, Ok(()));
        lines.extend(world.trace().iter().cloned());
    }
    for want in [
        "call move.switch",
        "run move.switch",
        "stepped_down",
        "retry move.switch",
    ] {
        assert!(lines.iter().any(|l| l.contains(want)), "{want}");
    }
}

#[test]
fn a_clean_move_hands_the_term_to_the_target() {
    let mut world = World::new(31);
    let cast = move_cast(&mut world);
    world.move_home(&cast.m5, &cast.m1);
    world.run_for(10_000);
    for node in [&cast.m5, &cast.m1, &cast.m2, &cast.w1] {
        let state = world.core(node).and_then(|c| c.state()).expect("state");
        assert_eq!(state.home, cast.m1, "{}", world.name_of(node));
        assert_eq!(state.term.n, 2);
    }
    assert!(world.thinks_home(&cast.m1));
    assert!(!world.thinks_home(&cast.m5));
}

#[test]
fn a_move_toward_a_cut_off_machine_changes_nothing() {
    let mut world = World::new(32);
    let cast = move_cast(&mut world);
    world.partition(&cast.m5, &cast.m2);
    world.run_for(30_000);
    world.move_home(&cast.m5, &cast.m2);
    world.run_for(30_000);
    assert!(world.thinks_home(&cast.m5));
    let state = world.core(&cast.m5).and_then(|c| c.state()).expect("state");
    assert_eq!(state.term.n, 1);
}

fn until_home(world: &mut World, n: &super::super::id::NodeId) -> bool {
    for _ in 0..200 {
        world.run_for(1);
        if world.thinks_home(n) {
            return true;
        }
    }
    false
}

#[test]
fn a_lost_switch_answer_is_retried_and_lands_once() {
    let mut world = World::new(33);
    let cast = move_cast(&mut world);
    let mut mv = PendingMove::start(&mut world, &cast.m5, &cast.m1);
    assert!(until_home(&mut world, &cast.m1), "m1 never took the term");
    world.partition(&cast.m5, &cast.m1);
    world.run_for(2_000);
    assert_eq!(check(&world), Ok(()));
    let first = world.answer_to(&cast.m5, mv.call);
    assert!(!matches!(&first, Some(Ok(_))), "{first:?}");
    assert!(!world.retry_move(&mut mv), "the move isn't over yet");
    assert!(mv.maybe_sent);
    world.run_for(2_000);
    let retried = world.answer_to(&cast.m5, mv.call);
    assert!(
        !matches!(&retried, Some(Err(e)) if e.code == "refused"),
        "{retried:?}"
    );
    world.heal();
    let mut done = false;
    for _ in 0..30 {
        world.run_for(2_000);
        assert_eq!(check(&world), Ok(()));
        if world.retry_move(&mut mv) {
            done = true;
            break;
        }
    }
    assert!(done, "the retried move never finished");
    world.run_for(10_000);
    assert_eq!(check(&world), Ok(()));
    for node in [&cast.m5, &cast.m1, &cast.m2, &cast.w1] {
        let state = world.core(node).and_then(|c| c.state()).expect("state");
        assert_eq!(state.home, cast.m1, "{}", world.name_of(node));
        assert_eq!(state.term.n, 2);
    }
}

#[test]
fn a_retry_toward_a_term_already_taken_answers_ok() {
    let mut world = World::new(34);
    let cast = move_cast(&mut world);
    world.partition(&cast.m1, &cast.m2);
    world.partition(&cast.m1, &cast.w1);
    let first = world.move_home(&cast.m5, &cast.m1);
    assert!(until_home(&mut world, &cast.m1));
    let again = world.move_home(&cast.m5, &cast.m1);
    world.run_for(500);
    let answers = [
        world.answer_to(&cast.m5, first),
        world.answer_to(&cast.m5, again),
    ];
    assert!(
        answers
            .iter()
            .all(|a| !matches!(a, Some(Err(e)) if e.code == "refused")),
        "{answers:?}"
    );
    assert_eq!(check(&world), Ok(()));
}
