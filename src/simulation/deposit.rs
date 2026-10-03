//! Serial pheromone deposit pass, run once per fixed step after movement.

use bevy::prelude::*;

use crate::constants::ant::{ANT_YOUTH_DEPOSIT_MAX, ANT_YOUTH_DEPOSIT_MIN};
use crate::constants::pheromone::PHEROMONE_DEPOSIT_RATE;
use crate::core::grid::world_to_grid;
use crate::pheromone::grid::PheromoneGrid;
use crate::simulation::ant::Ant;

pub fn deposit_pheromones(
    ant_query: Query<(&Ant, &Transform)>,
    mut pheromone_grid: ResMut<PheromoneGrid>,
    time: Res<Time<Fixed>>,
) {
    let delta = time.delta_secs();

    for (ant, transform) in &ant_query {
        let pos = Vec2::new(transform.translation.x, transform.translation.y);
        let Some(cell) = world_to_grid(pos) else {
            continue;
        };

        let youth_factor = (ant.lifetime / ant.max_lifetime).max(0.0);
        let youth_multiplier =
            ANT_YOUTH_DEPOSIT_MIN + (youth_factor * youth_factor * ANT_YOUTH_DEPOSIT_MAX);
        let deposit_amount = PHEROMONE_DEPOSIT_RATE * delta * youth_multiplier;

        if ant.has_food {
            pheromone_grid.add_kernel(cell, deposit_amount, 0.0);
        } else {
            pheromone_grid.add_kernel(cell, 0.0, deposit_amount);
        }
    }
}
