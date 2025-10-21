use bevy::prelude::*;

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
    pub cells: Vec<(usize, usize)>,
}
