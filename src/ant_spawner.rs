use bevy::prelude::*;

#[derive(Resource)]
pub struct AntSpawner {
    pub timer: Timer,
    pub count: usize,
}
