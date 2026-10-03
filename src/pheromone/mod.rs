pub mod decay;
pub mod grid;

use bevy::prelude::*;

use crate::core::sets::GameSet;
use crate::simulation::collide::check_collisions;
use crate::simulation::deposit::deposit_pheromones;

pub use grid::PheromoneGrid;

/// Owns the pheromone grid resource, the deposit pass and decay.
pub struct PheromonePlugin;

impl Plugin for PheromonePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<PheromoneGrid>().add_systems(
            FixedUpdate,
            (deposit_pheromones, decay::decay_pheromones)
                .chain()
                .after(check_collisions)
                .in_set(GameSet::Sim),
        );
    }
}
