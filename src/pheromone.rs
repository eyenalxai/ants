use crate::constants::{GRID_HEIGHT, GRID_SIZE, GRID_WIDTH, WINDOW_HEIGHT, WINDOW_WIDTH};
use bevy::prelude::*;

#[derive(Clone, Copy, Default)]
pub struct Pheromone {
    pub to_food: f32,
    pub to_nest: f32,
}

#[derive(Resource)]
pub struct PheromoneGrid {
    cells: Vec<Vec<Pheromone>>,
}

impl PheromoneGrid {
    pub fn new() -> Self {
        Self {
            cells: vec![vec![Pheromone::default(); GRID_WIDTH]; GRID_HEIGHT],
        }
    }

    pub fn world_to_grid(&self, world_pos: Vec2) -> Option<(usize, usize)> {
        let x = ((world_pos.x + WINDOW_WIDTH as f32 / 2.0) / GRID_SIZE) as i32;
        let y = ((world_pos.y + WINDOW_HEIGHT as f32 / 2.0) / GRID_SIZE) as i32;

        if x >= 0 && x < GRID_WIDTH as i32 && y >= 0 && y < GRID_HEIGHT as i32 {
            Some((x as usize, y as usize))
        } else {
            None
        }
    }

    pub fn get(&self, x: usize, y: usize) -> Option<&Pheromone> {
        self.cells.get(y).and_then(|row| row.get(x))
    }

    pub fn get_mut(&mut self, x: usize, y: usize) -> Option<&mut Pheromone> {
        self.cells.get_mut(y).and_then(|row| row.get_mut(x))
    }
}
