use crate::components::Ant;
use crate::constants::*;
use crate::pheromone::PheromoneGrid;
use bevy::prelude::*;

pub fn deposit_pheromone(ant: &Ant, pos: Vec2, pheromone_grid: &mut PheromoneGrid, delta: f32) {
    if let Some((grid_x, grid_y)) = pheromone_grid.world_to_grid(pos) {
        let youth_factor = (ant.lifetime / ant.max_lifetime).max(0.0);
        let youth_multiplier =
            ANT_YOUTH_DEPOSIT_MIN + (youth_factor * youth_factor * ANT_YOUTH_DEPOSIT_MAX);
        let deposit_amount = PHEROMONE_DEPOSIT_RATE * delta * youth_multiplier;

        if ant.has_food {
            pheromone_grid.add_pheromone(
                grid_x,
                grid_y,
                deposit_amount,
                0.0,
                PHEROMONE_MAX_INTENSITY,
            );
        } else {
            pheromone_grid.add_pheromone(
                grid_x,
                grid_y,
                0.0,
                deposit_amount,
                PHEROMONE_MAX_INTENSITY,
            );
        }
    }
}

