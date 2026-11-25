use super::scenario::{setup, Cast};
use super::secrets::SecretPlant;
use super::world::World;
use crate::cluster::core::LocalCmd;
use crate::cluster::id::NodeId;

pub(super) struct Stage {
    pub world: World,
    pub cast: Cast,
}

pub(super) const VAULT: [(&str, bool); 3] = [("tok-a", false), ("tok-b", true), ("tok-c", false)];

fn allow(world: &mut World, home: &NodeId, node: &NodeId, names: Option<Vec<&str>>, any: bool) {
    let cmd = LocalCmd::SetAllow {
        node: node.clone(),
        names: names.map(|l| l.into_iter().map(str::to_string).collect()),
        any: Some(any),
    };
    world.local(home, cmd);
}

pub(super) fn secrets_setup(world: &mut World, cast: &Cast) {
    world.seed_vault(&cast.m5, &VAULT);
    allow(world, &cast.m5, &cast.w1, None, true);
    allow(
        world,
        &cast.m5,
        &cast.w2,
        Some(vec!["tok-a", "tok-b"]),
        false,
    );
    allow(world, &cast.m5, &cast.m1, Some(vec!["tok-a"]), false);
    world.run_for(800);
}

pub(super) fn stage(seed: u64, plant: SecretPlant) -> Stage {
    let mut world = World::new(seed);
    let cast = setup(&mut world);
    secrets_setup(&mut world, &cast);
    world.plant_secrets(plant);
    Stage { world, cast }
}
