//! HUD: buttons, pause control and FPS counter.

pub mod fps;
pub mod hud;

use bevy::prelude::*;

use crate::core::sets::{GameSet, Paused};

pub struct UiPlugin;

impl Plugin for UiPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Paused>()
            .add_systems(
                Startup,
                (
                    fps::setup_fps_counter,
                    hud::setup_pause_button,
                    hud::setup_food_button,
                    hud::setup_nest_button,
                ),
            )
            .add_systems(
                Update,
                (
                    hud::toggle_pause,
                    hud::toggle_food_management,
                    hud::toggle_nest_management,
                    hud::sync_editor_button_colors,
                    fps::fps_text_update_system,
                    fps::fps_counter_showhide,
                )
                    .chain()
                    .in_set(GameSet::Ui),
            );
    }
}
