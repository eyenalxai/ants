use bevy::prelude::*;

#[derive(Component)]
pub struct Nest;

#[derive(Component)]
pub struct Food;

#[derive(Component)]
pub struct Ant {
    pub direction: f32,
    pub has_food: bool,
}

#[derive(Component)]
pub struct PheromoneCell {
    pub grid_x: usize,
    pub grid_y: usize,
}
