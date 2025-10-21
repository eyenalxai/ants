use bevy::prelude::*;

#[derive(Component)]
pub struct Nest;

#[derive(Component)]
pub struct Food;

#[derive(Component)]
pub struct Ant {
    pub direction: f32,
}
