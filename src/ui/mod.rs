//! HUD: buttons, pause control, colony stats and FPS counter.

pub mod fps;
pub mod hud;
pub mod widgets;

use bevy::prelude::*;

use crate::core::sets::{GameSet, Paused};

/// Owns the HUD systems, including the colony stats panel registered here so
/// `simulation` does not depend on `ui`.
pub struct UiPlugin;

impl Plugin for UiPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Paused>()
            .add_systems(Startup, (hud::setup_hud, fps::setup_fps_counter))
            .add_systems(
                Update,
                (
                    hud::handle_button_press,
                    widgets::sync_tool_button_colors,
                    hud::sync_paused_indicator,
                    fps::fps_text_update_system,
                    fps::fps_perf_text_update_system,
                    fps::fps_counter_showhide,
                    hud::update_colony_stats_hud,
                )
                    .chain()
                    .in_set(GameSet::Ui),
            );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::diagnostic::DiagnosticsStore;

    /// `main.rs` adds `PerfPlugin` next to `UiPlugin`; a headless app must come
    /// up with the counters registered and every UI system runnable, including
    /// the colony stats panel that reads simulation resources.
    #[test]
    fn ui_plugin_boots_headless_with_perf_counters() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .init_resource::<crate::editor::EditorMode>()
            .init_resource::<crate::simulation::ant::AntPopulation>()
            .init_resource::<crate::simulation::colony::NestStore>()
            .init_resource::<crate::simulation::colony::ColonyStats>()
            .insert_resource(ButtonInput::<KeyCode>::default())
            .insert_resource(DiagnosticsStore::default())
            .add_plugins(crate::perf::PerfPlugin)
            .add_plugins(UiPlugin);

        app.update();

        assert!(
            app.world()
                .get_resource::<crate::perf::PerfStats>()
                .is_some()
        );
    }
}
