use bevy::diagnostic::FrameTimeDiagnosticsPlugin;
use bevy::prelude::*;
use std::collections::HashMap;

mod ant_spawner;
mod components;
mod constants;
mod fps_counter;
mod pheromone;
mod resources;
mod systems;

use ant_spawner::AntSpawner;
use constants::*;
use fps_counter::{fps_counter_showhide, fps_text_update_system, setup_fps_counter};
use pheromone::PheromoneGrid;
use resources::{FoodCells, FoodManagementState, PauseState, PheromoneDisplayState, SelectedAnt};
use systems::{
    check_collisions, decay_pheromones, draw_sensor_cone, handle_food_clicks, move_ants, setup,
    setup_food_button, setup_pause_button, spawn_ants, toggle_food_management, toggle_pause,
    toggle_pheromone_display, update_ant_lifetime, update_food_cursor, update_food_depletion,
    update_food_visuals, update_pheromone_visuals,
};

fn main() {
    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "Ants Simulation".into(),
                resolution: (WINDOW_WIDTH, WINDOW_HEIGHT).into(),
                resizable: true,
                present_mode: bevy::window::PresentMode::AutoNoVsync,
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
        .insert_resource(PheromoneDisplayState { enabled: false })
        .insert_resource(SelectedAnt { entity: None })
        .insert_resource(FoodCells {
            cells: HashMap::new(),
            world_positions: Vec::new(),
        })
        .insert_resource(FoodManagementState { enabled: false })
        .insert_resource(PauseState { paused: false })
        .add_systems(
            Startup,
            (
                setup,
                setup_fps_counter,
                setup_pause_button,
                setup_food_button,
            ),
        )
        .add_systems(
            Update,
            (
                update_ant_lifetime.run_if(|pause: Res<PauseState>| !pause.paused),
                spawn_ants.run_if(|pause: Res<PauseState>| !pause.paused),
                move_ants.run_if(|pause: Res<PauseState>| !pause.paused),
                check_collisions.run_if(|pause: Res<PauseState>| !pause.paused),
                decay_pheromones.run_if(|pause: Res<PauseState>| !pause.paused),
                update_pheromone_visuals,
                fps_text_update_system,
                fps_counter_showhide,
                toggle_pheromone_display,
                draw_sensor_cone,
                toggle_pause,
                toggle_food_management,
                update_food_cursor,
                handle_food_clicks,
                update_food_depletion,
                update_food_visuals,
            ),
        )
        .run();
}
