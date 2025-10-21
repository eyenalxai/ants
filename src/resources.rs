use bevy::prelude::*;
use std::collections::HashMap;

#[derive(Resource)]
pub struct PheromoneDisplayState {
    pub enabled: bool,
}

#[derive(Resource)]
pub struct SelectedAnt {
    pub entity: Option<Entity>,
}

#[derive(Resource)]
pub struct FoodCells {
    pub cells: HashMap<(usize, usize), f32>,
    pub world_positions: Vec<Vec2>,
}

impl FoodCells {
    pub fn rebuild_cache(&mut self) {
        use crate::constants::*;

        self.world_positions.clear();
        for &(grid_x, grid_y) in self.cells.keys() {
            let world_x = grid_x as f32 * GRID_SIZE - PLAY_AREA_WIDTH / 2.0 + GRID_SIZE / 2.0;
            let world_y = grid_y as f32 * GRID_SIZE - PLAY_AREA_HEIGHT / 2.0 + GRID_SIZE / 2.0;
            self.world_positions.push(Vec2::new(world_x, world_y));
        }
    }
}

#[derive(Resource)]
pub struct FoodManagementState {
    pub enabled: bool,
}

#[derive(Resource)]
pub struct PauseState {
    pub paused: bool,
}

#[derive(Resource)]
pub struct NestManagementState {
    pub enabled: bool,
    pub dragging: bool,
}
