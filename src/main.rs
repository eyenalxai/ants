use bevy::diagnostic::FrameTimeDiagnosticsPlugin;
use bevy::prelude::*;

mod ant_spawner;
mod components;
mod constants;
mod fps_counter;
mod pheromone;
mod systems;

use ant_spawner::AntSpawner;
use constants::{ANT_SPAWN_INTERVAL, WINDOW_HEIGHT, WINDOW_WIDTH};
use fps_counter::{fps_counter_showhide, fps_text_update_system, setup_fps_counter};
use pheromone::PheromoneGrid;
use systems::{
    check_collisions, decay_pheromones, move_ants, setup, spawn_ants, update_ant_lifetime,
    update_pheromone_visuals,
};

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
        .add_plugins(FrameTimeDiagnosticsPlugin::default())
        .insert_resource(PheromoneGrid::new())
        .insert_resource(AntSpawner {
            timer: Timer::from_seconds(ANT_SPAWN_INTERVAL, TimerMode::Repeating),
            count: 0,
        })
        .add_systems(Startup, (setup, setup_fps_counter))
        .add_systems(
            Update,
            (
                update_ant_lifetime,
                spawn_ants,
                move_ants,
                check_collisions,
                decay_pheromones,
                update_pheromone_visuals,
                fps_text_update_system,
                fps_counter_showhide,
            ),
        )
        .run();
}
