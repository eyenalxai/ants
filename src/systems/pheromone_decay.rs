use crate::constants::PHEROMONE_DECAY_RATE;
use crate::pheromone::PheromoneGrid;
use bevy::prelude::*;

pub fn decay_pheromones(mut pheromone_grid: ResMut<PheromoneGrid>, time: Res<Time>) {
    pheromone_grid.apply_decay(PHEROMONE_DECAY_RATE, time.delta_secs());
}
