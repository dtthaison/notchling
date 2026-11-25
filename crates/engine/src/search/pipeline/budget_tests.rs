use super::*;
use std::time::Duration;

fn plugged() -> Power {
    Power {
        on_battery: false,
        low_power: false,
        thermal_serious: false,
        user_paused: false,
    }
}

fn ms(n: u64) -> Duration {
    Duration::from_millis(n)
}

fn simulate(power: &Power, batches: u32, cpu: u64, wall: u64) -> (Duration, Duration) {
    let mut budget = Budget::new();
    let mut total_cpu = Duration::ZERO;
    let mut total_wall = Duration::ZERO;
    for _ in 0..batches {
        let rest = budget.rest_after(ms(cpu), ms(wall), power);
        total_cpu += ms(cpu);
        total_wall += ms(wall) + rest;
    }
    (total_cpu, total_wall)
}

#[test]
fn the_cpu_meter_keeps_half_a_core() {
    let (cpu, wall) = simulate(&plugged(), 100, 40, 40);
    assert!(wall >= cpu * 2 - ms(40), "cpu {cpu:?} wall {wall:?}");
    assert!(
        wall <= cpu * 2 + ms(40),
        "it does not rest longer than it must"
    );
}

#[test]
fn a_batch_that_waited_already_rested() {
    let mut budget = Budget::new();
    assert_eq!(
        budget.rest_after(ms(40), ms(200), &plugged()),
        Duration::ZERO
    );
    assert_eq!(
        budget.rest_after(Duration::ZERO, ms(5), &plugged()),
        Duration::ZERO
    );
}

#[test]
fn battery_and_heat_lower_or_pause() {
    let battery = Power {
        on_battery: true,
        ..plugged()
    };
    let (cpu, wall) = simulate(&battery, 100, 40, 40);
    assert!(wall >= cpu * 4 - ms(40), "cpu {cpu:?} wall {wall:?}");
    assert!(wall <= cpu * 4 + ms(40));

    let budget = Budget::new();
    assert_eq!(budget.paused(&plugged()), Paused::No);
    assert_eq!(budget.paused(&battery), Paused::No);
    assert_eq!(
        budget.paused(&Power {
            low_power: true,
            ..plugged()
        }),
        Paused::LowPower
    );
    assert_eq!(
        budget.paused(&Power {
            thermal_serious: true,
            ..plugged()
        }),
        Paused::Thermal
    );
    assert_eq!(
        budget.paused(&Power {
            user_paused: true,
            low_power: true,
            ..plugged()
        }),
        Paused::User
    );
}
