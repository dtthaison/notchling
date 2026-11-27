use super::*;

const S: i64 = 1000;
const T0: i64 = 1_000_000;

fn key(id: &str) -> BlockingTarget {
    BlockingTarget::Item(id.to_owned())
}

fn target(id: &str, since: i64, expires: Option<i64>) -> Target {
    Target {
        key: key(id),
        title: "Turn On Machines".to_owned(),
        waiting_since_ms: since,
        expires_ms: expires,
        alert_actions: vec![AlertAction::Allow, AlertAction::Deny, AlertAction::Change],
    }
}

fn input(level: Level, targets: Vec<Target>) -> Input {
    Input {
        targets,
        level,
        notch_shown: true,
        notifications: Notifications::Allowed,
        reduce_motion: false,
        app_active: false,
        delivered: Vec::new(),
        closed: Vec::new(),
        expired: Vec::new(),
    }
}

fn run(a: &mut Attention, i: &Input, at: i64) -> Vec<Effect> {
    a.step(i, T0 + at * S)
}

fn banners(fx: &[Effect]) -> Vec<(BannerStep, bool)> {
    fx.iter()
        .filter_map(|e| match e {
            Effect::Banner { step, sound, .. } => Some((*step, *sound)),
            _ => None,
        })
        .collect()
}

fn tones(fx: &[Effect]) -> Vec<u8> {
    fx.iter()
        .filter_map(|e| match e {
            Effect::Tone { times } => Some(*times),
            _ => None,
        })
        .collect()
}

fn has(fx: &[Effect], want: &Effect) -> bool {
    fx.contains(want)
}

fn pulses(fx: &[Effect]) -> usize {
    fx.iter().filter(|e| **e == Effect::PulseTray).count()
}

fn one(level: Level, expires: Option<i64>) -> Input {
    input(level, vec![target("a", T0, expires)])
}

#[test]
fn normal_start_opens_the_notch_posts_a_banner_with_a_tone_and_badges_the_dock() {
    let mut a = Attention::new();
    let i = one(Level::Normal, Some(T0 + 3600 * S));
    let fx = run(&mut a, &i, 0);
    assert!(has(
        &fx,
        &Effect::OpenNotch {
            key: key("a"),
            ms: 45_000,
            stay: false
        }
    ));
    assert_eq!(banners(&fx), vec![(BannerStep::Start, true)]);
    assert_eq!(tones(&fx), vec![1]);
    assert_eq!(pulses(&fx), 1);
    assert!(has(&fx, &Effect::DockBadge(Some(1))));
    assert!(!fx.iter().any(|e| matches!(e, Effect::DockBounce { .. })));
}

#[test]
fn normal_ladder_fires_at_0_60_180_last_call_and_expiry() {
    let mut a = Attention::new();
    let i = one(Level::Normal, Some(T0 + 600 * S));
    assert!(!run(&mut a, &i, 0).is_empty());
    assert!(run(&mut a, &i, 10).is_empty());
    assert!(run(&mut a, &i, 59).is_empty());

    let fx = run(&mut a, &i, 60);
    assert!(has(&fx, &Effect::GrowNotch { key: key("a") }));
    assert_eq!(banners(&fx), vec![(BannerStep::Pulse, true)]);
    assert_eq!(tones(&fx), vec![1]);
    assert_eq!(pulses(&fx), 1);

    assert!(run(&mut a, &i, 100).is_empty());

    let fx = run(&mut a, &i, 180);
    assert!(has(
        &fx,
        &Effect::OpenNotch {
            key: key("a"),
            ms: 45_000,
            stay: false
        }
    ));
    assert_eq!(banners(&fx), vec![(BannerStep::Stronger, true)]);
    assert_eq!(tones(&fx), vec![1]);
    assert!(has(&fx, &Effect::DockBounce { critical: false }));

    assert!(run(&mut a, &i, 400).is_empty());

    let fx = run(&mut a, &i, 540);
    assert_eq!(banners(&fx), vec![(BannerStep::LastCall, true)]);
    assert_eq!(tones(&fx), vec![1]);
    assert!(has(&fx, &Effect::DockBounce { critical: false }));
    assert!(fx.iter().any(|e| matches!(e, Effect::OpenNotch { .. })));

    let mut ended = input(Level::Normal, Vec::new());
    ended.expired = vec![key("a")];
    let fx = run(&mut a, &ended, 600);
    assert_eq!(banners(&fx), vec![(BannerStep::Expired, false)]);
    assert!(tones(&fx).is_empty());
    assert_eq!(pulses(&fx), 0);
    assert!(has(
        &fx,
        &Effect::OpenNotch {
            key: key("a"),
            ms: 4_000,
            stay: false
        }
    ));
}

#[test]
fn calm_is_silent_and_never_pulses() {
    let mut a = Attention::new();
    let i = one(Level::Calm, Some(T0 + 600 * S));
    let fx = run(&mut a, &i, 0);
    assert!(has(
        &fx,
        &Effect::OpenNotch {
            key: key("a"),
            ms: 8_000,
            stay: false
        }
    ));
    assert_eq!(banners(&fx), vec![(BannerStep::Start, false)]);
    assert!(tones(&fx).is_empty());
    assert_eq!(pulses(&fx), 0);
    assert!(!fx.iter().any(|e| matches!(e, Effect::DockBounce { .. })));
    assert!(run(&mut a, &i, 60).is_empty());
    assert!(run(&mut a, &i, 180).is_empty());
    let fx = run(&mut a, &i, 540);
    assert_eq!(banners(&fx), vec![(BannerStep::LastCall, false)]);
    assert!(tones(&fx).is_empty());
    assert_eq!(pulses(&fx), 0);
}

#[test]
fn urgent_keeps_the_notch_open_tones_twice_and_bounces_critically() {
    let mut a = Attention::new();
    let i = one(Level::Urgent, Some(T0 + 3600 * S));
    let fx = run(&mut a, &i, 0);
    assert!(has(
        &fx,
        &Effect::OpenNotch {
            key: key("a"),
            ms: 45_000,
            stay: true
        }
    ));
    assert_eq!(tones(&fx), vec![2]);
    assert!(has(&fx, &Effect::DockBounce { critical: true }));
    assert_eq!(banners(&fx), vec![(BannerStep::Start, true)]);
}

#[test]
fn urgent_repeats_at_120_then_every_120_after_180() {
    let mut a = Attention::new();
    let i = one(Level::Urgent, None);
    run(&mut a, &i, 0);
    assert_eq!(
        banners(&run(&mut a, &i, 60)),
        vec![(BannerStep::Pulse, true)]
    );
    assert_eq!(
        banners(&run(&mut a, &i, 120)),
        vec![(BannerStep::Pulse, true)]
    );
    assert_eq!(
        banners(&run(&mut a, &i, 180)),
        vec![(BannerStep::Stronger, true)]
    );
    assert!(run(&mut a, &i, 240).is_empty());
    let fx = run(&mut a, &i, 300);
    assert_eq!(banners(&fx), vec![(BannerStep::Pulse, true)]);
    assert_eq!(tones(&fx), vec![2]);
    assert!(run(&mut a, &i, 360).is_empty());
    assert_eq!(
        banners(&run(&mut a, &i, 420)),
        vec![(BannerStep::Pulse, true)]
    );
}

#[test]
fn normal_with_no_end_stops_at_hold() {
    let mut a = Attention::new();
    let i = one(Level::Normal, None);
    run(&mut a, &i, 0);
    run(&mut a, &i, 60);
    run(&mut a, &i, 180);
    for at in [240, 300, 600, 3600, 7200] {
        assert!(run(&mut a, &i, at).is_empty(), "at {at}");
    }
}

#[test]
fn a_sleep_jump_fires_only_the_latest_step_with_one_tone() {
    let mut a = Attention::new();
    let i = one(Level::Normal, None);
    run(&mut a, &i, 0);
    assert!(run(&mut a, &i, 30).is_empty());
    let fx = run(&mut a, &i, 600);
    assert_eq!(banners(&fx), vec![(BannerStep::Stronger, true)]);
    assert_eq!(tones(&fx), vec![1]);
    assert_eq!(pulses(&fx), 1);
    assert!(run(&mut a, &i, 601).is_empty());
}

#[test]
fn a_jump_across_the_end_makes_no_tone() {
    let mut a = Attention::new();
    let i = one(Level::Normal, Some(T0 + 300 * S));
    run(&mut a, &i, 0);
    let mut ended = input(Level::Normal, Vec::new());
    ended.expired = vec![key("a")];
    let fx = run(&mut a, &ended, 900);
    assert_eq!(banners(&fx), vec![(BannerStep::Expired, false)]);
    assert!(tones(&fx).is_empty());
}

#[test]
fn a_jump_across_the_end_of_an_open_item_is_a_silent_last_call() {
    let mut a = Attention::new();
    let i = one(Level::Normal, Some(T0 + 300 * S));
    run(&mut a, &i, 0);
    let fx = run(&mut a, &i, 900);
    assert_eq!(banners(&fx), vec![(BannerStep::LastCall, false)]);
    assert!(tones(&fx).is_empty());
}

#[test]
fn an_open_item_still_listed_after_its_end_never_posts_expired() {
    let mut a = Attention::new();
    let i = one(Level::Normal, Some(T0 + 600 * S));
    run(&mut a, &i, 0);
    run(&mut a, &i, 540);
    let fx = run(&mut a, &i, 601);
    assert!(banners(&fx).is_empty());
    assert!(!fx.iter().any(|e| matches!(
        e,
        Effect::Banner {
            step: BannerStep::Expired,
            ..
        }
    )));
}

#[test]
fn an_answered_item_removes_its_banner_and_never_posts_expired_even_after_its_end() {
    let mut a = Attention::new();
    let i = one(Level::Normal, Some(T0 + 600 * S));
    run(&mut a, &i, 0);
    let mut closed = input(Level::Normal, Vec::new());
    closed.closed = vec![key("a")];
    let fx = run(&mut a, &closed, 700);
    assert!(banners(&fx).is_empty());
    assert!(has(&fx, &Effect::RemoveBanner { key: key("a") }));
    assert!(!fx.iter().any(|e| matches!(e, Effect::OpenNotch { .. })));
}

#[test]
fn a_delivered_banner_is_removed_on_answer_even_when_the_machine_never_posted_it() {
    let mut a = Attention::new();
    let mut i = input(Level::Normal, vec![target("a", T0 - 240 * S, None)]);
    i.delivered = vec![key("a")];
    let fx = a.step(&i, T0);
    assert!(banners(&fx).is_empty());
    let mut closed = input(Level::Normal, Vec::new());
    closed.delivered = vec![key("a")];
    closed.closed = vec![key("a")];
    let fx = a.step(&closed, T0 + 5 * S);
    assert!(has(&fx, &Effect::RemoveBanner { key: key("a") }));
}

#[test]
fn an_expired_item_posts_a_silent_expired_banner_and_opens_the_notch_briefly() {
    let mut a = Attention::new();
    let i = one(Level::Normal, Some(T0 + 600 * S));
    run(&mut a, &i, 0);
    let mut ended = input(Level::Normal, Vec::new());
    ended.expired = vec![key("a")];
    let fx = run(&mut a, &ended, 601);
    assert_eq!(banners(&fx), vec![(BannerStep::Expired, false)]);
    assert!(!has(&fx, &Effect::RemoveBanner { key: key("a") }));
}

#[test]
fn the_machine_without_a_reason_still_reads_a_vanished_item_past_its_end_as_expired() {
    let mut a = Attention::new();
    let i = one(Level::Normal, Some(T0 + 600 * S));
    run(&mut a, &i, 0);
    let fx = run(&mut a, &input(Level::Normal, Vec::new()), 601);
    assert_eq!(banners(&fx), vec![(BannerStep::Expired, false)]);
}

fn urgent_pair() -> (Attention, Input) {
    let mut a = Attention::new();
    let mut i = input(
        Level::Urgent,
        vec![target("a", T0, None), target("b", T0, None)],
    );
    i.notch_shown = false;
    let fx = run(&mut a, &i, 0);
    assert_eq!(
        fx.iter()
            .filter(|e| matches!(e, Effect::DockBounce { critical: true }))
            .count(),
        2
    );
    (a, i)
}

#[test]
fn answering_one_of_two_bouncing_items_cancels_and_keeps_the_other_bouncing() {
    let (mut a, _) = urgent_pair();
    let mut left = input(Level::Urgent, vec![target("b", T0, None)]);
    left.notch_shown = false;
    left.closed = vec![key("a")];
    let fx = run(&mut a, &left, 5);
    let cancel = fx.iter().position(|e| *e == Effect::CancelBounce);
    let again = fx
        .iter()
        .position(|e| *e == Effect::DockBounce { critical: true });
    assert!(cancel.is_some());
    assert!(again > cancel);
    let mut none = input(Level::Urgent, Vec::new());
    none.notch_shown = false;
    none.closed = vec![key("b")];
    let fx = run(&mut a, &none, 9);
    assert!(has(&fx, &Effect::CancelBounce));
    assert!(!fx.iter().any(|e| matches!(e, Effect::DockBounce { .. })));
}

#[test]
fn answering_the_item_that_started_the_bounce_cancels_it_while_a_quiet_one_remains() {
    let mut a = Attention::new();
    let mut i = input(Level::Urgent, vec![target("a", T0, None)]);
    i.notch_shown = false;
    assert!(has(
        &run(&mut a, &i, 0),
        &Effect::DockBounce { critical: true }
    ));
    i.targets.push(target("b", T0 - 300 * S, None));
    run(&mut a, &i, 20);
    let mut left = input(Level::Urgent, vec![target("b", T0 - 300 * S, None)]);
    left.notch_shown = false;
    left.closed = vec![key("a")];
    let fx = run(&mut a, &left, 30);
    assert!(has(&fx, &Effect::CancelBounce));
    assert!(!fx.iter().any(|e| matches!(e, Effect::DockBounce { .. })));
}

#[test]
fn an_active_app_cancels_the_bounce_once() {
    let mut a = Attention::new();
    let mut i = input(Level::Urgent, vec![target("a", T0, None)]);
    i.notch_shown = false;
    run(&mut a, &i, 0);
    i.app_active = true;
    let fx = run(&mut a, &i, 3);
    assert_eq!(fx.iter().filter(|e| **e == Effect::CancelBounce).count(), 1);
    assert!(!run(&mut a, &i, 4).contains(&Effect::CancelBounce));
}

#[test]
fn the_tray_holds_still_for_calm_and_for_reduce_motion() {
    assert!(tray_still(Level::Calm, false));
    assert!(tray_still(Level::Normal, true));
    assert!(tray_still(Level::Urgent, true));
    assert!(!tray_still(Level::Normal, false));
    assert!(!tray_still(Level::Urgent, false));
}

#[test]
fn first_sight_at_240_is_a_silent_banner_without_the_notch() {
    let mut a = Attention::new();
    let i = input(Level::Normal, vec![target("a", T0 - 240 * S, None)]);
    let fx = a.step(&i, T0);
    assert_eq!(banners(&fx), vec![(BannerStep::Stronger, false)]);
    assert!(tones(&fx).is_empty());
    assert!(!fx.iter().any(|e| matches!(
        e,
        Effect::OpenNotch { .. } | Effect::GrowNotch { .. } | Effect::DockBounce { .. }
    )));
    assert_eq!(pulses(&fx), 0);
    assert!(has(&fx, &Effect::DockBadge(Some(1))));
    assert!(a.step(&i, T0 + 30 * S).is_empty());
}

#[test]
fn first_sight_skips_the_banner_when_one_is_delivered() {
    let mut a = Attention::new();
    let mut i = input(Level::Normal, vec![target("a", T0 - 240 * S, None)]);
    i.delivered = vec![key("a")];
    let fx = a.step(&i, T0);
    assert!(banners(&fx).is_empty());
}

#[test]
fn first_sight_under_10s_is_a_normal_start() {
    let mut a = Attention::new();
    let i = input(Level::Normal, vec![target("a", T0 - 8 * S, None)]);
    let fx = a.step(&i, T0);
    assert_eq!(banners(&fx), vec![(BannerStep::Start, true)]);
}

#[test]
fn first_sight_later_steps_run_as_normal() {
    let mut a = Attention::new();
    let i = input(Level::Normal, vec![target("a", T0 - 120 * S, None)]);
    a.step(&i, T0);
    assert!(a.step(&i, T0 + 30 * S).is_empty());
    let fx = a.step(&i, T0 + 60 * S);
    assert_eq!(banners(&fx), vec![(BannerStep::Stronger, true)]);
}

#[test]
fn hide_notch_toggled_mid_ladder_stops_notch_effects_and_bounces_at_start() {
    let mut a = Attention::new();
    let mut i = one(Level::Normal, Some(T0 + 3600 * S));
    run(&mut a, &i, 0);
    i.notch_shown = false;
    let fx = run(&mut a, &i, 60);
    assert!(!fx.iter().any(|e| matches!(
        e,
        Effect::OpenNotch { .. } | Effect::GrowNotch { .. } | Effect::CloseNotch
    )));
    assert_eq!(banners(&fx), vec![(BannerStep::Pulse, true)]);

    let mut b = Attention::new();
    let hidden = input(Level::Normal, vec![target("a", T0, Some(T0 + 3600 * S))]);
    let mut hidden = hidden;
    hidden.notch_shown = false;
    let fx = run(&mut b, &hidden, 0);
    assert!(has(&fx, &Effect::DockBounce { critical: false }));
    assert!(!fx.iter().any(|e| matches!(e, Effect::OpenNotch { .. })));
    assert_eq!(banners(&fx), vec![(BannerStep::Start, true)]);
}

#[test]
fn answered_elsewhere_removes_the_banner_drops_the_badge_and_cancels_the_bounce() {
    let mut a = Attention::new();
    let mut i = one(Level::Normal, Some(T0 + 3600 * S));
    i.notch_shown = false;
    let fx = run(&mut a, &i, 0);
    assert!(has(&fx, &Effect::DockBounce { critical: false }));
    let gone = input(Level::Normal, Vec::new());
    let fx = run(&mut a, &gone, 20);
    assert!(has(&fx, &Effect::RemoveBanner { key: key("a") }));
    assert!(has(&fx, &Effect::DockBadge(None)));
    assert!(has(&fx, &Effect::CancelBounce));
    assert!(run(&mut a, &gone, 21).is_empty());
}

#[test]
fn answered_closes_a_notch_that_was_held_open() {
    let mut a = Attention::new();
    let i = one(Level::Urgent, None);
    run(&mut a, &i, 0);
    let fx = run(&mut a, &input(Level::Urgent, Vec::new()), 5);
    assert!(has(&fx, &Effect::CloseNotch));
}

#[test]
fn later_lets_go_of_the_held_notch() {
    let mut a = Attention::new();
    let i = one(Level::Urgent, None);
    run(&mut a, &i, 0);
    a.later(&key("a"));
    let fx = run(&mut a, &input(Level::Urgent, Vec::new()), 5);
    assert!(!has(&fx, &Effect::CloseNotch));
}

#[test]
fn three_targets_within_two_seconds_post_three_banners_and_one_tone() {
    let mut a = Attention::new();
    let i = input(
        Level::Normal,
        vec![
            target("a", T0, None),
            target("b", T0 + S, None),
            target("c", T0 + 2 * S, None),
        ],
    );
    let fx = a.step(&i, T0 + 2 * S);
    assert_eq!(banners(&fx).len(), 3);
    assert_eq!(banners(&fx).iter().filter(|(_, s)| *s).count(), 1);
    assert_eq!(tones(&fx), vec![1]);
    assert!(has(&fx, &Effect::DockBadge(Some(3))));
}

#[test]
fn tones_keep_four_seconds_apart_across_calls() {
    let mut a = Attention::new();
    let first = input(Level::Normal, vec![target("a", T0, None)]);
    a.step(&first, T0);
    let both = input(
        Level::Normal,
        vec![target("a", T0, None), target("b", T0 + 2 * S, None)],
    );
    let fx = a.step(&both, T0 + 3 * S);
    assert_eq!(banners(&fx), vec![(BannerStep::Start, false)]);
    assert!(tones(&fx).is_empty());
    let late = input(
        Level::Normal,
        vec![
            target("a", T0, None),
            target("b", T0 + 2 * S, None),
            target("c", T0 + 4 * S, None),
        ],
    );
    let fx = a.step(&late, T0 + 4 * S);
    assert_eq!(banners(&fx), vec![(BannerStep::Start, true)]);
}

#[test]
fn hook_prompts_step_at_0_60_and_230_and_nothing_at_180() {
    let mut a = Attention::new();
    let hook = Target {
        key: BlockingTarget::HookPrompt("p1".to_owned()),
        title: "Bash".to_owned(),
        waiting_since_ms: T0,
        expires_ms: Some(T0 + 290 * S),
        alert_actions: vec![AlertAction::Allow, AlertAction::Deny],
    };
    let i = input(Level::Normal, vec![hook]);
    assert_eq!(
        banners(&run(&mut a, &i, 0)),
        vec![(BannerStep::Start, true)]
    );
    assert_eq!(
        banners(&run(&mut a, &i, 60)),
        vec![(BannerStep::Pulse, true)]
    );
    assert!(run(&mut a, &i, 180).is_empty());
    assert_eq!(
        banners(&run(&mut a, &i, 230)),
        vec![(BannerStep::LastCall, true)]
    );
}

#[test]
fn an_item_ending_in_three_minutes_skips_stronger() {
    let mut a = Attention::new();
    let i = one(Level::Normal, Some(T0 + 180 * S));
    let mut seen = Vec::new();
    for at in 0..=180 {
        seen.extend(banners(&run(&mut a, &i, at)).into_iter().map(|b| b.0));
    }
    assert!(!seen.contains(&BannerStep::Stronger));
    assert!(seen.contains(&BannerStep::LastCall));
    assert!(!seen.contains(&BannerStep::Expired));
    let mut ended = input(Level::Normal, Vec::new());
    ended.expired = vec![key("a")];
    assert_eq!(
        banners(&run(&mut a, &ended, 181)),
        vec![(BannerStep::Expired, false)]
    );
}

#[test]
fn an_open_item_past_its_end_is_at_last_call() {
    let mut a = Attention::new();
    let i = input(
        Level::Normal,
        vec![target("a", T0 - 100 * S, Some(T0 + 30 * S))],
    );
    let fx = a.step(&i, T0);
    assert_eq!(banners(&fx), vec![(BannerStep::LastCall, false)]);
}

#[test]
fn a_waiting_since_in_the_future_counts_as_now() {
    let mut a = Attention::new();
    let i = input(Level::Normal, vec![target("a", T0 + 500 * S, None)]);
    let fx = a.step(&i, T0);
    assert_eq!(banners(&fx), vec![(BannerStep::Start, true)]);
    assert!(a.step(&i, T0 + 10 * S).is_empty());
}

#[test]
fn denied_notifications_post_no_banner_and_warn_once() {
    let mut a = Attention::new();
    let mut i = one(Level::Normal, None);
    i.notifications = Notifications::Denied;
    let fx = run(&mut a, &i, 0);
    assert!(banners(&fx).is_empty());
    assert!(tones(&fx).is_empty());
    assert_eq!(
        fx.iter()
            .filter(|e| **e == Effect::Warn(Warning::NoChannel))
            .count(),
        1
    );
    let fx = run(&mut a, &i, 60);
    assert!(banners(&fx).is_empty());
    assert!(!has(&fx, &Effect::Warn(Warning::NoChannel)));
    assert!(has(&fx, &Effect::GrowNotch { key: key("a") }));
}

#[test]
fn reduce_motion_never_pulses_the_tray() {
    let mut a = Attention::new();
    let mut i = one(Level::Urgent, Some(T0 + 3600 * S));
    i.reduce_motion = true;
    for at in [0, 60, 120, 180, 300] {
        assert_eq!(pulses(&run(&mut a, &i, at)), 0, "at {at}");
    }
}

#[test]
fn nothing_blocking_means_no_effect_at_any_level() {
    for level in [Level::Calm, Level::Normal, Level::Urgent] {
        let mut a = Attention::new();
        let i = input(level, Vec::new());
        for at in [0, 10, 60, 180, 600] {
            assert!(run(&mut a, &i, at).is_empty());
        }
    }
}

#[test]
fn an_active_app_gets_no_dock_bounce() {
    let mut a = Attention::new();
    let mut i = one(Level::Urgent, None);
    i.app_active = true;
    let fx = run(&mut a, &i, 0);
    assert!(!fx.iter().any(|e| matches!(e, Effect::DockBounce { .. })));
}

#[test]
fn a_level_change_mid_ladder_does_not_replay_old_steps() {
    let mut a = Attention::new();
    let calm = one(Level::Calm, None);
    run(&mut a, &calm, 0);
    let normal = one(Level::Normal, None);
    assert!(run(&mut a, &normal, 100).is_empty());
    assert_eq!(
        banners(&run(&mut a, &normal, 180)),
        vec![(BannerStep::Stronger, true)]
    );
}

#[test]
fn next_due_names_the_following_step() {
    let mut a = Attention::new();
    let i = one(Level::Normal, Some(T0 + 600 * S));
    assert_eq!(a.next_due_ms(&i, T0), Some(T0));
    run(&mut a, &i, 0);
    assert_eq!(a.next_due_ms(&i, T0 + 5 * S), Some(T0 + 60 * S));
    run(&mut a, &i, 60);
    assert_eq!(a.next_due_ms(&i, T0 + 61 * S), Some(T0 + 180 * S));
    run(&mut a, &i, 180);
    assert_eq!(a.next_due_ms(&i, T0 + 181 * S), Some(T0 + 540 * S));
}

#[test]
fn level_round_trips_through_its_names() {
    for level in [Level::Calm, Level::Normal, Level::Urgent] {
        assert_eq!(Level::parse(level.as_str()), Some(level));
    }
    assert_eq!(Level::parse("loud"), None);
}
