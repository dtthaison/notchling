use serde_json::json;

use super::invariants::{check, Violation};
use super::world::World;
use crate::cluster::core::LocalCmd;
use crate::cluster::id::{NodeId, Tool};
use crate::cluster::jobs::JobSpec;
use crate::cluster::wire::{Origin, OriginTier};

const TRACE_TAIL: usize = 40;

#[derive(Clone, Copy, Debug, Default)]
pub struct Plant {
    pub double_start: bool,
    pub skip_term_check: bool,
}

#[derive(Clone, Debug)]
pub struct Failure {
    pub seed: u64,
    pub violation: Violation,
    pub trace_tail: Vec<String>,
}

impl std::fmt::Display for Failure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "seed {} broke {:?}", self.seed, self.violation)?;
        for line in &self.trace_tail {
            writeln!(f, "{line}")?;
        }
        Ok(())
    }
}

struct Cast {
    m5: NodeId,
    m1: NodeId,
    w1: NodeId,
    w2: NodeId,
}

fn setup(world: &mut World) -> Cast {
    let m5 = world.add_node("m5", true);
    let m1 = world.add_node("m1", true);
    let w1 = world.add_node("w1", false);
    let w2 = world.add_node("w2", false);
    world.create(&m5);
    world.join(&m1, true);
    world.join(&w1, false);
    world.join(&w2, false);
    world.run_for(5_000);
    Cast { m5, m1, w1, w2 }
}

fn dispatch(world: &mut World, from: &NodeId, key: &str, node: &NodeId, tool: Tool) {
    let tier = if world.is_client(from) {
        OriginTier::App
    } else {
        OriginTier::Agent
    };
    world.local(
        from,
        LocalCmd::Dispatch {
            key: key.to_string(),
            spec: JobSpec {
                node: node.clone(),
                tool,
                cwd: "/srv/repo".to_string(),
                args: json!({"prompt": "go"}),
                secrets: Vec::new(),
            },
            tier,
        },
    );
}

pub fn run_world(seed: u64, steps: usize, plant: Plant) -> (World, Result<(), Violation>) {
    let mut world = World::new(seed);
    if plant.double_start {
        world.plant_double_start();
    }
    if plant.skip_term_check {
        world.plant_skip_term_check();
    }
    let cast = setup(&mut world);
    let everyone = [
        cast.m5.clone(),
        cast.m1.clone(),
        cast.w1.clone(),
        cast.w2.clone(),
    ];
    let macs = [cast.m5.clone(), cast.m1.clone()];
    let mut keys: Vec<String> = Vec::new();
    if let Err(v) = check(&world) {
        return (world, Err(v));
    }
    for step in 0..steps {
        let roll = world.rng().below(100);
        let pick = |world: &mut World, from: &[NodeId]| -> NodeId {
            let i = world.rng().below(from.len() as u64) as usize;
            from[i].clone()
        };
        match roll {
            0..=29 => {
                let from = pick(&mut world, &everyone);
                let node = pick(&mut world, &everyone);
                let key = if !keys.is_empty() && world.rng().chance(20) {
                    let i = world.rng().below(keys.len() as u64) as usize;
                    keys[i].clone()
                } else {
                    let key = format!("k{step}");
                    keys.push(key.clone());
                    key
                };
                let tool = match world.rng().below(10) {
                    0..=4 => Tool::Codex,
                    5..=7 => Tool::Shell,
                    8 => Tool::Agy,
                    _ => Tool::Claude,
                };
                dispatch(&mut world, &from, &key, &node, tool);
            }
            30..=37 => {
                let a = pick(&mut world, &everyone);
                let b = pick(&mut world, &everyone);
                if a != b {
                    world.partition(&a, &b);
                }
            }
            38..=45 => world.heal(),
            46..=49 => {
                let n = pick(&mut world, &everyone);
                world.crash(&n);
            }
            50..=55 => {
                let n = pick(&mut world, &everyone);
                world.restart(&n);
            }
            56..=59 => {
                let n = pick(&mut world, &everyone);
                world.suspend(&n);
            }
            60..=65 => {
                let n = pick(&mut world, &everyone);
                world.resume(&n);
            }
            66..=68 => {
                let from = pick(&mut world, &macs);
                let worker = if world.rng().chance(50) {
                    cast.w1.clone()
                } else {
                    cast.w2.clone()
                };
                world.local(&from, LocalCmd::Remove { node: worker });
            }
            69..=73 => {
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

pub fn run_seed(seed: u64, steps: usize) -> Result<(), Failure> {
    run_seed_with(seed, steps, Plant::default())
}

pub fn run_seed_with(seed: u64, steps: usize, plant: Plant) -> Result<(), Failure> {
    let (world, result) = run_world(seed, steps, plant);
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

pub fn stale_call_scenario(plant_skip: bool) -> (World, Result<(), Violation>) {
    let mut world = World::new(11);
    if plant_skip {
        world.plant_skip_term_check();
    }
    let m5 = world.add_node("m5", true);
    let m1 = world.add_node("m1", true);
    let w1 = world.add_node("w1", false);
    world.create(&m5);
    world.join(&m1, true);
    world.join(&w1, false);
    world.run_for(5_000);
    world.partition(&m5, &m1);
    world.bump_term(&m1);
    world.run_for(3_000);
    world.local(
        &m5,
        LocalCmd::Forward {
            node: w1.clone(),
            op: "session.reply".to_string(),
            args: json!({"id": "s1", "text": "hello"}),
            origin: Origin {
                node: m5.clone(),
                tier: OriginTier::App,
            },
        },
    );
    world.run_for(3_000);
    let result = check(&world);
    (world, result)
}
