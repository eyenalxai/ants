use bevy::prelude::*;

#[derive(Component)]
pub struct Nest;

#[derive(Component)]
pub struct Ant {
    pub direction: f32,
    pub has_food: bool,
    pub lifetime: f32,
    pub max_lifetime: f32,
    pub speed: f32,
}

#[derive(Component)]
pub struct PheromoneCell {
    pub grid_x: usize,
    pub grid_y: usize,
}

#[derive(Component)]
pub struct Wall;
