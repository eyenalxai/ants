pub mod decay;
pub mod grid;

use bevy::prelude::*;

use crate::core::sets::SimSet;

pub use grid::PheromoneGrid;

/// Owns the pheromone grid resource and the decay pass.
///
/// Deposition lives in `simulation::deposit` and is registered by
/// `SimulationPlugin`, so this plugin does not depend on the `simulation`
/// module.
pub struct PheromonePlugin;

impl Plugin for PheromonePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<grid::PheromoneGrid>()
            .add_systems(FixedUpdate, decay::decay_pheromones.in_set(SimSet::Decay));
    }
}
