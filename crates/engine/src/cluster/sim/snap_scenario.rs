use super::invariants::{check, Violation};
use super::scenario::Failure;
use super::snaps::SnapPlant;
use super::world::World;
use crate::cluster::id::NodeId;

const TRACE_TAIL: usize = 40;

pub struct SnapCast {
    pub m5: NodeId,
    pub m1: NodeId,
    pub m2: NodeId,
    pub w1: NodeId,
}

pub fn snap_cast(world: &mut World, plant: SnapPlant) -> SnapCast {
    world.enable_snaps(plant);
    let m5 = world.add_node("m5", true);
    let m1 = world.add_node("m1", true);
    let m2 = world.add_node("m2", true);
    let w1 = world.add_node("w1", false);
    world.create(&m5);
    world.join(&m1, true);
    world.join(&m2, true);
    world.join(&w1, false);
    for id in ["sim-a", "sim-b", "sim-c"] {
        world.snap_add_session(id);
    }
    world.run_for(5_000);
    SnapCast { m5, m1, m2, w1 }
}

fn pick(world: &mut World, from: &[NodeId]) -> NodeId {
    let i = world.rng().below(from.len() as u64) as usize;
    from[i].clone()
}

pub fn run_snap_world(seed: u64, steps: usize, plant: SnapPlant) -> (World, Result<(), Violation>) {
    let mut world = World::new(seed);
    let cast = snap_cast(&mut world, plant);
    let everyone = [
        cast.m5.clone(),
        cast.m1.clone(),
        cast.m2.clone(),
        cast.w1.clone(),
    ];
    let macs = [cast.m5.clone(), cast.m1.clone(), cast.m2.clone()];
    let sessions = world.snap_sessions();
    if let Err(v) = check(&world) {
        return (world, Err(v));
    }
    for _ in 0..steps {
        let roll = world.rng().below(100);
        match roll {
            0..=39 => {
                let id = sessions[world.rng().below(sessions.len() as u64) as usize].clone();
                match world.rng().below(10) {
                    0..=3 => world.snap_session_work(&id),
                    4..=6 => world.snap_session_finish(&id),
                    7 => world.snap_session_pushed(&id, false),
                    8 => world.snap_session_pushed(&id, true),
                    _ => {
                        let on = world.rng().chance(50);
                        world.snap_git_lock(&id, on);
                    }
                }
            }
            40..=46 => {
                let a = pick(&mut world, &everyone);
                let b = pick(&mut world, &everyone);
                if a != b {
                    world.partition(&a, &b);
                }
            }
            47..=54 => world.heal(),
            55..=57 => {
                let n = pick(&mut world, &everyone);
                world.crash(&n);
            }
            58..=62 => {
                let n = pick(&mut world, &everyone);
                world.restart(&n);
            }
            63..=64 => {
                let n = pick(&mut world, &everyone);
                world.suspend(&n);
            }
            65..=68 => {
                let n = pick(&mut world, &everyone);
                world.resume(&n);
            }
            69..=70 => {
                let to = pick(&mut world, &macs);
                world.bump_term(&to);
            }
            71..=76 => {
                let n = pick(&mut world, &macs);
                world.snap_restore(&n);
            }
            _ => {
                let ms = 500 + world.rng().below(8_000);
                world.run_for(ms);
            }
        }
        let settle = 150 + world.rng().below(1_500);
        world.run_for(settle);
        if let Err(v) = check(&world) {
            return (world, Err(v));
        }
    }
    (world, Ok(()))
}

pub fn run_snap_seed(seed: u64, steps: usize, plant: SnapPlant) -> Result<(), Failure> {
    let (world, result) = run_snap_world(seed, steps, plant);
    result.map_err(|violation| {
        let trace = world.trace();
        let from = trace.len().saturating_sub(TRACE_TAIL);
        Failure {
            seed,
            violation,
            trace_tail: trace[from..].to_vec(),
        }
    })
}
