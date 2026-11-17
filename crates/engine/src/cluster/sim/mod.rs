mod host;
mod invariants;
mod move_scenario;
mod net;
mod run;
mod scenario;
mod secrets;
mod secrets_actions;
mod secrets_broker;
mod secrets_check;
mod secrets_inbox;
mod secrets_scan;
mod secrets_scenario;
mod secrets_worker;
mod snap_scenario;
mod snaps;
mod world;

pub use host::SimHost;
pub use invariants::{check, Violation};
pub use move_scenario::{move_cast, run_move_seed, run_move_world, MoveCast, PendingMove};
pub use net::SimNet;
pub use scenario::{run_seed, run_seed_with, run_world, stale_call_scenario, Failure, Plant};
pub use secrets::{LeakAt, SecretPlant, SimSecrets, Stats};
pub use snap_scenario::{run_snap_seed, run_snap_world, snap_cast, SnapCast};
pub use snaps::SnapPlant;
pub use world::World;

#[derive(Clone, Debug)]
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Rng {
        Rng(seed)
    }

    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    pub fn below(&mut self, n: u64) -> u64 {
        if n == 0 {
            return 0;
        }
        self.next() % n
    }

    pub fn chance(&mut self, pct: u8) -> bool {
        self.below(100) < u64::from(pct)
    }
}

#[cfg(test)]
mod move_tests;
#[cfg(test)]
mod sim_invite_tests;
#[cfg(test)]
mod sim_secrets_more_tests;
#[cfg(test)]
mod sim_secrets_tests;
#[cfg(test)]
mod sim_tests;
