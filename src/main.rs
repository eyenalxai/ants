use bevy::prelude::*;

mod ant_spawner;
mod components;
mod constants;
mod pheromone;
mod systems;

use ant_spawner::AntSpawner;
use constants::{ANT_SPAWN_INTERVAL, WINDOW_HEIGHT, WINDOW_WIDTH};
use pheromone::PheromoneGrid;
use systems::{move_ants, setup, spawn_ants};

fn main() {
    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "Ants Simulation".into(),
                resolution: (WINDOW_WIDTH, WINDOW_HEIGHT).into(),
                resizable: false,
                ..default()
            }),
            ..default()
        }))
        .insert_resource(PheromoneGrid::new())
        .insert_resource(AntSpawner {
            timer: Timer::from_seconds(ANT_SPAWN_INTERVAL, TimerMode::Repeating),
            count: 0,
        })
        .add_systems(Startup, setup)
        .add_systems(Update, (spawn_ants, move_ants))
        .run();
}
