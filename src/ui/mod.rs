//! HUD: buttons, pause control and FPS counter.

pub mod fps;
pub mod hud;
pub mod widgets;

use bevy::prelude::*;

use crate::core::sets::{GameSet, Paused};

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
                    fps::fps_counter_showhide,
                )
                    .chain()
                    .in_set(GameSet::Ui),
            );
    }
}
