use crate::components::Ant;
use crate::{PheromoneDisplayState, SelectedAnt};
use bevy::prelude::*;

pub fn toggle_pheromone_display(
    keyboard: Res<ButtonInput<KeyCode>>,
    mut display_state: ResMut<PheromoneDisplayState>,
    mut selected_ant: ResMut<SelectedAnt>,
    ant_query: Query<Entity, With<Ant>>,
) {
    if keyboard.just_pressed(KeyCode::F3) {
        display_state.enabled = !display_state.enabled;

        if display_state.enabled {
            let ants: Vec<Entity> = ant_query.iter().collect();
            if !ants.is_empty() {
                let random_index = (rand::random::<f32>() * ants.len() as f32) as usize;
                selected_ant.entity = Some(ants[random_index]);
            }
        } else {
            selected_ant.entity = None;
        }
    }
}
