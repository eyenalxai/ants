//! App wiring only: window/plugins, global resources and schedule sets.
//!
//! Feature behavior lives in the plugins under `simulation`, `pheromone`,
//! `overlays`, `editor` and `ui`.

pub mod constants;
pub mod core;
pub mod editor;
pub mod overlays;
pub mod pheromone;
pub mod simulation;
pub mod ui;

use bevy::diagnostic::FrameTimeDiagnosticsPlugin;
use bevy::prelude::*;

use crate::constants::world::{WINDOW_HEIGHT, WINDOW_WIDTH};
use crate::core::sets::GameSet;

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
        .insert_resource(Time::<Fixed>::from_hz(64.0))
        .add_plugins((
            simulation::SimulationPlugin,
            pheromone::PheromonePlugin,
            overlays::OverlayPlugin,
            editor::EditorPlugin,
            ui::UiPlugin,
        ))
        // `Ui` writes `EditorMode` on button presses and the editor systems
        // read it (cursor visibility, run conditions), so `Ui` must run before
        // `Editor` for a press to take effect in the same frame. `Overlay` is
        // independent and runs first.
        .configure_sets(
            Update,
            (GameSet::Overlay, GameSet::Ui, GameSet::Editor).chain(),
        )
        .configure_sets(FixedUpdate, GameSet::Sim)
        .run();
}
