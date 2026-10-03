use bevy::prelude::*;

use crate::pheromone::grid::PheromoneGrid;

pub fn decay_pheromones(mut pheromone_grid: ResMut<PheromoneGrid>, time: Res<Time<Fixed>>) {
    pheromone_grid.step(time.delta_secs());
}
