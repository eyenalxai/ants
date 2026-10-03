//! Debug overlays: pheromone visualization and the selected-ant sensor cone.

pub mod pheromone;
pub mod sensor_cone;

use bevy::prelude::*;

use crate::core::sets::GameSet;
use crate::simulation::ant::Ant;

#[derive(Resource)]
pub struct PheromoneDisplayState {
    pub enabled: bool,
}

#[derive(Resource)]
pub struct SelectedAnt {
    pub entity: Option<Entity>,
}

/// Owns the overlay resources, pheromone cell sprites and overlay systems.
pub struct OverlayPlugin;

impl Plugin for OverlayPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(PheromoneDisplayState { enabled: false })
            .insert_resource(SelectedAnt { entity: None })
            .add_systems(Startup, pheromone::setup_pheromone_cells)
            .add_systems(
                Update,
                (
                    toggle_pheromone_display,
                    pheromone::update_pheromone_visuals,
                    sensor_cone::draw_sensor_cone,
                )
                    .chain()
                    .in_set(GameSet::Overlay),
            );
    }
}

/// F3 toggles the pheromone overlay and picks a random ant to inspect.
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
                let random_index = (fastrand::f32() * ants.len() as f32) as usize;
                selected_ant.entity = Some(ants[random_index]);
            }
        } else {
            selected_ant.entity = None;
        }
    }
}
