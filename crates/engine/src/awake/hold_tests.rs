use super::*;

fn setting(mode: Mode, display: bool) -> Setting {
    Setting { mode, display }
}

fn awake() -> (Awake, FakePower) {
    let power = FakePower::new();
    (Awake::new(Box::new(power.clone())), power)
}

#[test]
fn off_holds_nothing_and_asks_the_system_for_nothing() {
    let (mut awake, power) = awake();
    assert_eq!(
        awake.apply(setting(Mode::Off, false), || true),
        Outcome::Unchanged
    );
    assert_eq!(awake.holding(), None);
    assert!(power.events().is_empty());
}

#[test]
fn always_on_holds_once_and_keeps_that_one_hold() {
    let (mut awake, power) = awake();
    assert_eq!(
        awake.apply(setting(Mode::On, false), || false),
        Outcome::Held(Kind::System)
    );
    for _ in 0..3 {
        assert_eq!(
            awake.apply(setting(Mode::On, false), || false),
            Outcome::Unchanged
        );
    }
    assert_eq!(awake.holding(), Some(Kind::System));
    assert_eq!(power.events(), ["hold system"]);
}

#[test]
fn while_working_holds_at_the_first_working_session_and_lets_go_at_the_last() {
    let (mut awake, power) = awake();
    let mode = setting(Mode::WhileWorking, false);
    assert_eq!(awake.apply(mode, || false), Outcome::Unchanged);
    assert_eq!(awake.apply(mode, || true), Outcome::Held(Kind::System));
    assert_eq!(awake.apply(mode, || true), Outcome::Unchanged);
    assert_eq!(awake.apply(mode, || false), Outcome::Released);
    assert_eq!(awake.holding(), None);
    assert_eq!(awake.apply(mode, || false), Outcome::Unchanged);
    assert_eq!(power.events(), ["hold system", "release system"]);
}

#[test]
fn turning_the_mode_off_lets_go() {
    let (mut awake, power) = awake();
    awake.apply(setting(Mode::On, false), || false);
    assert_eq!(
        awake.apply(setting(Mode::Off, false), || false),
        Outcome::Released
    );
    assert_eq!(power.events(), ["hold system", "release system"]);
}

#[test]
fn flipping_the_display_swaps_the_hold_without_a_gap() {
    let (mut awake, power) = awake();
    awake.apply(setting(Mode::On, false), || false);
    assert_eq!(
        awake.apply(setting(Mode::On, true), || false),
        Outcome::Held(Kind::Display)
    );
    assert_eq!(awake.holding(), Some(Kind::Display));
    assert_eq!(
        power.events(),
        ["hold system", "hold display", "release system"]
    );
}

#[test]
fn the_display_flag_changes_nothing_while_nothing_is_held() {
    let (mut awake, power) = awake();
    let off = setting(Mode::WhileWorking, true);
    assert_eq!(awake.apply(off, || false), Outcome::Unchanged);
    assert!(power.events().is_empty());
}

#[test]
fn a_refusal_is_reported_once_and_tried_again_each_time() {
    let (mut awake, power) = awake();
    power.refuse_with(Some("no"));
    let on = setting(Mode::On, false);
    assert_eq!(awake.apply(on, || false), Outcome::Failed("no".into()));
    assert_eq!(awake.apply(on, || false), Outcome::Unchanged);
    assert_eq!(awake.holding(), None);
    power.refuse_with(None);
    assert_eq!(awake.apply(on, || false), Outcome::Held(Kind::System));
    assert_eq!(
        power.events(),
        ["refused system", "refused system", "hold system"]
    );
}

#[test]
fn a_refused_swap_keeps_the_hold_it_has() {
    let (mut awake, power) = awake();
    awake.apply(setting(Mode::On, false), || false);
    power.refuse_with(Some("busy"));
    assert_eq!(
        awake.apply(setting(Mode::On, true), || false),
        Outcome::Failed("busy".into())
    );
    assert_eq!(awake.holding(), Some(Kind::System));
    assert_eq!(power.events(), ["hold system", "refused display"]);
}

#[test]
fn release_and_drop_let_go_of_what_is_held() {
    let (mut awake, power) = awake();
    awake.apply(setting(Mode::On, false), || false);
    assert_eq!(awake.release(), Outcome::Released);
    assert_eq!(awake.release(), Outcome::Unchanged);
    awake.apply(setting(Mode::On, true), || false);
    drop(awake);
    assert_eq!(
        power.events(),
        [
            "hold system",
            "release system",
            "hold display",
            "release display"
        ]
    );
}
