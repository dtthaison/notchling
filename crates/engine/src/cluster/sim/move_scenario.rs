use serde_json::Value;

use super::invariants::{check, Violation};
use super::scenario::Failure;
use super::world::{Event, World};
use crate::cluster::core::{Input, LocalCmd, Takeover};
use crate::cluster::id::NodeId;
use crate::cluster::wire::WireError;

const TRACE_TAIL: usize = 40;

pub struct MoveCast {
    pub m5: NodeId,
    pub m1: NodeId,
    pub m2: NodeId,
    pub w1: NodeId,
}

pub fn move_cast(world: &mut World) -> MoveCast {
    let m5 = world.add_node("m5", true);
    let m1 = world.add_node("m1", true);
    let m2 = world.add_node("m2", true);
    let w1 = world.add_node("w1", false);
    world.create(&m5);
    world.join(&m1, true);
    world.join(&m2, true);
    world.join(&w1, false);
    world.run_for(5_000);
    MoveCast { m5, m1, m2, w1 }
}

impl World {
    pub(super) fn run_takeover(&mut self, n: &NodeId, call: u64, args: &Value) {
        let result = match serde_json::from_value::<Takeover>(args.clone()) {
            Ok(takeover) => {
                let id = self.next_local;
                self.next_local += 1;
                self.step(
                    n,
                    Input::Local {
                        id,
                        cmd: LocalCmd::TakePrimary {
                            takeover: Box::new(takeover),
                        },
                    },
                );
                self.answers
                    .iter()
                    .rev()
                    .find(|(node, local, _)| node == n && *local == id)
                    .map(|(_, _, result)| result.clone())
                    .unwrap_or_else(|| Err(WireError::new("failed", "no answer")))
            }
            Err(_) => Err(WireError::new("bad_args", "not a takeover")),
        };
        let at = self.now + 1 + self.rng.below(5);
        self.schedule(
            at,
            Event::LocalDone {
                node: n.clone(),
                call,
                result,
            },
        );
    }

    pub fn move_home(&mut self, from: &NodeId, to: &NodeId) {
        self.local(
            from,
            LocalCmd::MoveSwitch {
                to: to.clone(),
                sessions: Vec::new(),
            },
        );
    }

    pub fn thinks_home(&self, n: &NodeId) -> bool {
        self.core(n).is_some_and(|c| c.is_home())
    }
}

fn pick(world: &mut World, from: &[NodeId]) -> NodeId {
    let i = world.rng().below(from.len() as u64) as usize;
    from[i].clone()
}

pub fn run_move_world(seed: u64, steps: usize) -> (World, Result<(), Violation>) {
    let mut world = World::new(seed);
    let cast = move_cast(&mut world);
    let everyone = [
        cast.m5.clone(),
        cast.m1.clone(),
        cast.m2.clone(),
        cast.w1.clone(),
    ];
    let macs = [cast.m5.clone(), cast.m1.clone(), cast.m2.clone()];
    if let Err(v) = check(&world) {
        return (world, Err(v));
    }
    for _ in 0..steps {
        let roll = world.rng().below(100);
        match roll {
            0..=29 => {
                let from = pick(&mut world, &macs);
                let to = pick(&mut world, &macs);
                if from != to && world.thinks_home(&from) {
                    world.move_home(&from, &to);
                }
            }
            30..=39 => {
                let a = pick(&mut world, &everyone);
                let b = pick(&mut world, &everyone);
                if a != b {
                    world.partition(&a, &b);
                }
            }
            40..=52 => world.heal(),
            53..=56 => {
                let n = pick(&mut world, &everyone);
                world.crash(&n);
            }
            57..=63 => {
                let n = pick(&mut world, &everyone);
                world.restart(&n);
            }
            64..=66 => {
                let n = pick(&mut world, &everyone);
                world.suspend(&n);
            }
            67..=71 => {
                let n = pick(&mut world, &everyone);
                world.resume(&n);
            }
            72..=74 => {
                let to = pick(&mut world, &macs);
                world.bump_term(&to);
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

pub fn run_move_seed(seed: u64, steps: usize) -> Result<(), Failure> {
    let (world, result) = run_move_world(seed, steps);
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
