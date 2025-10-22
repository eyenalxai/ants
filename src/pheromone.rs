use crate::constants::{GRID_HEIGHT, GRID_SIZE, GRID_WIDTH, PLAY_AREA_HEIGHT, PLAY_AREA_WIDTH};
use bevy::prelude::*;
use std::collections::HashSet;

#[derive(Clone, Copy, Default)]
pub struct Pheromone {
    pub to_food: f32,
    pub to_nest: f32,
}

#[derive(Resource)]
pub struct PheromoneGrid {
    cells: Vec<Pheromone>,
    active_cells: HashSet<usize>,
}

impl PheromoneGrid {
    pub fn new() -> Self {
        Self {
            cells: vec![Pheromone::default(); GRID_WIDTH * GRID_HEIGHT],
            active_cells: HashSet::new(),
        }
    }

    #[inline]
    fn get_index(&self, x: usize, y: usize) -> usize {
        y * GRID_WIDTH + x
    }

    pub fn world_to_grid(&self, world_pos: Vec2) -> Option<(usize, usize)> {
        let x = ((world_pos.x + PLAY_AREA_WIDTH / 2.0) / GRID_SIZE) as i32;
        let y = ((world_pos.y + PLAY_AREA_HEIGHT / 2.0) / GRID_SIZE) as i32;

        if x >= 0 && x < GRID_WIDTH as i32 && y >= 0 && y < GRID_HEIGHT as i32 {
            Some((x as usize, y as usize))
        } else {
            None
        }
    }

    pub fn get(&self, x: usize, y: usize) -> Option<&Pheromone> {
        if x < GRID_WIDTH && y < GRID_HEIGHT {
            Some(&self.cells[self.get_index(x, y)])
        } else {
            None
        }
    }

    pub fn add_pheromone(
        &mut self,
        x: usize,
        y: usize,
        to_food: f32,
        to_nest: f32,
        max_intensity: f32,
    ) {
        if x < GRID_WIDTH && y < GRID_HEIGHT {
            let idx = self.get_index(x, y);
            let cell = &mut self.cells[idx];
            cell.to_food = (cell.to_food + to_food).min(max_intensity);
            cell.to_nest = (cell.to_nest + to_nest).min(max_intensity);

            if cell.to_food > 0.01 || cell.to_nest > 0.01 {
                self.active_cells.insert(idx);
            }
        }
    }

    pub fn apply_decay(&mut self, decay_rate: f32, delta_time: f32) {
        const THRESHOLD: f32 = 0.01;
        let decay_factor = decay_rate.powf(delta_time * 60.0);

        self.active_cells.retain(|&idx| {
            let pheromone = &mut self.cells[idx];
            pheromone.to_food *= decay_factor;
            pheromone.to_nest *= decay_factor;

            if pheromone.to_food < THRESHOLD {
                pheromone.to_food = 0.0;
            }
            if pheromone.to_nest < THRESHOLD {
                pheromone.to_nest = 0.0;
            }

            pheromone.to_food > 0.0 || pheromone.to_nest > 0.0
        });
    }
}
