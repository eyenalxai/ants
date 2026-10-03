//! App wiring only: window/plugins, global resources and schedule sets.
//!
//! Feature behavior lives in the plugins under `simulation`, `pheromone`,
//! `overlays`, `editor`, `ui` and `perf`.

pub mod constants;
pub mod core;
pub mod editor;
pub mod overlays;
pub mod perf;
pub mod pheromone;
pub mod simulation;
pub mod ui;

use bevy::app::{TaskPoolOptions, TaskPoolPlugin, TaskPoolThreadAssignmentPolicy};
use bevy::diagnostic::FrameTimeDiagnosticsPlugin;
use bevy::prelude::*;

use crate::constants::world::{WINDOW_HEIGHT, WINDOW_WIDTH};
use crate::core::sets::GameSet;

/// Thread assignment for the default pools: the simulation's `move_ants` is
/// the only heavy parallel workload, so the compute pool gets nearly all
/// cores while io/async keep one thread each (the app loads a few assets once
/// and uses no async compute).
fn task_pool_plugin() -> TaskPoolPlugin {
    TaskPoolPlugin {
        task_pool_options: TaskPoolOptions {
            io: TaskPoolThreadAssignmentPolicy {
                min_threads: 1,
                max_threads: 2,
                percent: 0.05,
                on_thread_spawn: None,
                on_thread_destroy: None,
            },
            async_compute: TaskPoolThreadAssignmentPolicy {
                min_threads: 1,
                max_threads: 2,
                percent: 0.05,
                on_thread_spawn: None,
                on_thread_destroy: None,
            },
            compute: TaskPoolThreadAssignmentPolicy {
                min_threads: 1,
                max_threads: usize::MAX,
                percent: 1.0,
                on_thread_spawn: None,
                on_thread_destroy: None,
            },
            ..default()
        },
    }
}

fn main() {
    App::new()
        .add_plugins(
            DefaultPlugins
                .set(WindowPlugin {
                    primary_window: Some(Window {
                        title: "Ants Simulation".into(),
                        resolution: (WINDOW_WIDTH, WINDOW_HEIGHT).into(),
                        resizable: true,
                        present_mode: bevy::window::PresentMode::AutoNoVsync,
                        ..default()
                    }),
                    ..default()
                })
                .set(task_pool_plugin()),
        )
        .add_plugins(FrameTimeDiagnosticsPlugin::default())
        .insert_resource(Time::<Fixed>::from_hz(64.0))
        .add_plugins((
            simulation::SimulationPlugin,
            pheromone::PheromonePlugin,
            overlays::OverlayPlugin,
            editor::EditorPlugin,
            ui::UiPlugin,
            perf::PerfPlugin,
        ))
        // `Ui` writes `EditorMode` on button presses and the editor systems
        // read it (cursor visibility, run conditions), so `Ui` must run before
        // `Editor` for a press to take effect in the same frame. `Overlay` is
        // independent and runs first.
        .configure_sets(
            Update,
            (GameSet::Overlay, GameSet::Ui, GameSet::Editor).chain(),
        )
        .run();
}
