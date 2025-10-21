use crate::PheromoneDisplayState;
use bevy::prelude::*;

pub fn toggle_pheromone_display(
    keyboard: Res<ButtonInput<KeyCode>>,
    mut display_state: ResMut<PheromoneDisplayState>,
) {
    if keyboard.just_pressed(KeyCode::F3) {
        display_state.enabled = !display_state.enabled;
    }
}
