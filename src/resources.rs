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
}

#[derive(Resource)]
pub struct FoodManagementState {
    pub enabled: bool,
}
