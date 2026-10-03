//! HUD: buttons, pause control and FPS counter.

pub mod fps;
pub mod hud;
pub mod widgets;

use bevy::prelude::*;

use crate::core::sets::{GameSet, Paused};
use crate::perf::PerfPlugin;

pub struct UiPlugin;

impl Plugin for UiPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(PerfPlugin)
            .init_resource::<Paused>()
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

    /// `UiPlugin` owns `PerfPlugin`; a headless app must come up with the
    /// counters registered and every UI system runnable.
    #[test]
    fn ui_plugin_registers_perf_counters_headless() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .init_resource::<crate::editor::EditorMode>()
            .insert_resource(ButtonInput::<KeyCode>::default())
            .insert_resource(DiagnosticsStore::default())
            .add_plugins(UiPlugin);

        app.update();

        assert!(
            app.world()
                .get_resource::<crate::perf::PerfStats>()
                .is_some()
        );
    }
}
