use std::time::Duration;

const SHARE: f64 = 0.5;
const SHARE_ON_BATTERY: f64 = 0.25;

pub struct Power {
    pub on_battery: bool,
    pub low_power: bool,
    pub thermal_serious: bool,
    pub user_paused: bool,
}

impl From<notchling_platform::power::State> for Power {
    fn from(state: notchling_platform::power::State) -> Power {
        Power {
            on_battery: state.on_battery,
            low_power: state.low_power,
            thermal_serious: state.thermal_serious,
            user_paused: false,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Paused {
    No,
    User,
    LowPower,
    Thermal,
}

#[derive(Default)]
pub struct Budget {
    cpu: Duration,
    wall: Duration,
}

impl Budget {
    pub fn new() -> Budget {
        Budget::default()
    }

    pub fn paused(&self, power: &Power) -> Paused {
        if power.user_paused {
            Paused::User
        } else if power.low_power {
            Paused::LowPower
        } else if power.thermal_serious {
            Paused::Thermal
        } else {
            Paused::No
        }
    }

    pub fn rest_after(&mut self, cpu: Duration, wall: Duration, power: &Power) -> Duration {
        let share = if power.on_battery {
            SHARE_ON_BATTERY
        } else {
            SHARE
        };
        let rest = cpu.div_f64(share).saturating_sub(wall);
        self.cpu += cpu;
        self.wall += wall + rest;
        rest
    }

    pub fn totals(&self) -> (Duration, Duration) {
        (self.cpu, self.wall)
    }
}
